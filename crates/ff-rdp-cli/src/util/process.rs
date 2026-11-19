// iter-105 Theme D: this module is the concentrated home of the CLI's OS
// process-management FFI (unix `libc::kill`/`getpgid`/`setsid`, Windows
// `OpenProcess`/`TerminateProcess`).  The crate default is
// `unsafe_code = "deny"`; this narrow, file-scoped allowance keeps the audited,
// `// SAFETY:`-documented FFI compiling while the rest of the crate still denies
// unsafe.  Every `unsafe` block below carries its own SAFETY justification.
#![allow(unsafe_code)]

use std::time::{Duration, Instant};

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
/// On Unix this sends signal 0 (no-op) to the process; on Windows it checks
/// that the process has not signalled its exit (see the comment there).  On
/// other platforms it conservatively returns `true`.
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
        use windows_sys::Win32::Foundation::{CloseHandle, STILL_ACTIVE, WAIT_TIMEOUT};
        use windows_sys::Win32::System::Threading::{
            GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
            PROCESS_SYNCHRONIZE, WaitForSingleObject,
        };

        // "Openable" is not "alive": a process that has exited stays openable
        // for as long as anyone holds a handle to it, and Firefox's content
        // processes hold one to the browser. `stop` then saw a terminated
        // browser as still running (iter-299). So ask whether the process
        // object has signalled its exit.
        //
        // SAFETY: `OpenProcess` only returns a handle (or NULL on failure);
        // the handle is closed on every path below.
        let handle = unsafe {
            OpenProcess(
                PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
                0,
                pid,
            )
        };
        if !handle.is_null() {
            // SAFETY: `handle` is a valid process handle opened with
            // SYNCHRONIZE; a zero timeout only polls its signalled state.
            let running = unsafe { WaitForSingleObject(handle, 0) } == WAIT_TIMEOUT;
            // SAFETY: `handle` is the valid handle opened above, not used again.
            unsafe { CloseHandle(handle) };
            return running;
        }
        // SYNCHRONIZE can be refused where a limited query is not (another
        // account's or an elevated process). Fall back to the exit code; a
        // process that really exited with code 259 reads as alive here, the
        // conservative direction for every caller (prune keeps, stop waits), as is
        // a failed query.
        //
        // SAFETY: as above.
        let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
        if handle.is_null() {
            return false;
        }
        let mut code: u32 = 0;
        // SAFETY: `handle` is valid and `code` is a live `u32` out-parameter.
        let ok = unsafe { GetExitCodeProcess(handle, &raw mut code) };
        // SAFETY: `handle` is the valid handle opened above, not used again.
        unsafe { CloseHandle(handle) };
        // A failed exit-code query proves nothing: keep the old openable-means-alive
        // answer rather than declare a running process dead.
        ok == 0 || code == STILL_ACTIVE.cast_unsigned()
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

/// The file name of the executable `pid` is running (`firefox`,
/// `firefox.exe`, …), as reported by the OS — never from a command line,
/// which the process itself controls.
///
/// - macOS: `proc_pidpath`.
/// - Linux/Android: the `/proc/<pid>/exe` link target.
/// - Windows: `QueryFullProcessImageNameW`.
///
/// `None` when the process does not exist, the OS refuses the query, or the
/// platform has no supported source. Callers that gate a signal on this treat
/// `None` as "not Firefox" and refuse.
pub fn process_executable_name(pid: u32) -> Option<String> {
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    {
        let target = libc::c_int::try_from(pid).ok()?;
        let size = usize::try_from(libc::PROC_PIDPATHINFO_MAXSIZE).ok()?;
        let mut buf = vec![0u8; size];
        let len = u32::try_from(buf.len()).ok()?;
        // SAFETY: `proc_pidpath` writes at most `len` bytes into `buf`, a live
        // allocation of exactly that size, and returns the byte count written
        // (or <= 0 on failure). No other side effects.
        let written = unsafe { libc::proc_pidpath(target, buf.as_mut_ptr().cast(), len) };
        let written = usize::try_from(written).ok().filter(|n| *n > 0)?;
        buf.truncate(written);
        let path = String::from_utf8(buf).ok()?;
        executable_file_name(&path)
    }

    #[cfg(any(target_os = "linux", target_os = "android"))]
    {
        let link = std::fs::read_link(format!("/proc/{pid}/exe")).ok()?;
        let path = link.to_string_lossy();
        // A binary replaced on disk (a package upgrade under a running
        // Firefox) reads back as "<path> (deleted)".
        executable_file_name(path.strip_suffix(" (deleted)").unwrap_or(&path))
    }

    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStringExt;
        use windows_sys::Win32::Foundation::CloseHandle;
        use windows_sys::Win32::System::Threading::{
            OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
            QueryFullProcessImageNameW,
        };

        // SAFETY: `OpenProcess` only returns a handle (or NULL); it is closed
        // on every path below.
        let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
        if handle.is_null() {
            return None;
        }
        let mut buf = vec![0u16; 32_768];
        let mut len = u32::try_from(buf.len()).unwrap_or(u32::MAX);
        // SAFETY: `handle` is valid; `buf` is a live buffer of `len` UTF-16
        // units and `len` is updated in place to the count written.
        let ok = unsafe {
            QueryFullProcessImageNameW(handle, PROCESS_NAME_WIN32, buf.as_mut_ptr(), &raw mut len)
        };
        // SAFETY: `handle` is the valid handle opened above, not used again.
        unsafe { CloseHandle(handle) };
        if ok == 0 {
            return None;
        }
        buf.truncate(usize::try_from(len).ok()?);
        let path = std::ffi::OsString::from_wide(&buf);
        executable_file_name(&path.to_string_lossy())
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

/// The parent PID the OS records for `pid` (a Toolhelp32 process snapshot).
///
/// `None` when `pid` is not in the snapshot or the snapshot fails. Windows
/// never re-parents an orphan, so a recorded parent PID may since have been
/// reused; callers pair it with a liveness check on the parent they expect.
#[cfg(windows)]
pub fn parent_pid(pid: u32) -> Option<u32> {
    process_table()?
        .into_iter()
        .find_map(|(entry, parent)| (entry == pid).then_some(parent))
}

/// The PIDs whose recorded parent is `pid` (a Toolhelp32 process snapshot);
/// empty when the snapshot fails.
///
/// The same caveat as [`parent_pid`]: a recorded parent PID may have been
/// reused, so a caller relies on this only while it holds a handle to `pid`
/// (which keeps the number from being reused) — and even then only to wait,
/// never to signal: a process orphaned by an earlier holder of the number is
/// listed too.
#[cfg(windows)]
pub fn child_pids(pid: u32) -> Vec<u32> {
    process_table()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|(entry, parent)| (parent == pid && entry != pid).then_some(entry))
        .collect()
}

/// Every `(pid, parent pid)` in a Toolhelp32 process snapshot.
#[cfg(windows)]
fn process_table() -> Option<Vec<(u32, u32)>> {
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
        TH32CS_SNAPPROCESS,
    };

    // SAFETY: `CreateToolhelp32Snapshot` only returns a snapshot handle (or
    // INVALID_HANDLE_VALUE); it is closed below on every path that got one.
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if snapshot == INVALID_HANDLE_VALUE {
        return None;
    }
    // SAFETY: PROCESSENTRY32W is plain old data, for which all-zero is valid.
    let mut entry: PROCESSENTRY32W = unsafe { std::mem::zeroed() };
    let table = u32::try_from(std::mem::size_of::<PROCESSENTRY32W>())
        .ok()
        .map(|size| {
            entry.dwSize = size;
            let mut table = Vec::new();
            // SAFETY: `snapshot` is a valid snapshot handle and `entry` is a
            // live PROCESSENTRY32W whose `dwSize` is its own size, as required.
            let mut ok = unsafe { Process32FirstW(snapshot, &raw mut entry) };
            while ok != 0 {
                table.push((entry.th32ProcessID, entry.th32ParentProcessID));
                // SAFETY: as above.
                ok = unsafe { Process32NextW(snapshot, &raw mut entry) };
            }
            table
        });
    // SAFETY: `snapshot` is the valid handle opened above, not used again.
    unsafe { CloseHandle(snapshot) };
    table
}

/// Stop every process ff-rdp spawns from inheriting ff-rdp's own standard
/// handles (iter-299).
///
/// `std::process::Command` on Windows creates the child with handle
/// inheritance on, so a child receives every *inheritable* handle in this
/// process — including the stdout pipe of a caller running
/// `out=$(ff-rdp launch …)`, not just the null/pipe handles the command set up.
/// Firefox's launcher process keeps such a handle for the browser's whole
/// lifetime, so the caller's capture never saw EOF and `launch` never
/// returned. Clearing `HANDLE_FLAG_INHERIT` on the three standard handles
/// removes them from what a child can inherit. A later child that asks for
/// `Stdio::inherit()` is unaffected: std duplicates an inheritable copy for it.
///
/// Best effort: a handle that is absent, invalid or a console pseudo-handle
/// that refuses the call is left as it was.
#[cfg(windows)]
pub fn disinherit_std_handles() {
    use windows_sys::Win32::Foundation::{
        HANDLE_FLAG_INHERIT, INVALID_HANDLE_VALUE, SetHandleInformation,
    };
    use windows_sys::Win32::System::Console::{
        GetStdHandle, STD_ERROR_HANDLE, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE,
    };

    for which in [STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, STD_ERROR_HANDLE] {
        // SAFETY: `GetStdHandle` only reads this process's standard handle
        // slot; it returns NULL or INVALID_HANDLE_VALUE when there is none.
        let handle = unsafe { GetStdHandle(which) };
        if handle.is_null() || handle == INVALID_HANDLE_VALUE {
            continue;
        }
        // SAFETY: `handle` is this process's own standard handle, which
        // outlives the call; changing only its inherit flag cannot invalidate
        // it, and a failure leaves the handle untouched.
        unsafe { SetHandleInformation(handle, HANDLE_FLAG_INHERIT, 0) };
    }
}

/// The last component of an executable path, split on both separators so a
/// Windows path parses the same on every host.
fn executable_file_name(path: &str) -> Option<String> {
    let name = path.rsplit(['/', '\\']).next()?.trim();
    (!name.is_empty()).then(|| name.to_owned())
}

/// Whether `name` (an executable file name from [`process_executable_name`])
/// is a Firefox binary: `firefox`, `firefox-bin` or `firefox-esr`, with an
/// optional `.exe`, compared case-insensitively.
///
/// Deliberately a closed list rather than a `firefox*` prefix match: the
/// point of the check is that `stop` cannot be steered into signalling some
/// other program, and a prefix would admit any binary someone names
/// `firefox-anything`.
pub fn is_firefox_executable_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    let stem = lower.strip_suffix(".exe").unwrap_or(&lower);
    matches!(stem, "firefox" | "firefox-bin" | "firefox-esr")
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

/// Return `true` when `pid` is the leader of its own process group, i.e.
/// `getpgid(pid) == pid`.
///
/// The precondition for every group signal in this module: `launch` puts
/// Firefox in its own group (`process_group(0)`), but a group signal sent on
/// the *assumption* that it did would, for a process sharing the caller's
/// group, reach the caller's whole shell job. Unix only: elsewhere there are
/// no process groups to signal.
#[cfg(unix)]
fn is_process_group_leader(pid: u32) -> bool {
    get_process_group_id(pid).is_some_and(|pgid| i64::from(pgid) == i64::from(pid))
}

/// Send `signal` to `pid`'s process group when `pid` leads its own group,
/// otherwise to `pid` alone. Never signals a group whose id differs from
/// `pid`, and never PID 0 or 1.
#[cfg(unix)]
fn signal_pid_or_own_group(pid: u32, signal: libc::c_int) {
    let Ok(target) = libc::pid_t::try_from(pid) else {
        return;
    };
    if target <= 1 {
        return;
    }
    let target = if is_process_group_leader(pid) {
        -target
    } else {
        target
    };
    // SAFETY: `kill` only delivers a signal. `target` is either the PID itself
    // or the negated id of the group that PID leads, verified by `getpgid`
    // immediately above. A group id cannot be reused while any member lives.
    // Errors surface only through the return value, which the caller ignores
    // (it polls liveness separately).
    unsafe {
        libc::kill(target, signal);
    }
}

/// SIGTERM `pid` — its whole process group when `pid` is the group leader
/// (Unix), so Firefox's content/GPU children are reached too.
///
/// The group is signalled only after `getpgid(pid) == pid` is confirmed; a
/// process that shares another group (the caller's shell job, typically)
/// gets the signal to its PID alone. On Windows there is no POSIX
/// process-group concept; the PID is terminated (same as `kill_process`).
///
/// Errors are silently ignored — the caller checks PID liveness separately.
pub fn kill_process_group(pid: u32) {
    #[cfg(unix)]
    {
        signal_pid_or_own_group(pid, libc::SIGTERM);
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

/// SIGKILL `pid` — its process group when `pid` leads it (Unix), with the
/// same `getpgid(pid) == pid` precondition as [`kill_process_group`].
///
/// Used as a last resort after the SIGTERM grace period expires.
/// On Windows, falls back to `kill_process`.
pub fn kill_process_group_force(pid: u32) {
    #[cfg(unix)]
    {
        signal_pid_or_own_group(pid, libc::SIGKILL);
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
/// Unlike [`kill_process_group_force`] — which checks leadership at signal
/// time and so cannot reach a group whose leader already exited — this helper
/// takes a pgid that was resolved once at escalation entry via
/// [`get_process_group_id`].  The captured value survives the parent's death
/// because the kernel keeps the PGID alive as long as any member process exists.
///
/// On Unix the group is signalled only when `pgid == pid_for_windows`; a
/// captured group id that differs from the target PID is never signalled.
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
        // Only the group the target PID itself led may be signalled: a
        // captured pgid that differs from the PID is some other group — the
        // caller's shell job, typically — and is never a target. `None`
        // (getpgid failed, or leadership was never verified) sends nothing.
        if let Some(pgid_val) = pgid
            && pgid_val > 1
            && i64::from(pgid_val) == i64::from(pid_for_windows)
        {
            // SAFETY: `kill(-pgid, SIGKILL)` sends SIGKILL to every process in the
            // group identified by `pgid_val`, which equals the target PID: the
            // group that PID led when the caller captured it. A group id cannot
            // be reused while any member is alive. SIGKILL cannot be caught.
            unsafe {
                libc::kill(-pgid_val, libc::SIGKILL);
            }
        }
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

/// TerminateProcess on `pid` — the Windows fallback of `kill_process_group`
/// and `kill_process_group_force`, which have no process groups to signal.
///
/// Errors are silently ignored — the caller checks PID liveness separately
/// to decide whether the termination succeeded.
#[cfg(windows)]
fn kill_process(pid: u32) {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_TERMINATE, TerminateProcess};

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

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

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

    /// The executable-name gate `stop` applies before any signal: a closed
    /// list of Firefox binary names, case-insensitive, optional `.exe`.
    #[test]
    fn unit_stop_firefox_executable_names() {
        for yes in [
            "firefox",
            "firefox-bin",
            "firefox-esr",
            "firefox.exe",
            "FIREFOX.EXE",
            "Firefox",
        ] {
            assert!(is_firefox_executable_name(yes), "{yes} is a Firefox binary");
        }
        for no in [
            "",
            "sleep",
            "firefox-evil",
            "notfirefox",
            "firefox.sh",
            "bash",
            "ff-rdp",
            "plugin-container",
        ] {
            assert!(!is_firefox_executable_name(no), "{no} must not pass");
        }
        assert_eq!(
            executable_file_name(r"C:\Program Files\Mozilla Firefox\firefox.exe").as_deref(),
            Some("firefox.exe")
        );
        assert_eq!(
            executable_file_name("/Applications/Firefox.app/Contents/MacOS/firefox").as_deref(),
            Some("firefox")
        );
        assert_eq!(executable_file_name("/"), None);
    }

    /// `process_executable_name` reads this test binary's own name from the
    /// OS — which is not a Firefox name, so the gate refuses it.
    #[test]
    fn unit_stop_process_executable_name_of_self_is_not_firefox() {
        let name = process_executable_name(std::process::id());
        #[cfg(any(target_os = "macos", target_os = "linux", windows))]
        assert!(name.is_some(), "supported platform must name its own exe");
        assert!(!name.as_deref().is_some_and(is_firefox_executable_name));
        assert_eq!(process_executable_name(999_999_999), None);
    }

    /// Rule 5: a process sharing the caller's group is not a group leader, so
    /// a "group" kill reaches the PID alone — the child dies (the old
    /// `kill(-pid)` hit no group and left it running) and this test process,
    /// in the same group, is untouched by construction.
    #[cfg(unix)]
    #[test]
    fn unit_stop_non_leader_is_signalled_by_pid_alone() {
        let mut child = Command::new("sleep").arg("30").spawn().expect("spawn");
        let pid = child.id();
        assert!(
            !is_process_group_leader(pid),
            "a child in our group must not read as a group leader"
        );
        kill_process_group_force(pid);
        let status = child.wait().expect("wait");
        assert!(!status.success(), "the PID-alone SIGKILL must reach it");
    }

    /// Rule 5: a leader of its own group is recognised, and `kill_process_tree`
    /// refuses a captured pgid that is not the target PID's own group.
    #[cfg(unix)]
    #[test]
    fn unit_stop_tree_kill_refuses_a_foreign_pgid() {
        use std::os::unix::process::CommandExt;
        let mut bystander = Command::new("sleep")
            .arg("30")
            .process_group(0)
            .spawn()
            .expect("spawn bystander");
        let mut target = Command::new("sleep")
            .arg("30")
            .process_group(0)
            .spawn()
            .expect("spawn target");
        assert!(is_process_group_leader(target.id()));
        let bystander_group = get_process_group_id(bystander.id());
        assert!(bystander_group.is_some());

        kill_process_tree(target.id(), bystander_group);
        std::thread::sleep(Duration::from_millis(100));
        assert!(
            matches!(bystander.try_wait(), Ok(None)),
            "a pgid other than the target PID must never be signalled"
        );
        assert!(
            matches!(target.try_wait(), Ok(None)),
            "nothing is sent at all when the pgid does not match"
        );

        kill_process_tree(target.id(), get_process_group_id(target.id()));
        assert!(!target.wait().expect("wait target").success());
        let _ = bystander.kill();
        let _ = bystander.wait();
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

    /// iter-299: a process that has exited reads as dead even while a handle
    /// to it is still open — here the `Child`, which keeps its process handle
    /// until dropped, exactly as Firefox's content processes hold the browser's.
    #[cfg(windows)]
    #[test]
    fn unit_299_exited_process_held_open_reads_dead() {
        let mut child = Command::new("ping")
            .args(["-n", "31", "127.0.0.1"])
            .stdout(std::process::Stdio::null())
            .spawn()
            .expect("spawn ping");
        let pid = child.id();
        assert!(is_process_alive(pid), "a running child must read alive");
        child.kill().expect("kill");
        child.wait().expect("wait");
        assert!(
            !is_process_alive(pid),
            "an exited process must read dead while its handle is still open"
        );
        drop(child);
    }

    /// iter-299: the Toolhelp32 parent lookup names the process that spawned
    /// the child, and nothing for a PID that does not exist.
    #[cfg(windows)]
    #[test]
    fn unit_299_parent_pid_of_a_child_is_this_process() {
        let mut child = Command::new("ping")
            .args(["-n", "31", "127.0.0.1"])
            .stdout(std::process::Stdio::null())
            .spawn()
            .expect("spawn ping");
        let parent = parent_pid(child.id());
        let _ = child.kill();
        let _ = child.wait();
        assert_eq!(parent, Some(std::process::id()));
        assert_eq!(parent_pid(999_999_999), None);
    }

    /// iter-304: the children lookup lists a process this one spawned.
    #[cfg(windows)]
    #[test]
    fn unit_304_child_pids_lists_a_spawned_child() {
        let mut child = Command::new("ping")
            .args(["-n", "31", "127.0.0.1"])
            .stdout(std::process::Stdio::null())
            .spawn()
            .expect("spawn ping");
        let children = child_pids(std::process::id());
        let _ = child.kill();
        let _ = child.wait();
        assert!(children.contains(&child.id()), "{children:?}");
        assert!(!children.contains(&std::process::id()), "{children:?}");
    }
}
