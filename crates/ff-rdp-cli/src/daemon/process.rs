// iter-105 Theme D: this module is the concentrated home of the CLI's OS
// process-management FFI (unix `libc::kill`/`getpgid`/`setsid`, Windows
// `OpenProcess`/`TerminateProcess`).  The crate default is
// `unsafe_code = "deny"`; this narrow, file-scoped allowance keeps the audited,
// `// SAFETY:`-documented FFI compiling while the rest of the crate still denies
// unsafe.  Every `unsafe` block below carries its own SAFETY justification.
#![allow(unsafe_code)]

use std::fs::{File, OpenOptions};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};

use super::registry::{self, DaemonInfo};

/// Process-group ID type, aliased so callers don't need to gate on `cfg(unix)`
/// themselves. `libc` (and therefore `libc::pid_t`) is only a dependency on
/// Unix targets; on other platforms `get_process_group_id` always returns
/// `None`, so the concrete integer type is never observed — `i32` just needs
/// to exist and be the same on both sides of the alias.
#[cfg(unix)]
pub(crate) type Pgid = libc::pid_t;
#[cfg(not(unix))]
pub(crate) type Pgid = i32;

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
        if handle.is_null() {
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
// Process signaling
// ---------------------------------------------------------------------------

/// Kill the entire process group of `pid` using SIGTERM (Unix).
///
/// On Unix we send SIGTERM to the negative PID (the process group leader).
/// This reaches Firefox's child processes (GPU, RDD, etc.) so the port is
/// actually freed instead of just the parent shell wrapper exiting.
///
/// On Windows there is no POSIX process-group concept; we fall back to
/// terminating just the parent PID (same as `kill_process`).
///
/// Errors are silently ignored — the caller checks PID liveness separately.
pub fn kill_process_group(pid: u32) {
    #[cfg(unix)]
    {
        // SAFETY: `kill(-pgid, SIGTERM)` sends SIGTERM to all processes in the
        // process group.  We use the PID as the PGID because Firefox calls
        // `setsid()` making itself a session leader whose PGID equals its PID.
        // The cast from u32 to pid_t (i32) is intentional; the OS accepts any
        // valid PGID and we received this PID from the OS registry.
        #[allow(clippy::cast_possible_wrap)]
        unsafe {
            libc::kill(-(pid as libc::pid_t), libc::SIGTERM);
        }
    }

    #[cfg(windows)]
    {
        // Windows has no POSIX process groups — fall back to killing the parent.
        kill_process(pid);
    }

    #[cfg(not(any(unix, windows)))]
    {
        let _ = pid;
    }
}

/// Forcibly kill the process group of `pid` using SIGKILL (Unix).
///
/// Used as a last resort after the SIGTERM grace period expires.
/// On Windows, falls back to `kill_process`.
///
/// **Note:** This function derives the PGID by assuming `pid == pgid`, which
/// is true when Firefox called `setsid()` at startup. If the parent has already
/// exited by the time this is called, the PGID may have been re-assigned by the
/// OS. Prefer [`kill_process_tree`] when a pre-captured pgid is
/// available — it targets the correct group even after the parent dies.
pub fn kill_process_group_force(pid: u32) {
    #[cfg(unix)]
    {
        // SAFETY: Same rationale as `kill_process_group`; SIGKILL cannot be caught
        // or ignored, so it is guaranteed to terminate the process group.
        #[allow(clippy::cast_possible_wrap)]
        unsafe {
            libc::kill(-(pid as libc::pid_t), libc::SIGKILL);
        }
    }

    #[cfg(windows)]
    {
        kill_process(pid);
    }

    #[cfg(not(any(unix, windows)))]
    {
        let _ = pid;
    }
}

// ---------------------------------------------------------------------------
// Process-group ID capture
// ---------------------------------------------------------------------------

/// Return the process-group ID of `pid`, or `None` on error or unsupported platform.
///
/// On Unix this calls `getpgid(pid)`.  This should be captured **before** the
/// escalation ladder begins so it remains valid even if the parent process exits
/// mid-escalation (the PGID is a kernel attribute of the group, not of the
/// individual parent process).
///
/// Returns `None` on Windows and other non-Unix platforms.
pub fn get_process_group_id(pid: u32) -> Option<Pgid> {
    #[cfg(unix)]
    {
        // SAFETY: `getpgid(pid)` is a pure query syscall with no side effects.
        // It returns -1 and sets `errno` on failure (e.g. ESRCH if the process
        // has already exited).  We convert the error into `None`.
        #[allow(clippy::cast_possible_wrap)]
        let raw_pgid = unsafe { libc::getpgid(pid as libc::pid_t) };
        if raw_pgid == -1 { None } else { Some(raw_pgid) }
    }

    #[cfg(not(unix))]
    {
        let _ = pid;
        None
    }
}

/// Forcibly kill a **pre-captured** process group using SIGKILL (Unix).
///
/// Unlike [`kill_process_group_force`] — which derives the PGID from the parent
/// PID and can race if the parent exits before the signal is sent — this helper
/// takes a pgid that was resolved once at escalation entry via
/// [`get_process_group_id`].  The captured value survives the parent's death
/// because the kernel keeps the PGID alive as long as any member process exists.
///
/// On Windows the equivalent "kill the whole tree" operation is
/// `taskkill /F /T /PID <pid>`, which terminates the process and all its
/// children regardless of process-group membership. The `pgid` parameter is
/// unused on Windows; pass the original Firefox PID in `pid_for_windows` to
/// drive `taskkill`. Both Unix and Windows paths target the same conceptual
/// goal: reap every descendant of the original Firefox process.
///
/// Errors are silently ignored — the caller polls the port to verify.
pub fn kill_process_tree(pid_for_windows: u32, pgid: Option<Pgid>) {
    #[cfg(unix)]
    {
        if let Some(pgid_val) = pgid {
            // SAFETY: `kill(-pgid, SIGKILL)` sends SIGKILL to every process in the
            // group identified by `pgid_val`. The pgid was captured before the
            // escalation ladder started and remains valid as long as any group
            // member is alive. SIGKILL cannot be caught or ignored.
            unsafe {
                libc::kill(-pgid_val, libc::SIGKILL);
            }
        }
        // If pgid is None (getpgid failed, meaning the process had already exited),
        // there is nothing left to kill — the group has already dissolved.
        let _ = pid_for_windows;
    }

    #[cfg(windows)]
    {
        // `taskkill /F /T /PID <pid>` forcibly terminates the process and the
        // entire process tree it roots (equivalent to Unix killpg on the group).
        // Errors are silently ignored — the caller polls the port to verify.
        let _ = std::process::Command::new("taskkill")
            .args(["/F", "/T", "/PID", &pid_for_windows.to_string()])
            .output();
        let _ = pgid;
    }

    #[cfg(not(any(unix, windows)))]
    {
        let _ = (pid_for_windows, pgid);
    }
}

/// Send SIGTERM (Unix) or TerminateProcess (Windows) to `pid`.
///
/// Errors are silently ignored — the caller checks PID liveness separately
/// to decide whether the termination succeeded.
///
/// Also used internally by `kill_process_group` and `kill_process_group_force`
/// as the Windows fallback path inside `#[cfg(windows)]` blocks.
#[allow(dead_code)]
pub fn kill_process(pid: u32) {
    #[cfg(unix)]
    {
        // SAFETY: `kill(pid, SIGTERM)` sends a signal to another process.
        // This is a well-defined POSIX operation with no memory-safety implications.
        // The cast from u32 to pid_t is safe for any PID the OS hands us.
        #[allow(clippy::cast_possible_wrap)]
        unsafe {
            libc::kill(pid as libc::pid_t, libc::SIGTERM);
        }
    }

    #[cfg(windows)]
    {
        use windows_sys::Win32::Foundation::CloseHandle;
        use windows_sys::Win32::System::Threading::{
            OpenProcess, PROCESS_TERMINATE, TerminateProcess,
        };

        // SAFETY: Standard Windows API call to open and terminate a process.
        // The handle is closed immediately after use.
        unsafe {
            let handle = OpenProcess(PROCESS_TERMINATE, 0, pid);
            if !handle.is_null() {
                TerminateProcess(handle, 1);
                CloseHandle(handle);
            }
        }
    }

    #[cfg(not(any(unix, windows)))]
    {
        let _ = pid;
    }
}

// ---------------------------------------------------------------------------
// Port liveness
// ---------------------------------------------------------------------------

/// Return `true` if something is accepting TCP connections on `localhost:port`.
///
/// Uses a non-blocking connect with a 100 ms timeout to avoid hanging on
/// firewalled ports.
pub fn is_port_in_use(port: u16) -> bool {
    use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, TcpStream};
    let addr = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, port));
    TcpStream::connect_timeout(&addr, Duration::from_millis(100)).is_ok()
}

/// Poll `localhost:port` every 100 ms until it stops accepting connections,
/// or until `timeout` elapses.
///
/// Returns `true` if the port is free (connection refused) before the deadline,
/// `false` if it is still listening when `timeout` expires.
pub fn wait_for_port_closed(port: u16, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    loop {
        if !is_port_in_use(port) {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

// ---------------------------------------------------------------------------
// Log file helpers
// ---------------------------------------------------------------------------

/// Open the daemon log file for appending.
///
/// On Unix the file is created with mode `0o600` so that log lines (which
/// may contain URLs, cookies, or auth tokens) are not readable by other OS
/// users on multi-user hosts.  On Windows the parent directory's ACL
/// (inherited from `~/.ff-rdp` which is user-only) provides equivalent
/// protection; no additional mode is set.
fn open_log_file(path: &Path) -> Result<File> {
    let mut opts = OpenOptions::new();
    opts.create(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        opts.mode(0o600);
    }
    let file = opts
        .open(path)
        .with_context(|| format!("opening daemon log file {}", path.display()))?;
    // `mode(0o600)` only takes effect on file creation. If the log already
    // existed with broader permissions (e.g. from a daemon built before this
    // change), force-tighten it now so log lines (URLs, cookies, auth tokens)
    // remain unreadable to other OS users.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let perms = std::fs::Permissions::from_mode(0o600);
        // Best-effort: failure here doesn't block daemon start (e.g. on
        // exotic filesystems that reject chmod), but on a normal POSIX
        // setup it should succeed.
        let _ = std::fs::set_permissions(path, perms);
    }
    Ok(file)
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
    // Open the log with create+append so re-spawning the daemon appends rather
    // than truncating.  On Unix we set 0o600 so URLs/tokens in log lines are
    // not readable by other users.  On Windows, ACL inheritance from the
    // parent directory (0o700 equivalent) is sufficient.
    let log_file = open_log_file(&log_path)?;
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

/// Poll `~/.ff-rdp/daemon.<expected_port>.json` every 50 ms until it appears,
/// contains a valid `DaemonInfo` targeting `expected_host`:`expected_port`, or
/// until `timeout` elapses.  The registry is keyed per Firefox port (iter-123
/// Theme B), so only the file for `expected_port` is polled.
///
/// Validating the host and port ensures we connect to the daemon we just
/// spawned, not a leftover entry targeting a different Firefox instance.
///
/// Returns an error if the timeout is exceeded, the registry cannot be read,
/// or the registry contains a mismatched host/port.
pub fn wait_for_registry(
    timeout: Duration,
    expected_host: &str,
    expected_port: u16,
) -> Result<DaemonInfo> {
    let deadline = Instant::now() + timeout;
    loop {
        match registry::read_registry(expected_port) {
            Ok(Some(info)) => {
                anyhow::ensure!(
                    info.firefox_host == expected_host && info.firefox_port == expected_port,
                    "registry targets {}:{} but expected {expected_host}:{expected_port}",
                    info.firefox_host,
                    info.firefox_port,
                );
                return Ok(info);
            }
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
