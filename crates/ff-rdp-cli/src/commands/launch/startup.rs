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
    spawn_failures: u64,
    first_spawn_failure: Option<Instant>,
    /// The last `marker.len() - 1` bytes read, for markers split across reads.
    marker_carry: Vec<u8>,
}

/// What Firefox's parent process prints when its sandbox launcher cannot
/// start a tab (content) process — on lenny and, by the identical symptom,
/// probably on GitHub's windows-latest runner
/// (`[Parent …] WARNING: Failed to launch tab subprocess @SB::LA::SpawnTarget
/// (Error:-2147024809)`, kb/research/windows-lenny-baseline-2026-11.md). With
/// no content process the first tab never gets a window global and `listTabs`
/// keeps failing. Windows-only in practice; elsewhere it simply never appears.
pub(super) const CONTENT_SPAWN_FAILURE_MARKER: &[u8] = b"Failed to launch tab subprocess";

/// The `listTabs` error a tab without a window global produces.
pub(super) const NO_WINDOW_GLOBAL_ERROR: &str = "currentWindowGlobal is null";

/// How long after the first [`CONTENT_SPAWN_FAILURE_MARKER`] a first tab may
/// still come up before the instance counts as dead on arrival.
///
/// A single failed spawn does not mean a dead instance: lenny saw one failed
/// spawn next to seven healthy content processes, and that tab came up. The
/// marker appears ~0.8 s after the spawn; healthy headless launches on lenny
/// list their first tab 2.4–3.1 s after the spawn (5.8 s at worst, on the
/// first run after a Firefox update), headed ones within 4.7 s. 8 s after the
/// first marker is past every healthy launch seen, while a dead instance
/// would otherwise hold the whole 30 s launch bound.
pub(super) const CONTENT_SPAWN_GRACE: Duration = Duration::from_secs(8);

/// Is this Firefox dead on arrival: its sandbox reported failing to start a
/// tab process, `listTabs` still fails because the first tab has no window
/// global, and the grace period since the first failure is over?
///
/// Without the marker this is always `false`: a slow tab keeps the plain
/// "listed no tab" path and its error text.
pub(super) fn content_processes_dead(
    last_list_tabs_error: Option<&str>,
    spawn_failures: u64,
    since_first_spawn_failure: Option<Duration>,
) -> bool {
    spawn_failures > 0
        && since_first_spawn_failure.is_some_and(|elapsed| elapsed >= CONTENT_SPAWN_GRACE)
        && last_list_tabs_error.is_some_and(|error| error.contains(NO_WINDOW_GLOBAL_ERROR))
}

fn count_occurrences(haystack: &[u8], needle: &[u8]) -> u64 {
    if needle.is_empty() || haystack.len() < needle.len() {
        return 0;
    }
    let mut count = 0;
    let mut i = 0;
    while i + needle.len() <= haystack.len() {
        if haystack[i..].starts_with(needle) {
            count += 1;
            i += needle.len();
        } else {
            i += 1;
        }
    }
    count
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
        // Taken out for the turn so `ingest` can borrow `self`; always put back.
        let Some(mut pipe) = self.pipe.take() else {
            return;
        };
        self.pump_from(&mut pipe, deadline);
        self.pipe = Some(pipe);
    }

    fn pump_from(&mut self, pipe: &mut ChildStderr, deadline: Option<Instant>) {
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
                    self.ingest(&buffer[..n]);
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

    /// Retain `chunk` in the bounded tail and count child-spawn failure
    /// markers in it. A marker split across two reads is counted once: the
    /// search covers the previous read's last `marker.len() - 1` bytes (too
    /// short to hold a whole marker) plus `chunk`.
    fn ingest(&mut self, chunk: &[u8]) {
        self.observed = self.observed.saturating_add(chunk.len() as u64);
        let remove = (self.tail.len() + chunk.len()).saturating_sub(RETAIN);
        self.tail.drain(..remove);
        self.tail.extend(chunk);
        let mut window = std::mem::take(&mut self.marker_carry);
        window.extend_from_slice(chunk);
        let found = count_occurrences(&window, CONTENT_SPAWN_FAILURE_MARKER);
        let keep_from = window
            .len()
            .saturating_sub(CONTENT_SPAWN_FAILURE_MARKER.len() - 1);
        window.drain(..keep_from);
        self.marker_carry = window;
        if found > 0 {
            self.spawn_failures = self.spawn_failures.saturating_add(found);
            self.first_spawn_failure.get_or_insert_with(Instant::now);
        }
    }

    /// How many times Firefox's stderr so far reported that its sandbox could
    /// not start a tab (content) process ([`CONTENT_SPAWN_FAILURE_MARKER`]).
    /// Counted on the raw bytes, so it works while the text stays redacted.
    pub(super) fn spawn_failures(&self) -> u64 {
        self.spawn_failures
    }

    /// Time since the first spawn-failure marker was read, if one was.
    pub(super) fn since_first_spawn_failure(&self) -> Option<Duration> {
        self.first_spawn_failure.map(|at| at.elapsed())
    }

    pub(super) fn summary(&self) -> String {
        let missing = if self.pipe.is_none() {
            "unavailable"
        } else if self.eof {
            "EOF observed"
        } else {
            "unread bytes unknown"
        };
        // iter-296: say how to see the redacted text — the `launch_startup`
        // trace logs it at debug level, unredacted only with the raw opt-in.
        format!(
            "startup stderr: {} bytes observed, {} retained (content redacted), {} discarded; {missing}; read error {:?}; to see it, rerun with --log-level debug and FF_RDP_TRACE_RAW=1",
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
#[expect(
    clippy::unnecessary_wraps,
    reason = "signature matches the fallible Unix `configure`; Windows reads nonblockingly via PeekNamedPipe"
)]
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
        if error.raw_os_error() == Some(ERROR_BROKEN_PIPE.cast_signed()) {
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

#[cfg(test)]
mod tests {
    use super::*;

    const LINE: &[u8] = b"[Parent 25704, IPC I/O Parent] WARNING: Failed to launch tab subprocess @SB::LA::SpawnTarget (Error:-2147024809): file checkouts\\gecko\\ipc\\glue\\GeckoChildProcessHost.cpp:978\n";

    #[test]
    fn spawn_failure_markers_are_counted_across_reads_and_redaction() {
        let mut capture = StderrCapture::default();
        capture.ingest(b"*** You are running in headless mode.\n");
        assert_eq!(capture.spawn_failures(), 0);
        assert!(capture.since_first_spawn_failure().is_none());
        capture.ingest(LINE);
        // A marker split across two reads still counts once.
        let split = LINE.len() / 2;
        capture.ingest(&LINE[..split]);
        capture.ingest(&LINE[split..]);
        // Other subprocess kinds are not the tab marker.
        capture.ingest(b"WARNING: Failed to launch socket subprocess\n");
        assert_eq!(capture.spawn_failures(), 2);
        assert!(capture.since_first_spawn_failure().is_some());
        // The summary never shows the text, only counts.
        assert!(!capture.summary().contains("Failed to launch"));
    }

    #[test]
    fn markers_keep_counting_after_the_tail_wraps() {
        let mut capture = StderrCapture::default();
        for _ in 0..1000 {
            capture.ingest(LINE);
        }
        assert_eq!(capture.spawn_failures(), 1000);
        assert!(capture.tail.len() <= RETAIN);
    }

    #[test]
    fn dead_on_arrival_needs_marker_error_and_grace() {
        let error = "TypeError — can't access property \"outerWindowId\", \
                     browsingContext.currentWindowGlobal is null";
        let past = Some(CONTENT_SPAWN_GRACE);
        assert!(content_processes_dead(Some(error), 17, past));
        // The partial case: a failed spawn, but still inside the grace period.
        assert!(!content_processes_dead(
            Some(error),
            1,
            CONTENT_SPAWN_GRACE.checked_sub(Duration::from_millis(1))
        ));
        // No marker: the existing "listed no tab" path, whatever the error.
        assert!(!content_processes_dead(Some(error), 0, None));
        // A marker but another listTabs failure (or none) is not this case.
        assert!(!content_processes_dead(
            Some("connection refused"),
            17,
            past
        ));
        assert!(!content_processes_dead(None, 17, past));
    }
}
