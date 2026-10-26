//! Stopping a Firefox ff-rdp launched: the shared path behind
//! `launch --replace` and `ff-rdp stop`.
//!
//! The only ownership proof is the owner-marker pair `launch` writes into the
//! managed profile it creates (iter-110 Theme A0, iter-171): the process must
//! be named by such a marker under a trusted root, its recorded start token
//! must match, and it must be running a Firefox executable — see
//! [`crate::util::profile_dir::authorize_kill`]. Anything else is refused: a
//! Firefox the user started by hand, one launched with a `--profile`
//! directory the user owns (which never receives a marker), or a recycled PID.

use std::time::Duration;

use crate::error::AppError;
use crate::util::process::{self, Pgid};
use crate::util::profile_dir::{self, KillRefusal, OwnedFirefox};

/// Maximum time to wait for a port to become free (or a PID to exit) after
/// stopping Firefox.
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

/// The error for a port held by a process ff-rdp will not signal (rule 7:
/// refusal is an error, and it names the PID and process).
pub(crate) fn refusal_error(
    port: u16,
    process_name: &str,
    pid: u32,
    refusal: &KillRefusal,
    advice: &str,
) -> AppError {
    let who = if process_name.is_empty() {
        format!("PID {pid}")
    } else {
        format!("{process_name} (PID {pid})")
    };
    AppError::User(format!(
        "port {port} is in use by {who}, which ff-rdp did not launch: {}. Refusing to stop a \
         process ff-rdp does not own — {advice}",
        refusal.reason()
    ))
}

/// Stop the ff-rdp-launched Firefox listening on `port`.
///
/// Returns the [`StopOutcome`] if the port is free afterwards, `Err` if it is
/// still in use or its listener is not provably ours. Never prints.
pub(crate) fn stop_prior_instance(port: u16) -> Result<StopOutcome, AppError> {
    let Ok(Some(listener)) = crate::port_owner::find_listener(port) else {
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
    // 6000. Killing it (the 2026-07-09 incident) is never acceptable.
    let owned = profile_dir::authorize_kill(listener.pid).map_err(|refusal| {
        refusal_error(
            port,
            &listener.process_name,
            listener.pid,
            &refusal,
            "stop it yourself, or pass --debug-port to use a different port.",
        )
    })?;
    let report = stop_owned_firefox(&owned, Some(port));
    if !report.released {
        return Err(AppError::User(report.message));
    }
    Ok(StopOutcome {
        stopped: true,
        pid: Some(owned.pid),
    })
}

/// What [`stop_owned_firefox`] achieved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StopReport {
    /// The PID is no longer alive.
    pub(crate) exited: bool,
    /// The resource the stop waited on was released: the port when one was
    /// given, otherwise the PID itself.
    pub(crate) released: bool,
    /// Why the stop failed; empty when `released`.
    pub(crate) message: String,
}

/// Run the SIGTERM → 2 s → SIGKILL ladder against a Firefox whose ownership
/// [`profile_dir::authorize_kill`] has just proved.
///
/// Ownership is re-proved before **every** signal (rule 6): the marker, start
/// token and executable through `authorize_kill`, and — when `port` is given —
/// that no *other* process now holds the port. A port the PID already closed
/// (a Firefox that dropped its listener early in shutdown but is still
/// running) does not block the SIGKILL: the start token is the identity proof,
/// the port only named the candidate. A PID recycled between the check and a
/// signal therefore never receives one.
pub(crate) fn stop_owned_firefox(owned: &OwnedFirefox, port: Option<u16>) -> StopReport {
    let reverify = move |pid: u32| {
        profile_dir::authorize_kill(pid).is_ok()
            && port.is_none_or(|p| port_free_or_held_by(p, pid))
    };
    stop_pid_with_full_escalation(owned.pid, port, &EscalationHooks::real(), &reverify)
}

/// Is `port` either free or held by `pid`? An unidentifiable holder of an
/// in-use port answers `false` (fail closed).
fn port_free_or_held_by(port: u16, pid: u32) -> bool {
    match crate::port_owner::find_listener(port) {
        Ok(Some(current)) => current.pid == pid,
        Ok(None) | Err(_) => !crate::port_owner::is_port_in_use(port),
    }
}

/// The process group of whatever now listens on `port`, if identifiable.
fn listener_pgid(port: u16) -> Option<Pgid> {
    let listener = crate::port_owner::find_listener(port).ok().flatten()?;
    process::get_process_group_id(listener.pid)
}

/// Format the post-escalation "still running" error message.
///
/// `pgid_killed` indicates whether the pgid-level kill step was attempted.
fn still_held_after_escalation_msg(pid: u32, port: Option<u16>, pgid_killed: bool) -> String {
    #[cfg(unix)]
    let escalation_detail = if pgid_killed {
        "after SIGTERM+SIGKILL on pid + SIGKILL on pgid"
    } else {
        "after SIGTERM+SIGKILL escalation (pgid kill skipped)"
    };
    #[cfg(not(unix))]
    let escalation_detail = if pgid_killed {
        "after TerminateProcess + taskkill /T on pid tree"
    } else {
        "after TerminateProcess escalation (tree kill skipped)"
    };
    let secs = PORT_FREE_WAIT_BOUND.as_secs();
    match port {
        Some(port) => format!(
            "stopped Firefox (pid {pid}) but port {port} is still listening after {secs} s \
             ({escalation_detail}, port still listening) — another process may be holding it. \
             Run `ff-rdp doctor` or `lsof -i :{port}` to investigate."
        ),
        None => format!(
            "Firefox (pid {pid}) is still running after {secs} s ({escalation_detail}) — \
             ff-rdp stopped signalling it once its ownership could no longer be confirmed, or \
             it ignored SIGKILL. Check it with `ps -p {pid}`."
        ),
    }
}

/// The injectable operations used by [`stop_pid_with_full_escalation`]: a
/// struct of plain function pointers, real implementations in
/// [`EscalationHooks::real`] and stubs in tests.
struct EscalationHooks {
    /// Returns `true` if the process with `pid` is currently alive.
    is_alive: fn(u32) -> bool,
    /// SIGTERM `pid` — its group only when `pid` leads it.
    kill_group_term: fn(u32),
    /// SIGKILL `pid` — its group only when `pid` leads it.
    kill_group_kill: fn(u32),
    /// Kill the explicitly captured process group `pgid` (refused unless it
    /// equals `pid`). Also receives the original `pid` for the Windows
    /// `taskkill` path.
    kill_process_tree: fn(u32, Option<Pgid>),
    /// Capture the PGID of `pid` (before escalation starts).
    get_pgid: fn(u32) -> Option<Pgid>,
    /// Poll `port` until closed or `timeout` elapses; returns `true` if closed.
    wait_port_closed: fn(u16, Duration) -> bool,
    /// The process group of the port's current listener (step-4 re-check).
    #[cfg_attr(not(unix), allow(dead_code))]
    listener_pgid: fn(u16) -> Option<Pgid>,
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
            listener_pgid,
        }
    }
}

/// Poll `is_alive(pid)` until it reports dead or `timeout` elapses.
pub(crate) fn wait_for_exit(is_alive: fn(u32) -> bool, pid: u32, timeout: Duration) -> bool {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        if !is_alive(pid) {
            return true;
        }
        if std::time::Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// The stop-escalation ladder (iter-158 Theme C): SIGTERM → grace → SIGKILL →
/// wait for release → tree-kill the captured group → wait.
///
/// "Release" is the port closing when `port` is `Some`, the PID exiting when
/// it is `None` (`stop --all`, which has no port to watch).
///
/// The pgid is captured **before any signal is sent** (by then a dead parent's
/// `getpgid` fails) and kept only when it equals `pid` — Firefox leads its own
/// group. A pgid that differs from the PID, or one that could not be read, is
/// never signalled (rule 5); the step is skipped instead. Every signal to the
/// PID itself is preceded by `reverify` (rule 6).
///
/// Returns a [`StopReport`]; `message` is non-empty only when not released.
fn stop_pid_with_full_escalation(
    pid: u32,
    port: Option<u16>,
    h: &EscalationHooks,
    reverify: &dyn Fn(u32) -> bool,
) -> StopReport {
    let captured_pgid = (h.get_pgid)(pid).filter(|group| i64::from(*group) == i64::from(pid));
    let released = |timeout: Duration| match port {
        Some(port) => (h.wait_port_closed)(port, timeout),
        None => wait_for_exit(h.is_alive, pid, timeout),
    };

    // Step 1: SIGTERM, then a bounded grace period.
    if (h.is_alive)(pid) && reverify(pid) {
        (h.kill_group_term)(pid);
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while (h.is_alive)(pid) && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    // Step 2: SIGKILL.
    if (h.is_alive)(pid) && reverify(pid) {
        (h.kill_group_kill)(pid);
        std::thread::sleep(Duration::from_millis(300));
    }

    // Step 3: wait for the OS to reclaim the socket (or the PID).
    if released(PORT_FREE_WAIT_BOUND) {
        return StopReport {
            exited: !(h.is_alive)(pid),
            released: true,
            message: String::new(),
        };
    }

    // Step 4: reach the children that outlived the parent — re-verified like
    // every other signal (rule 6). On Unix the target is the group the PID
    // led when it was captured, and it is signalled only while the PID is
    // still alive and proved ours, or while the port's current listener is a
    // member of that very group (a group id cannot be reused while any member
    // lives, so that listener is one of Firefox's children). On Windows it is
    // `taskkill /T /PID`, which is PID-scoped, so it fires only while the PID
    // is alive and proved ours: a dead PID's number may belong to someone else.
    #[cfg(unix)]
    let tree_kill_allowed = captured_pgid.is_some()
        && (((h.is_alive)(pid) && reverify(pid))
            || port.is_some_and(|p| (h.listener_pgid)(p) == captured_pgid));
    #[cfg(not(unix))]
    let tree_kill_allowed = (h.is_alive)(pid) && reverify(pid);
    if tree_kill_allowed {
        (h.kill_process_tree)(pid, captured_pgid);
    }
    if released(Duration::from_millis(500)) {
        return StopReport {
            exited: !(h.is_alive)(pid),
            released: true,
            message: String::new(),
        };
    }

    StopReport {
        exited: !(h.is_alive)(pid),
        released: false,
        message: still_held_after_escalation_msg(pid, port, tree_kill_allowed),
    }
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

    fn always_owned(_pid: u32) -> bool {
        true
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
            // A surviving child of the captured group still holds the port.
            listener_pgid: |_port| Some(4242),
        }
    }

    /// Rule 6 on step 4: with the parent dead, the group kill fires only when
    /// the port's current holder is in the captured group. A holder from some
    /// other group (the PID's group is gone and the port changed hands) gets
    /// nothing.
    #[cfg(unix)]
    #[test]
    fn unit_stop_tree_kill_requires_listener_in_captured_group() {
        let _serialized = begin_call_log();
        let hooks = EscalationHooks {
            listener_pgid: |_port| Some(9999),
            ..recording_hooks_dead_parent()
        };
        let _ = stop_pid_with_full_escalation(4242, Some(65005), &hooks, &always_owned);
        let log = take_call_log();
        assert!(
            !log.contains(&"kill_process_tree"),
            "a port holder outside the captured group must not trigger a group kill; {log:?}"
        );
    }

    /// `get_pgid` is called strictly before the first kill hook (iter-158).
    #[cfg(unix)]
    #[test]
    fn unit_158_stop_ladder_captures_pgid_before_any_kill() {
        let _serialized = begin_call_log();
        let hooks = recording_hooks_dead_parent();
        let _ = stop_pid_with_full_escalation(4242, Some(65000), &hooks, &always_owned);

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
    /// reaches `kill_process_tree` on the group the PID led (iter-158).
    #[cfg(unix)]
    #[test]
    fn unit_158_stop_ladder_reaches_tree_kill_when_parent_is_dead() {
        let _serialized = begin_call_log();
        let hooks = recording_hooks_dead_parent();
        let report = stop_pid_with_full_escalation(4242, Some(65001), &hooks, &always_owned);

        let log = take_call_log();
        assert!(
            log.contains(&"kill_process_tree"),
            "the tree kill must run even when the parent is already dead; log was {log:?}"
        );
        assert!(report.exited, "a dead parent counts as stopped");
        assert!(!report.released, "the stub never frees the port");
        assert!(
            report.message.contains("still listening"),
            "a failed stop must carry an escalation message: {}",
            report.message
        );
    }

    /// Rule 5 (replaces iter-158's pid fallback): when `getpgid` fails there
    /// is no verified group, so no group signal is sent at all — previously
    /// the ladder assumed the PID was the group id and signalled `-pid`.
    #[test]
    fn unit_stop_tree_kill_skipped_when_pgid_lookup_fails() {
        let _serialized = begin_call_log();
        let hooks = EscalationHooks {
            get_pgid: |_pid| None,
            ..recording_hooks_dead_parent()
        };
        let report = stop_pid_with_full_escalation(4242, Some(65002), &hooks, &always_owned);
        let log = take_call_log();
        assert!(
            !log.contains(&"kill_process_tree"),
            "no verified group means no group signal; log was {log:?}"
        );
        assert!(report.message.contains("skipped"), "{}", report.message);
    }

    /// Rule 5: a captured pgid that is not the PID's own (the caller's shell
    /// job group, say) is never handed to the tree kill.
    #[test]
    fn unit_stop_tree_kill_skipped_when_pgid_differs_from_pid() {
        let _serialized = begin_call_log();
        let hooks = EscalationHooks {
            get_pgid: |_pid| Some(7),
            ..recording_hooks_dead_parent()
        };
        let _ = stop_pid_with_full_escalation(4242, Some(65004), &hooks, &always_owned);
        let log = take_call_log();
        assert!(
            !log.contains(&"kill_process_tree"),
            "a foreign pgid must never be signalled; log was {log:?}"
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
            listener_pgid: |_port| None,
        };
        let _ = stop_pid_with_full_escalation(4242, Some(65003), &hooks, &|_pid| false);
        let log = take_call_log();
        assert!(
            !log.contains(&"kill_group_term") && !log.contains(&"kill_group_kill"),
            "a recycled PID must never be signalled; log was {log:?}"
        );
    }

    /// Without a port (`stop --all`), release means the PID exited: a PID
    /// that is already dead is released at once and no signal is sent.
    #[test]
    fn unit_stop_portless_ladder_waits_on_the_pid() {
        let _serialized = begin_call_log();
        let report = stop_pid_with_full_escalation(
            4242,
            None,
            &recording_hooks_dead_parent(),
            &always_owned,
        );
        let log = take_call_log();
        assert!(report.released && report.exited);
        assert!(
            !log.contains(&"wait_port_closed"),
            "no port to wait on; log was {log:?}"
        );
        assert!(
            !log.iter().any(|c| c.starts_with("kill_")),
            "a dead PID is never signalled; log was {log:?}"
        );
    }
}
