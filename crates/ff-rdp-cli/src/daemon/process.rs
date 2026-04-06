use std::fs::File;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};

use super::registry::{self, DaemonInfo};

// ---------------------------------------------------------------------------
// PID liveness
// ---------------------------------------------------------------------------

/// Return `true` if a process with `pid` is currently alive.
///
/// On Unix this sends signal 0 (no-op) to the process; on Windows it tries
/// to open a handle with `PROCESS_QUERY_LIMITED_INFORMATION`.  On other
/// platforms it conservatively returns `true`.
pub fn is_process_alive(pid: u32) -> bool {
    #[cfg(unix)]
    {
        // SAFETY: `kill(pid, 0)` never delivers a signal — it only checks
        // whether the process exists and we have permission to signal it.
        // The return value and `errno` are the only observable side effects.
        // The cast from u32 to i32 (pid_t) is intentional: POSIX mandates
        // pid_t is signed, and we clamp to the valid range the OS accepts.
        #[allow(clippy::cast_possible_wrap)]
        let rc = unsafe { libc::kill(pid as libc::pid_t, 0) };
        rc == 0
    }

    #[cfg(windows)]
    {
        use windows_sys::Win32::Foundation::CloseHandle;
        use windows_sys::Win32::System::Threading::{
            OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
        };

        // SAFETY: `OpenProcess` is an FFI call whose only side effect is
        // returning a handle (or NULL on failure).  We close the handle
        // immediately after checking it.
        let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
        if handle == 0 {
            return false;
        }
        // SAFETY: `handle` is a valid, non-null handle we just obtained.
        unsafe { CloseHandle(handle) };
        true
    }

    #[cfg(not(any(unix, windows)))]
    {
        // Conservative: assume the process is alive on unknown platforms.
        let _ = pid;
        true
    }
}

// ---------------------------------------------------------------------------
// Daemon spawning
// ---------------------------------------------------------------------------

/// Spawn the daemon as a fully detached background process.
///
/// The child process runs:
/// ```text
/// ff-rdp _daemon --host <firefox_host> --port <firefox_port>
///                --daemon-timeout <timeout_secs>
/// ```
/// Both `stdout` and `stderr` are redirected to the daemon log file
/// (`~/.ff-rdp/daemon.log`).  The daemon is detached from the current
/// terminal session so it survives the parent process exiting.
pub fn spawn_daemon(
    exe_path: &Path,
    firefox_host: &str,
    firefox_port: u16,
    timeout_secs: u64,
) -> Result<()> {
    let log_path = registry::log_path()?;
    let log_file = File::create(&log_path)
        .with_context(|| format!("creating daemon log file {}", log_path.display()))?;
    let stderr_file = log_file
        .try_clone()
        .context("cloning log file handle for stderr")?;

    let mut cmd = Command::new(exe_path);
    cmd.args([
        "_daemon",
        "--host",
        firefox_host,
        "--port",
        &firefox_port.to_string(),
        "--daemon-timeout",
        &timeout_secs.to_string(),
    ])
    .stdout(log_file)
    .stderr(stderr_file)
    .stdin(Stdio::null());

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt as _;
        // SAFETY: `setsid()` creates a new session, detaching the child from
        // the controlling terminal.  It has no memory-safety implications; it
        // only changes kernel process-group state.  The closure runs in the
        // child after `fork()` and before `exec()`, which is the correct place
        // for this call.
        unsafe {
            cmd.pre_exec(|| {
                libc::setsid();
                Ok(())
            });
        }
    }

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt as _;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }

    cmd.spawn().context("failed to spawn daemon process")?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Registry polling
// ---------------------------------------------------------------------------

/// Poll `~/.ff-rdp/daemon.json` every 50 ms until it appears and contains a
/// valid `DaemonInfo`, or until `timeout` elapses.
///
/// Returns an error if the timeout is exceeded or if the registry cannot be
/// read.
pub fn wait_for_registry(timeout: Duration) -> Result<DaemonInfo> {
    let deadline = Instant::now() + timeout;
    loop {
        match registry::read_registry() {
            Ok(Some(info)) => return Ok(info),
            Ok(None) => {}
            Err(e) => return Err(e).context("reading daemon registry while waiting"),
        }
        if Instant::now() >= deadline {
            anyhow::bail!("timed out after {timeout:?} waiting for daemon to write registry");
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_process_is_alive() {
        let pid = std::process::id();
        assert!(
            is_process_alive(pid),
            "current process (PID {pid}) should be detected as alive"
        );
    }

    #[test]
    fn very_large_pid_is_dead() {
        // PID 999_999_999 is astronomically unlikely to exist on any platform.
        assert!(
            !is_process_alive(999_999_999),
            "PID 999_999_999 should be detected as dead"
        );
    }
}
