//! The inheritable fd exists only in a dedicated subprocess, never parallel libtest.
#![allow(unsafe_code)]

use super::exclude_inherited;
use std::{
    fs::File,
    io::{self, Read, Write},
    net::{TcpListener, TcpStream},
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::process::CommandExt,
    },
    process::{Child, Command, ExitStatus, Stdio},
    time::{Duration, Instant},
};

const TEST: &str = "util::child_fds::tests::unit_282_inherited_fd_exclusion";
const ROLE: &str = "FF_RDP_282_FD_FIXTURE_ROLE";
const BOUND: Duration = Duration::from_secs(5);

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

struct OwnedChild(Child);
impl Drop for OwnedChild {
    fn drop(&mut self) {
        if !matches!(self.0.try_wait(), Ok(Some(_))) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}

fn wait(child: &mut Child, bound: Duration) -> io::Result<ExitStatus> {
    let deadline = Instant::now() + bound;
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(status);
        }
        if Instant::now() >= deadline {
            child.kill()?;
            let status = child.wait()?;
            return Err(io::Error::other(format!(
                "owned fixture deadline; killed and waited: {status}"
            )));
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn command(role: &str) -> io::Result<Command> {
    let mut command = Command::new(std::env::current_exe()?);
    command
        .args(["--exact", TEST, "--nocapture", "--test-threads=1"])
        .env(ROLE, role);
    Ok(command)
}

fn readable(fd: i32, timeout_ms: i32) -> io::Result<bool> {
    let mut pollfd = libc::pollfd {
        fd,
        events: libc::POLLIN,
        revents: 0,
    };
    // SAFETY: one initialized writable pollfd, bounded wait, descriptor remains owned.
    let result = unsafe { libc::poll(&raw mut pollfd, 1, timeout_ms) };
    if result < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(result > 0)
}

fn fixture_child() -> Result<()> {
    let mut control = TcpStream::connect(std::env::var("FF_RDP_282_FD_CONTROL")?)?;
    control.set_read_timeout(Some(BOUND))?;
    control.set_write_timeout(Some(BOUND))?;
    eprintln!("282-stderr-preserved");
    control.write_all(b"ready")?;
    let mut message = [0; 4];
    control.read_exact(&mut message)?;
    if &message != b"ping" {
        return Err("expected ping".into());
    }
    control.write_all(b"pong")?;
    control.read_exact(&mut message)?;
    if &message != b"quit" {
        return Err("expected quit".into());
    }
    Ok(())
}

fn isolated_parent() -> Result<()> {
    let mut limit = libc::rlimit {
        rlim_cur: 0,
        rlim_max: 0,
    };
    // SAFETY: initialized writable limit, changed only in the dedicated fixture.
    if unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, &raw mut limit) } != 0 {
        return Err(io::Error::last_os_error().into());
    }
    limit.rlim_cur = 2048;
    if unsafe { libc::setrlimit(libc::RLIMIT_NOFILE, &raw const limit) } != 0 {
        return Err(io::Error::last_os_error().into());
    }
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let mut pipe = [-1; 2];
    // SAFETY: output array has space for both owned descriptors.
    if unsafe { libc::pipe(pipe.as_mut_ptr()) } != 0 {
        return Err(io::Error::last_os_error().into());
    }
    // SAFETY: pipe() just transferred these two unique descriptors to us.
    let mut reader = unsafe { File::from_raw_fd(pipe[0]) };
    let original_writer = unsafe { File::from_raw_fd(pipe[1]) };
    // Above the fallback ceiling after lowering the soft limit; no race required.
    // SAFETY: valid owned source fd and a positive minimum descriptor.
    let high = unsafe { libc::fcntl(original_writer.as_raw_fd(), libc::F_DUPFD, 1100) };
    if high < 0 {
        return Err(io::Error::last_os_error().into());
    }
    // SAFETY: fcntl returned a new unique descriptor, owned here.
    let writer = unsafe { File::from_raw_fd(high) };
    drop(original_writer);
    // SAFETY: only this isolated process owns this deliberate inheritable fd.
    if unsafe { libc::fcntl(high, libc::F_SETFD, 0) } < 0 {
        return Err(io::Error::last_os_error().into());
    }
    let flags = unsafe { libc::fcntl(high, libc::F_GETFD) };
    assert_eq!(flags & libc::FD_CLOEXEC, 0);
    assert!(flags >= 0 && high >= 1100);
    limit.rlim_cur = 64;
    if unsafe { libc::setrlimit(libc::RLIMIT_NOFILE, &raw const limit) } != 0 {
        return Err(io::Error::last_os_error().into());
    }

    let mut cmd = command("child")?;
    cmd.env("FF_RDP_282_FD_CONTROL", listener.local_addr()?.to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .process_group(0);
    exclude_inherited(&mut cmd);
    let mut child = OwnedChild(cmd.spawn()?);
    drop(writer);
    let observation = (|| -> Result<(bool, bool)> {
        if !readable(listener.as_raw_fd(), 5000)? {
            return Err("child readiness deadline".into());
        }
        let (mut control, _) = listener.accept()?;
        control.set_read_timeout(Some(BOUND))?;
        control.set_write_timeout(Some(BOUND))?;
        let mut ready = [0; 5];
        control.read_exact(&mut ready)?;
        if &ready != b"ready" {
            return Err("invalid readiness".into());
        }
        // SAFETY: pid comes from our owned, not-yet-waited child.
        let group_ok =
            unsafe { libc::getpgid(i32::try_from(child.0.id())?) } == i32::try_from(child.0.id())?;
        // The post-exec handshake proves descriptor preparation is complete.
        // EOF must be available now; waiting would only hide inheritance.
        let eof = if readable(reader.as_raw_fd(), 0)? {
            let mut byte = [0];
            reader.read(&mut byte)? == 0
        } else {
            false
        };
        control.write_all(b"ping")?;
        let mut pong = [0; 4];
        control.read_exact(&mut pong)?;
        let alive = &pong == b"pong" && child.0.try_wait()?.is_none();
        control.write_all(b"quit")?;
        Ok((eof, group_ok && alive))
    })();
    let status = wait(&mut child.0, BOUND)?;
    let mut stderr = String::new();
    child
        .0
        .stderr
        .take()
        .ok_or("missing stderr")?
        .read_to_string(&mut stderr)?;
    eprintln!("282 owned child actual wait: {status}; stderr={stderr}");
    let (eof, alive_and_group) = observation?;
    eprintln!(
        "282 observed eof={eof}, alive_and_group={alive_and_group}, inherited_fd={high}, soft_limit={}",
        limit.rlim_cur
    );
    assert!(
        status.success(),
        "fixture child status={status}, stderr={stderr}"
    );
    assert!(stderr.contains("282-stderr-preserved"), "stderr={stderr}");
    assert!(
        alive_and_group,
        "child must respond while alive in its own group"
    );
    assert!(
        eof,
        "inheritable writer prevented EOF while child was positively alive; child now waited"
    );

    let mut missing = Command::new("/ff-rdp-282-missing-executable");
    exclude_inherited(&mut missing);
    let error = match missing.spawn() {
        Err(error) => error,
        Ok(mut unexpected) => {
            let status = wait(&mut unexpected, BOUND)?;
            panic!("missing executable returned a child; actual wait={status}");
        }
    };
    assert_eq!(
        error.raw_os_error(),
        Some(libc::ENOENT),
        "spawn error={error}"
    );
    Ok(())
}

#[test]
fn unit_282_inherited_fd_exclusion() -> Result<()> {
    match std::env::var(ROLE).as_deref() {
        Ok("child") => fixture_child(),
        Ok("parent") => isolated_parent(),
        _ => {
            let mut command = command("parent")?;
            // Output is inherited so outer timeout cannot hide diagnostics behind EOF.
            let mut fixture = OwnedChild(command.spawn()?);
            let status = wait(&mut fixture.0, Duration::from_secs(15))?;
            assert!(status.success(), "isolated inherited-fd fixture: {status}");
            Ok(())
        }
    }
}
