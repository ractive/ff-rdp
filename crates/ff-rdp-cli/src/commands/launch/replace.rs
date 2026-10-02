//! `launch --replace`: stop the Firefox ff-rdp launched on a port so a fresh
//! one can take it.
//!
//! The only ownership proof is the owner-PID marker `launch` writes into the
//! managed profile it creates (iter-110 Theme A0): the process listening on
//! the port must be named by such a marker, or nothing is signalled. A Firefox
//! the user started by hand — or one launched with a `--profile` directory the
//! user owns, which never receives a marker — is refused.

use std::time::Duration;

use crate::error::AppError;
use crate::util::process::{self, Pgid};

/// Maximum time to wait for a port to become free after stopping Firefox.
///
/// If the port is still in use after this bound, the tree kill runs before
/// declaring failure.
const PORT_FREE_WAIT_BOUND: Duration = Duration::from_secs(8);

/// Outcome of [`stop_prior_instance`], threaded back to the caller instead of
/// being printed. `launch --replace` folds this into its own envelope's
/// `meta.replaced` field so the command emits exactly one top-level JSON
/// document (iter-153) — `pid` is the PID of the instance that was stopped,
/// never confused with `results.pid` (the newly launched instance).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct StopOutcome {
    pub(crate) stopped: bool,
    pub(crate) pid: Option<u32>,
}

/// Stop the ff-rdp-launched Firefox listening on `port`.
///
/// Returns the [`StopOutcome`] if the port is free afterwards, `Err` if it is
/// still in use or its listener is not provably ours. Never prints.
pub(crate) fn stop_prior_instance(port: u16) -> Result<StopOutcome, AppError> {
    let Ok(Some(owner)) = crate::port_owner::find_listener(port) else {
        // Nothing we could identify is listening — the port may already be free.
        if !process::wait_for_port_closed(port, PORT_FREE_WAIT_BOUND) {
            return Err(AppError::User(format!(
                "port {port} is still in use, but its owner could not be identified. \
                 Run `ff-rdp doctor` or `lsof -i :{port}` to investigate."
            )));
        }
        return Ok(StopOutcome {
            stopped: true,
            pid: None,
        });
    };
    // iter-110 Theme A0: never signal a process we did not spawn. The
    // port-owner lookup finds whatever is *listening on the RDP port* — which
    // may be a Firefox the user launched by hand on ff-rdp's default port
    // 6000. Killing it (the 2026-07-09 incident) is never acceptable. Fails
    // closed: no marker ⇒ no kill (see `pid_is_ff_rdp_spawned`).
    if !crate::util::profile_dir::pid_is_ff_rdp_spawned(owner.pid) {
        return Err(AppError::User(format!(
            "port {port} is in use by {} (PID {}), which ff-rdp did not launch \
             (no owner-PID marker). Refusing to stop a process ff-rdp does not own — \
             stop it yourself, or pass --debug-port to use a different port.",
            owner.process_name, owner.pid
        )));
    }
    // iter-100 Theme D: re-verify port ownership immediately before each
    // signal. `find_listener` resolved the PID at time T; between T and the
    // signal the process may have exited and the OS may have recycled the PID
    // onto an unrelated process.
    let (_, port_free, escalation_msg) = stop_pid_with_full_escalation(
        owner.pid,
        port,
        &EscalationHooks::real(),
        Some(port_still_owned_by),
    );
    if !port_free {
        return Err(AppError::User(escalation_msg));
    }
    Ok(StopOutcome {
        stopped: true,
        pid: Some(owner.pid),
    })
}

/// [`OwnershipCheck`] for the port-owner stop path: does `pid` still own
/// `port`?
fn port_still_owned_by(port: u16, pid: u32) -> bool {
    matches!(
        crate::port_owner::find_listener(port),
        Ok(Some(ref current)) if current.pid == pid
    )
}

/// Format the post-escalation "port still listening" error message.
///
/// `pgid_killed` indicates whether the pgid-level kill step was attempted.
fn port_still_listening_after_escalation_msg(pid: u32, port: u16, pgid_killed: bool) -> String {
    #[cfg(unix)]
    let escalation_detail = if pgid_killed {
        "after SIGTERM+SIGKILL on pid + SIGKILL on pgid, port still listening"
    } else {
        "after SIGTERM+SIGKILL escalation (pgid kill skipped), port still listening"
    };
    #[cfg(not(unix))]
    let escalation_detail = if pgid_killed {
        "after TerminateProcess + taskkill /T on pid tree, port still listening"
    } else {
        "after TerminateProcess escalation (tree kill skipped), port still listening"
    };
    format!(
        "stopped Firefox (pid {pid}) but port {port} is still listening after {} s \
         ({escalation_detail}) — \
         another process may be holding it. Run `ff-rdp doctor` or \
         `lsof -i :{port}` to investigate.",
        PORT_FREE_WAIT_BOUND.as_secs()
    )
}

/// The injectable operations used by [`stop_pid_with_full_escalation`]: a
/// struct of plain function pointers, real implementations in
/// [`EscalationHooks::real`] and stubs in tests.
struct EscalationHooks {
    /// Returns `true` if the process with `pid` is currently alive.
    is_alive: fn(u32) -> bool,
    /// Send SIGTERM to the process group of `pid`.
    kill_group_term: fn(u32),
    /// Send SIGKILL to the process group of `pid` (pid==pgid assumed).
    kill_group_kill: fn(u32),
    /// Kill the explicitly captured process group `pgid`. Also receives the
    /// original `pid` for the Windows `taskkill` path.
    kill_process_tree: fn(u32, Option<Pgid>),
    /// Capture the PGID of `pid` (before escalation starts).
    get_pgid: fn(u32) -> Option<Pgid>,
    /// Poll `port` until closed or `timeout` elapses; returns `true` if closed.
    wait_port_closed: fn(u16, Duration) -> bool,
}

impl EscalationHooks {
    fn real() -> Self {
        Self {
            is_alive: process::is_process_alive,
            kill_group_term: process::kill_process_group,
            kill_group_kill: process::kill_process_group_force,
            kill_process_tree: process::kill_process_tree,
            get_pgid: process::get_process_group_id,
            wait_port_closed: process::wait_for_port_closed,
        }
    }
}

/// An ownership re-verification performed *between* signals: given
/// `(port, pid)`, returns `true` when `pid` still owns `port`.
type OwnershipCheck = fn(u16, u32) -> bool;

/// The stop-escalation ladder (iter-158 Theme C): SIGTERM group → grace →
/// SIGKILL group → wait for the port → tree-kill the captured pgid → wait.
///
/// The pgid is captured **before any signal is sent** (by then a dead parent's
/// `getpgid` fails), and there is no `is_alive` gate on the tree kill: a dead
/// parent whose children still hold the port is precisely the case it exists
/// for.
///
/// Returns `(stopped, port_free, escalation_msg)`. `escalation_msg` is
/// non-empty only when `port_free` is false.
fn stop_pid_with_full_escalation(
    pid: u32,
    port: u16,
    h: &EscalationHooks,
    reverify: Option<OwnershipCheck>,
) -> (bool, bool, String) {
    let captured_pgid = (h.get_pgid)(pid);
    let still_owns = || reverify.is_none_or(|check| check(port, pid));

    // Step 1: SIGTERM the process group, then a bounded grace period.
    if (h.is_alive)(pid) && still_owns() {
        (h.kill_group_term)(pid);
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while (h.is_alive)(pid) && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    // Step 2: SIGKILL the process group (`launch` puts Firefox in its own
    // group, so pid == pgid).
    if (h.is_alive)(pid) && still_owns() {
        (h.kill_group_kill)(pid);
        std::thread::sleep(Duration::from_millis(300));
    }

    // Step 3: wait for the OS to reclaim the socket.
    if (h.wait_port_closed)(port, PORT_FREE_WAIT_BOUND) {
        return (!(h.is_alive)(pid), true, String::new());
    }

    // Step 4: reach the children that outlived the parent via the
    // pre-captured PGID (`taskkill /F /T /PID <pid>` on Windows). `getpgid`
    // on an already-dead parent returns `None`; on Unix fall back to `pid`,
    // because `launch` makes Firefox its own group leader and a group
    // outlives its leader while any member is alive.
    #[cfg(unix)]
    let effective_pgid = captured_pgid.or_else(|| Pgid::try_from(pid).ok());
    #[cfg(not(unix))]
    let effective_pgid = captured_pgid;

    // Only fire the group kill when the group IS the target pid's own: a
    // Firefox not spawned into its own group would otherwise take the
    // caller's shell group with it.
    let pgid_safe_to_kill = match effective_pgid {
        Some(group_id) => i64::from(group_id) == i64::from(pid),
        None => true, // Windows path is pid-scoped, no group risk.
    };
    if pgid_safe_to_kill {
        (h.kill_process_tree)(pid, effective_pgid);
    }
    if (h.wait_port_closed)(port, Duration::from_millis(500)) {
        return (!(h.is_alive)(pid), true, String::new());
    }

    (
        !(h.is_alive)(pid),
        false,
        port_still_listening_after_escalation_msg(pid, port, pgid_safe_to_kill),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A recording [`EscalationHooks`] stub appends each hook's name to a
    /// shared log so a test can assert on the exact call ORDER. `fn` pointers
    /// cannot capture, so the log lives in a `static`.
    static CALL_LOG: std::sync::Mutex<Vec<&'static str>> = std::sync::Mutex::new(Vec::new());

    /// Serializes the tests that share [`CALL_LOG`].
    static CALL_LOG_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn log_call(name: &'static str) {
        if let Ok(mut v) = CALL_LOG.lock() {
            v.push(name);
        }
    }

    fn begin_call_log() -> std::sync::MutexGuard<'static, ()> {
        let guard = CALL_LOG_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Ok(mut v) = CALL_LOG.lock() {
            v.clear();
        }
        guard
    }

    fn take_call_log() -> Vec<&'static str> {
        CALL_LOG.lock().map(|v| v.clone()).unwrap_or_default()
    }

    /// Hooks where the process is already dead and the port never frees — the
    /// orphaned-children scenario the tree kill exists for.
    fn recording_hooks_dead_parent() -> EscalationHooks {
        EscalationHooks {
            is_alive: |_pid| {
                log_call("is_alive");
                false
            },
            kill_group_term: |_pid| log_call("kill_group_term"),
            kill_group_kill: |_pid| log_call("kill_group_kill"),
            kill_process_tree: |_pid, _pgid| log_call("kill_process_tree"),
            get_pgid: |pid| {
                log_call("get_pgid");
                Pgid::try_from(pid).ok()
            },
            wait_port_closed: |_port, _timeout| {
                log_call("wait_port_closed");
                false
            },
        }
    }

    /// `get_pgid` is called strictly before the first kill hook (iter-158).
    #[test]
    fn unit_158_stop_ladder_captures_pgid_before_any_kill() {
        let _serialized = begin_call_log();
        let hooks = recording_hooks_dead_parent();
        let _ = stop_pid_with_full_escalation(4242, 65000, &hooks, None);

        let log = take_call_log();
        let pgid_at = log
            .iter()
            .position(|c| *c == "get_pgid")
            .expect("get_pgid must be called");
        let first_kill_at = log
            .iter()
            .position(|c| {
                matches!(
                    *c,
                    "kill_group_term" | "kill_group_kill" | "kill_process_tree"
                )
            })
            .expect("at least one kill hook must be called");
        assert!(
            pgid_at < first_kill_at,
            "get_pgid must precede every kill; log was {log:?}"
        );
    }

    /// With the parent dead and the port never closing, the ladder still
    /// reaches `kill_process_tree` (iter-158).
    #[test]
    fn unit_158_stop_ladder_reaches_tree_kill_when_parent_is_dead() {
        let _serialized = begin_call_log();
        let hooks = recording_hooks_dead_parent();
        let (stopped, port_free, msg) = stop_pid_with_full_escalation(4242, 65001, &hooks, None);

        let log = take_call_log();
        assert!(
            log.contains(&"kill_process_tree"),
            "the tree kill must run even when the parent is already dead; log was {log:?}"
        );
        assert!(stopped, "a dead parent counts as stopped");
        assert!(!port_free, "the stub never frees the port");
        assert!(
            msg.contains("still listening"),
            "a failed stop must carry an escalation message: {msg}"
        );
    }

    /// On Unix a dead parent yields `None` from `getpgid`; the ladder falls
    /// back to `pid` as the group id so the tree kill is not a no-op.
    #[cfg(unix)]
    #[test]
    fn unit_158_tree_kill_falls_back_to_pid_when_pgid_lookup_fails() {
        use std::sync::atomic::{AtomicI64, Ordering};
        static SEEN_PGID: AtomicI64 = AtomicI64::new(-1);
        SEEN_PGID.store(-1, Ordering::SeqCst);

        let hooks = EscalationHooks {
            is_alive: |_pid| false,
            kill_group_term: |_pid| {},
            kill_group_kill: |_pid| {},
            kill_process_tree: |_pid, pgid| {
                SEEN_PGID.store(pgid.map_or(-1, i64::from), Ordering::SeqCst);
            },
            get_pgid: |_pid| None,
            wait_port_closed: |_port, _timeout| false,
        };
        let _ = stop_pid_with_full_escalation(4242, 65002, &hooks, None);
        assert_eq!(
            SEEN_PGID.load(Ordering::SeqCst),
            4242,
            "the tree kill must fall back to the pid as the group id"
        );
    }

    /// A failed ownership re-check stops the ladder from signalling anything.
    #[test]
    fn unit_100_lost_ownership_sends_no_signal() {
        let _serialized = begin_call_log();
        let hooks = EscalationHooks {
            is_alive: |_pid| true,
            kill_group_term: |_pid| log_call("kill_group_term"),
            kill_group_kill: |_pid| log_call("kill_group_kill"),
            kill_process_tree: |_pid, _pgid| log_call("kill_process_tree"),
            get_pgid: |pid| Pgid::try_from(pid).ok(),
            wait_port_closed: |_port, _timeout| true,
        };
        let _ = stop_pid_with_full_escalation(4242, 65003, &hooks, Some(|_port, _pid| false));
        let log = take_call_log();
        assert!(
            !log.contains(&"kill_group_term") && !log.contains(&"kill_group_kill"),
            "a recycled PID must never be signalled; log was {log:?}"
        );
    }
}
