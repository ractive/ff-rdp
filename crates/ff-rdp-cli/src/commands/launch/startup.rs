//! Bounded startup-only stderr observation. No reader thread survives launch.
use std::collections::VecDeque;
use std::io::{self, Read};
use std::process::{Child, ChildStderr, ExitStatus};
use std::time::{Duration, Instant};

const RETAIN: usize = 64 * 1024;
const TURN_BYTES: usize = 256 * 1024;
const TURN_READS: usize = 64;
pub(super) type TryWait = fn(&mut Child) -> io::Result<Option<ExitStatus>>;

#[derive(Default)]
pub(super) struct StderrCapture {
    pipe: Option<ChildStderr>,
    tail: VecDeque<u8>,
    observed: u64,
    eof: bool,
    read_error: Option<io::ErrorKind>,
    quota_hits: u64,
}

impl StderrCapture {
    pub(super) fn attach(&mut self, pipe: Option<ChildStderr>) -> io::Result<()> {
        self.pipe = pipe;
        if let Some(pipe) = self.pipe.as_ref()
            && let Err(error) = configure(pipe)
        {
            self.read_error = Some(error.kind());
            return Err(error);
        }
        Ok(())
    }

    /// One finite nonblocking turn, also safe after a child exits while a
    /// descendant still owns a writer. Interrupted reads spend this same budget.
    pub(super) fn pump(&mut self, deadline: Option<Instant>) {
        if self.eof || self.read_error.is_some() {
            return;
        }
        let Some(pipe) = self.pipe.as_mut() else {
            return;
        };
        let mut bytes = 0;
        let mut reads = 0;
        let mut buffer = [0_u8; 8192];
        while bytes < TURN_BYTES && reads < TURN_READS {
            if deadline.is_some_and(|end| Instant::now() >= end) {
                return;
            }
            reads += 1;
            match read_available(pipe, &mut buffer) {
                Ok(0) => {
                    self.eof = true;
                    return;
                }
                Ok(n) => {
                    bytes += n;
                    self.observed = self.observed.saturating_add(n as u64);
                    let remove = (self.tail.len() + n).saturating_sub(RETAIN);
                    self.tail.drain(..remove);
                    self.tail.extend(&buffer[..n]);
                }
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => return,
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                Err(error) => {
                    self.read_error = Some(error.kind());
                    return;
                }
            }
        }
        self.quota_hits = self.quota_hits.saturating_add(1);
    }

    pub(super) fn summary(&self) -> String {
        let missing = if self.pipe.is_none() {
            "unavailable"
        } else if self.eof {
            "EOF observed"
        } else {
            "unread bytes unknown"
        };
        format!(
            "startup stderr: {} bytes observed, {} retained (content redacted), {} discarded; {missing}; read error {:?}",
            self.observed,
            self.tail.len(),
            self.observed.saturating_sub(self.tail.len() as u64),
            self.read_error
        )
    }

    pub(super) fn trace(&self, pid: u32, phase: &str, state: &str, elapsed: Option<Duration>) {
        let tail: Vec<u8> = self.tail.iter().copied().collect();
        // `text` is unconditionally redacted by the existing transport trace
        // policy unless the user explicitly enables its raw-trace opt-in.
        let payload = ff_rdp_core::transport::redact(&serde_json::json!({
            "text": String::from_utf8_lossy(&tail),
            "invalid_utf8": std::str::from_utf8(&tail).is_err(),
        }));
        tracing::debug!(target: "ff_rdp_cli::launch_startup", pid, phase, state,
            elapsed_ms = ?elapsed.map(|value| value.as_millis()), observed_bytes = self.observed,
            retained_bytes = self.tail.len(), discarded_bytes = self.observed.saturating_sub(self.tail.len() as u64),
            eof = self.eof, unavailable = self.pipe.is_none(), unread_bytes_unknown = !self.eof,
            quota_hits = self.quota_hits, read_error = ?self.read_error,
            stderr = %payload, "Firefox startup observation");
    }
}

pub(crate) struct Observation<'a> {
    pub(super) child: &'a mut Child,
    pub(super) stderr: &'a mut StderrCapture,
    pub(super) try_wait: TryWait,
    pub(super) started: Instant,
}

impl Observation<'_> {
    pub(super) fn status(&mut self) -> io::Result<Option<ExitStatus>> {
        (self.try_wait)(self.child)
    }

    /// Preserve the original initial500ms interval; merely use its otherwise
    /// idle time to drain and notice death. This is not a second launch budget.
    pub(super) fn initial_interval(&mut self) -> io::Result<Option<ExitStatus>> {
        let deadline = Instant::now() + Duration::from_millis(500);
        loop {
            self.stderr.pump(Some(deadline));
            let status = self.status()?;
            if status.is_some() {
                return Ok(status);
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Ok(None);
            }
            std::thread::sleep(remaining.min(Duration::from_millis(200)));
        }
    }

    pub(super) fn terminal(&mut self, phase: &str, state: &str) {
        self.stderr.pump(None);
        self.stderr
            .trace(self.child.id(), phase, state, Some(self.started.elapsed()));
    }
}

#[cfg(unix)]
#[allow(unsafe_code)]
fn configure(pipe: &ChildStderr) -> io::Result<()> {
    use std::os::fd::AsRawFd;
    // SAFETY: the live owned read endpoint is not closed or shared with another
    // reader here. Preserve all existing flags; this never changes child writes.
    let flags = unsafe { libc::fcntl(pipe.as_raw_fd(), libc::F_GETFL) };
    if flags < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: same live descriptor and flags from F_GETFL above.
    if unsafe { libc::fcntl(pipe.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(not(unix))]
fn configure(_pipe: &ChildStderr) -> io::Result<()> {
    Ok(())
}

#[cfg(unix)]
fn read_available(pipe: &mut ChildStderr, buffer: &mut [u8]) -> io::Result<usize> {
    pipe.read(buffer)
}

#[cfg(windows)]
#[allow(unsafe_code)]
fn read_available(pipe: &mut ChildStderr, buffer: &mut [u8]) -> io::Result<usize> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::{Foundation::ERROR_BROKEN_PIPE, System::Pipes::PeekNamedPipe};
    let mut available = 0_u32;
    // SAFETY: PeekNamedPipe receives a live owned pipe handle and a valid output
    // count. No data buffer is requested; this is the sole reader.
    let ok = unsafe {
        PeekNamedPipe(
            pipe.as_raw_handle(),
            std::ptr::null_mut(),
            0,
            std::ptr::null_mut(),
            &raw mut available,
            std::ptr::null_mut(),
        )
    };
    if ok == 0 {
        let error = io::Error::last_os_error();
        if error.raw_os_error() == Some(ERROR_BROKEN_PIPE as i32) {
            return Ok(0);
        }
        return Err(error);
    }
    let length = buffer.len().min(available as usize);
    if length == 0 {
        return Err(io::ErrorKind::WouldBlock.into());
    }
    pipe.read(&mut buffer[..length])
}

#[cfg(not(any(unix, windows)))]
fn read_available(_pipe: &mut ChildStderr, _buffer: &mut [u8]) -> io::Result<usize> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "nonblocking startup stderr unavailable",
    ))
}
