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
// Process identity (iter-171)
// ---------------------------------------------------------------------------

/// An opaque, OS-supplied token that identifies *one particular incarnation*
/// of a PID: the process's creation time.
///
/// [`is_process_alive`] answers "does some process hold this PID right now",
/// which is a strictly weaker question than "is the process that wrote this
/// PID down still running". PIDs are recycled — measured on this project's
/// macOS dev machine at ~229 allocations/second under spawn-heavy load
/// against a `PID_MAX` of 99 999, i.e. a full wrap in roughly seven minutes
/// of saturated spawning, well inside one ~40-minute live sweep. Once a PID
/// is reused, every `kill(pid, 0)`-based ownership check in the codebase
/// silently starts answering about the *wrong process*.
///
/// Pairing the PID with this token turns liveness into an identity check: the
/// pair `(pid, start_token)` is unique for the lifetime of a boot, because a
/// recycled PID necessarily has a later creation time. Callers persist the
/// token next to the PID (see `util::profile_dir`'s owner markers) and compare
/// it back before trusting the PID.
///
/// Returns `None` when the process does not exist, when the OS refuses the
/// query (a PID owned by another user), or on a platform with no supported
/// source for the value. `None` is *not* evidence of death — callers must fall
/// back to [`is_process_alive`] alone, which is exactly the pre-iter-171
/// behaviour.
///
/// Per-platform source:
/// - macOS/iOS: `proc_pidinfo(PROC_PIDTBSDINFO)` → `pbi_start_tvsec`/`_tvusec`.
/// - Linux/Android: `/proc/<pid>/stat` field 22 (`starttime`, in clock ticks
///   since boot) — read as text, no FFI.
/// - Windows: `GetProcessTimes` → the creation `FILETIME`.
/// - Anything else: `None`.
pub fn process_start_token(pid: u32) -> Option<String> {
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    {
        let mut info = std::mem::MaybeUninit::<libc::proc_bsdinfo>::zeroed();
        let size = c_int_from(std::mem::size_of::<libc::proc_bsdinfo>())?;
        // SAFETY: `proc_pidinfo` writes at most `size` bytes into the buffer we
        // pass, and `size` is exactly `size_of::<proc_bsdinfo>()`. The pointer
        // comes from a live, correctly-aligned `MaybeUninit<proc_bsdinfo>` that
        // outlives the call. The only side effect is filling that buffer; a
        // non-existent or inaccessible PID is reported through the return
        // value, which we check against the full struct size before reading.
        #[allow(clippy::cast_possible_wrap)]
        let written = unsafe {
            libc::proc_pidinfo(
                pid as libc::c_int,
                libc::PROC_PIDTBSDINFO,
                0,
                info.as_mut_ptr().cast::<libc::c_void>(),
                size,
            )
        };
        if written != size {
            return None;
        }
        // SAFETY: `proc_pidinfo` returned exactly `size_of::<proc_bsdinfo>()`
        // bytes written, so the buffer is fully initialised.
        let info = unsafe { info.assume_init() };
        Some(format!(
            "{}.{:06}",
            info.pbi_start_tvsec, info.pbi_start_tvusec
        ))
    }

    #[cfg(any(target_os = "linux", target_os = "android"))]
    {
        // Field 22 of /proc/<pid>/stat is `starttime`. Fields 1 and 2 are the
        // PID and the comm, and comm is parenthesised and may itself contain
        // spaces and ')' — so split after the LAST ')' rather than tokenising
        // the whole line.
        let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
        let after_comm = &stat[stat.rfind(')')? + 1..];
        // After the comm, field 3 is `state`; `starttime` is field 22, i.e.
        // the 20th whitespace-separated token of this remainder.
        let starttime = after_comm.split_whitespace().nth(19)?;
        (!starttime.is_empty()).then(|| starttime.to_owned())
    }

    #[cfg(windows)]
    {
        use windows_sys::Win32::Foundation::{CloseHandle, FILETIME};
        use windows_sys::Win32::System::Threading::{
            GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
        };

        // SAFETY: `OpenProcess` only returns a handle (or NULL); we close it on
        // every path below.
        let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
        if handle.is_null() {
            return None;
        }
        let mut creation = FILETIME {
            dwLowDateTime: 0,
            dwHighDateTime: 0,
        };
        let mut exit = creation;
        let mut kernel = creation;
        let mut user = creation;
        // SAFETY: `handle` is a valid handle we just opened, and all four
        // out-pointers reference live, initialised `FILETIME` locals that
        // outlive the call.
        let ok = unsafe {
            GetProcessTimes(
                handle,
                &raw mut creation,
                &raw mut exit,
                &raw mut kernel,
                &raw mut user,
            )
        };
        // SAFETY: `handle` is the valid handle opened above and is not used
        // again after this call.
        unsafe { CloseHandle(handle) };
        if ok == 0 {
            return None;
        }
        let ticks = (u64::from(creation.dwHighDateTime) << 32) | u64::from(creation.dwLowDateTime);
        (ticks != 0).then(|| ticks.to_string())
    }

    #[cfg(not(any(
        target_os = "macos",
        target_os = "ios",
        target_os = "linux",
        target_os = "android",
        windows
    )))]
    {
        let _ = pid;
        None
    }
}

/// `usize` → `c_int` without an `as` cast, so an implausibly large struct size
/// yields `None` instead of a silently truncated FFI argument.
#[cfg(any(target_os = "macos", target_os = "ios"))]
fn c_int_from(n: usize) -> Option<libc::c_int> {
    libc::c_int::try_from(n).ok()
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
/// Returns an error if the timeout is exceeded, or if the registry contains a
/// mismatched host/port.
///
/// A *read* failure is **not** terminal (iter-172): see
/// [`wait_for_registry_in`].
pub fn wait_for_registry(
    timeout: Duration,
    expected_host: &str,
    expected_port: u16,
) -> Result<DaemonInfo> {
    wait_for_registry_in(
        &registry::registry_dir()?,
        timeout,
        expected_host,
        expected_port,
    )
}

/// [`wait_for_registry`] against an explicit registry directory (testable).
///
/// **A read failure is retried, not fatal (iter-172).** Through iter-171 an
/// unreadable registry ended the wait on the spot, so a single bad read —
/// most notoriously the zero-byte `daemon.<port>.json` the old writer
/// published while holding its lock — abandoned autostart within ~50 ms and
/// dropped the caller onto a direct connection. The caller then reported
/// "daemon started but did not register within 20s", which was not even true:
/// nothing had waited 20 s.
///
/// The writer fix (see [`registry::acquire_registry_write_lock_in`]) is what
/// removes that file. Retrying here is defence in depth, and it earns its
/// place on its own: this loop is polling a file another process is actively
/// producing, so "cannot read it *yet*" is a normal intermediate state, not a
/// verdict. The only reason it was ever fatal is that `read_registry` folded
/// "no record" and "unreadable record" into different arms.
///
/// The last read error is kept and reported on timeout, so a genuinely
/// corrupt registry still names itself rather than degrading to a bare
/// "timed out".
pub fn wait_for_registry_in(
    dir: &std::path::Path,
    timeout: Duration,
    expected_host: &str,
    expected_port: u16,
) -> Result<DaemonInfo> {
    let deadline = Instant::now() + timeout;
    // Assigned by every non-returning arm of the match below before it is read.
    let mut last_read_error: Option<String>;
    loop {
        match registry::read_registry_in(dir, expected_port) {
            Ok(Some(info)) => {
                anyhow::ensure!(
                    info.firefox_host == expected_host && info.firefox_port == expected_port,
                    "registry targets {}:{} but expected {expected_host}:{expected_port}",
                    info.firefox_host,
                    info.firefox_port,
                );
                return Ok(info);
            }
            Ok(None) => last_read_error = None,
            Err(e) => last_read_error = Some(format!("{e:#}")),
        }
        if Instant::now() >= deadline {
            match last_read_error {
                Some(e) => anyhow::bail!(
                    "timed out after {timeout:?} waiting for daemon to write registry; \
                     the registry stayed unreadable: {e}"
                ),
                None => {
                    anyhow::bail!(
                        "timed out after {timeout:?} waiting for daemon to write registry"
                    )
                }
            }
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

    /// AC (iter-171): `process_start_token` is available on this platform, is
    /// stable for a given process, and is `None` for a PID that does not
    /// exist. Without the first property the whole identity check silently
    /// degrades to the pre-iter-171 bare-liveness behaviour on this target, so
    /// the assertion is deliberately unconditional on the tier-1 platforms.
    #[test]
    fn unit_process_start_token_stable_for_self_and_none_for_dead_pid() {
        let pid = std::process::id();
        let first = process_start_token(pid);

        #[cfg(any(
            target_os = "macos",
            target_os = "ios",
            target_os = "linux",
            target_os = "android",
            windows
        ))]
        assert!(
            first.is_some(),
            "a supported platform must supply a start token for its own PID {pid}"
        );

        assert_eq!(
            first,
            process_start_token(pid),
            "the start token for a live process must not change between calls"
        );
        assert_eq!(
            process_start_token(999_999_999),
            None,
            "a PID that does not exist has no start token"
        );
    }

    /// AC (iter-171): the token actually *distinguishes* processes — two
    /// concurrently-live PIDs must not share a token. This is the property
    /// that makes `(pid, token)` a usable identity: if the token were, say, a
    /// per-boot constant, every recycled PID would still read as its original
    /// owner and the fix would be inert.
    #[test]
    fn unit_process_start_token_differs_between_processes() {
        let Some(mine) = process_start_token(std::process::id()) else {
            // Unsupported platform — `owner_liveness` degrades to bare
            // liveness there by design, so there is nothing to assert.
            return;
        };

        #[cfg(unix)]
        let mut child = Command::new("sleep")
            .arg("30")
            .spawn()
            .expect("spawn `sleep 30`");
        #[cfg(windows)]
        let mut child = Command::new("cmd")
            .args(["/C", "timeout", "/T", "30", "/NOBREAK"])
            .spawn()
            .expect("spawn timeout");

        let child_token = process_start_token(child.id());
        let _ = child.kill();
        let _ = child.wait();

        assert!(
            child_token.is_some(),
            "a live child process must have a start token"
        );
        assert_ne!(
            child_token,
            Some(mine),
            "two concurrently-live processes must not share a start token — \
             otherwise the token cannot distinguish a recycled PID"
        );
    }

    // ── iter-172: an unreadable registry must not end the autostart wait ────

    const WAIT_PORT: u16 = 6000;

    fn wait_sample_info() -> DaemonInfo {
        DaemonInfo {
            pid: std::process::id(),
            proxy_port: 7000,
            firefox_host: "127.0.0.1".to_owned(),
            firefox_port: WAIT_PORT,
            started_at: "2026-08-17T00:00:00Z".to_owned(),
            auth_token: "a".repeat(64),
        }
    }

    /// AC (iter-172): a registry that cannot be parsed is a *transient* state
    /// of a file another process is still producing, so the wait keeps polling
    /// until its deadline instead of giving up on the first bad read.
    ///
    /// **Fails on `main`**, where the `Err` arm returned immediately: the call
    /// came back in single-digit milliseconds while the caller went on to
    /// report "daemon started but did not register within 20s".
    #[test]
    fn unit_172_wait_for_registry_keeps_polling_an_unreadable_record() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("daemon.6000.json"), b"{ truncated").expect("plant");

        let timeout = Duration::from_millis(400);
        let started = Instant::now();
        let err = wait_for_registry_in(dir.path(), timeout, "127.0.0.1", WAIT_PORT)
            .expect_err("an unreadable registry cannot succeed");
        let elapsed = started.elapsed();

        assert!(
            elapsed >= timeout,
            "the wait must use its whole budget, not bail on the first bad read \
             (returned after {elapsed:?} of a {timeout:?} budget)"
        );
        let msg = format!("{err:#}");
        assert!(
            msg.contains("timed out"),
            "the failure must be reported as a timeout: {msg}"
        );
        assert!(
            msg.contains("parsing registry"),
            "the last read error must survive into the timeout message so a \
             genuinely corrupt registry still names itself: {msg}"
        );
    }

    /// The payoff: a record that becomes readable *after* an unreadable read
    /// is picked up, instead of the whole autostart being abandoned — the
    /// shape the old writer produced on every single write (unusable file
    /// first, real record at the `rename`).
    ///
    /// Deliberately plants *unparseable bytes* rather than the zero-byte file
    /// the old writer actually left: an empty record now reads as `Ok(None)`
    /// (see `registry::read_registry_in`), so planting one would exercise the
    /// "not yet registered" arm and never reach the retry this test is about.
    /// The zero-byte case is covered by
    /// `registry::tests::read_zero_byte_registry_is_treated_as_absent` and by
    /// `live_172_zero_byte_registry_does_not_downgrade_to_direct`.
    #[test]
    fn unit_172_wait_for_registry_recovers_after_a_bad_read() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("daemon.6000.json");
        std::fs::write(&path, b"{ truncated").expect("plant unreadable");

        let write_dir = dir.path().to_path_buf();
        let writer = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(250));
            registry::write_registry_in(&write_dir, &wait_sample_info()).expect("write");
        });

        let info =
            wait_for_registry_in(dir.path(), Duration::from_secs(10), "127.0.0.1", WAIT_PORT)
                .expect("the record that landed mid-wait must be picked up");
        writer.join().expect("writer thread");

        assert_eq!(info.proxy_port, 7000);
        assert_eq!(info.firefox_port, WAIT_PORT);
    }

    /// A registry that never appears at all still times out with the plain
    /// message — the "stayed unreadable" clause is only added when there
    /// really was a read error to report.
    #[test]
    fn unit_172_wait_for_registry_absent_record_times_out_plainly() {
        let dir = tempfile::tempdir().expect("tempdir");
        let err = wait_for_registry_in(
            dir.path(),
            Duration::from_millis(120),
            "127.0.0.1",
            WAIT_PORT,
        )
        .expect_err("no record can never succeed");
        let msg = format!("{err:#}");
        assert!(msg.contains("timed out"), "{msg}");
        assert!(
            !msg.contains("stayed unreadable"),
            "an absent record is not an unreadable one: {msg}"
        );
    }
}
