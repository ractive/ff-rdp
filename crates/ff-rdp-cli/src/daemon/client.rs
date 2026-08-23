use std::net::TcpStream;
use std::time::Duration;

use anyhow::{Context, Result};
use serde_json::{Value, json};

use ff_rdp_core::{FramedReader, FramedWriter, RdpTransport};

use super::process::{self, Pgid};
use super::registry::{self, DaemonInfo};
use crate::cli::args::Cli;
use crate::error::AppError;
use crate::output;
use crate::output_pipeline::OutputPipeline;

/// Maximum time to wait for a port to become free after killing a Firefox process.
///
/// If the port is still in use after this bound, the escalation sequence
/// (SIGTERM → 1 s grace → SIGKILL → ~500 ms re-poll) runs before declaring
/// failure.
const PORT_FREE_WAIT_BOUND: Duration = Duration::from_secs(8);

/// Format the "port still listening" error message.
///
/// The message embeds the actual bound from `PORT_FREE_WAIT_BOUND` so that
/// changing the constant keeps error text in sync automatically.
/// The refusal emitted when a launch record names a PID ff-rdp cannot prove
/// it spawned (iter-191).
///
/// Deliberately the same shape — and the same two load-bearing phrases, "did
/// not launch" and "does not own" — as the port-owner branch's iter-110
/// refusal in [`stop_prior_instance_with`]. Both say the same thing ("that
/// process is not mine, so I will not signal it"); only the artefact that
/// failed to prove ownership differs, and `live_110_kill_scoping` asserts on
/// exactly those phrases regardless of which branch produced them.
pub(crate) fn unowned_record_pid_msg(pid: u32, port: u16) -> String {
    format!(
        "port {port} is in use by PID {pid}, which ff-rdp's launch record for this port names \
         — but that record is stale: the PID no longer identifies the process ff-rdp launched \
         (its recorded start token does not match the live process, and no owner-PID marker \
         names it either), so ff-rdp did not launch whatever holds that PID now. Refusing to \
         stop a process ff-rdp does not own — stop it yourself, run `ff-rdp doctor`, or pass \
         --port to use a different port."
    )
}

pub(crate) fn port_still_listening_msg(pid: u32, port: u16) -> String {
    format!(
        "stopped Firefox (pid {pid}) but port {port} is still listening after {} s — \
         another process may be holding it. Run `ff-rdp doctor` or \
         `lsof -i :{port}` to investigate.",
        PORT_FREE_WAIT_BOUND.as_secs()
    )
}

/// Format the post-escalation "port still listening" error message.
///
/// `pgid_killed` indicates whether the pgid-level kill step was attempted.
/// When `true`, the message says "SIGTERM+SIGKILL on pid + SIGKILL on pgid"
/// so a future failure is unambiguous about which escalation path ran.
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

/// The set of injectable operations used by [`run_escalation`].
///
/// Using a struct of function pointers (instead of trait objects) keeps the
/// abstraction minimal and avoids dynamic dispatch overhead. The real
/// implementation plugs in the actual process helpers; tests inject stubs.
pub(crate) struct EscalationHooks {
    /// Returns `true` if the process with `pid` is currently alive.
    pub is_alive: fn(u32) -> bool,
    /// Send SIGTERM to the process group of `pid`.
    pub kill_group_term: fn(u32),
    /// Send SIGKILL to the process group of `pid` (pid==pgid assumed).
    pub kill_group_kill: fn(u32),
    /// Send SIGKILL to the explicitly captured process group `pgid`.
    /// Also receives the original `pid` for the Windows `taskkill` path.
    pub kill_process_tree: fn(u32, Option<Pgid>),
    /// Capture the PGID of `pid` (before escalation starts).
    pub get_pgid: fn(u32) -> Option<Pgid>,
    /// Poll `port` until closed or `timeout` elapses; returns `true` if closed.
    pub wait_port_closed: fn(u16, Duration) -> bool,
}

impl EscalationHooks {
    /// Production hooks that call the real process helpers.
    pub(crate) fn real() -> Self {
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

/// An optional ownership re-verification performed *between* signals.
///
/// Given `(port, pid)`, returns `true` when `pid` still owns `port`. Only the
/// port-owner stop path needs this: it resolves a PID from a live `lsof` query,
/// and between that query and the signal the process may exit and the OS may
/// recycle the PID onto something unrelated (iter-100 Theme D). The
/// `DaemonRecord` paths do not need it — their PID came from a file ff-rdp
/// itself wrote.
pub(crate) type OwnershipCheck = fn(u16, u32) -> bool;

/// The single stop-escalation ladder (iter-158 Theme C).
///
/// Before iter-158 this sequence — SIGTERM group → grace → SIGKILL group →
/// poll → tree-kill the captured pgid → poll — was written out **four** times
/// (`kill_pid_and_wait_port`, the two kills inside
/// `stop_daemon_and_build_result`, and `stop_prior_instance`'s port-owner
/// branch), and the only copy that reached the tree-kill step could never run
/// it: its caller killed the PID *first*, so the `is_alive` guard at the head
/// of the escalation returned immediately. Steps 3–7 — the entire mechanism
/// designed to reach orphaned children still holding the port — were dead code
/// in production, and `"port still listening after 8s"` was the symptom.
///
/// Two things changed:
/// * the pgid is captured **before any signal is sent**, not merely first
///   within the escalation helper (by then the parent was already dead), and
/// * there is no `is_alive` gate on escalation. A dead parent is precisely the
///   case the tree kill exists for.
///
/// `port` is the port this stop must free, or `None` when the target holds no
/// port (the proxy daemon: it connects to Firefox as a client and never binds
/// the debug port). With `None` the ladder stops after the pid-level signals
/// and reports the port free, since there is nothing to wait on.
///
/// Returns `(stopped, port_free, escalation_msg)`. `escalation_msg` is
/// non-empty only when `port_free` is false.
pub(crate) fn stop_pid_with_full_escalation(
    pid: u32,
    port: Option<u16>,
    h: &EscalationHooks,
    reverify: Option<OwnershipCheck>,
) -> (bool, bool, String) {
    // Capture the PGID FIRST — before any signal. This is the whole point of
    // the unification: `kill_pid_and_wait_port` used to SIGTERM the parent and
    // only then call into an escalation helper that captured the pgid, by
    // which time `getpgid` on a dead parent returns ESRCH.
    let captured_pgid = (h.get_pgid)(pid);

    // Ownership still holds? Absent a checker, always yes.
    let still_owns = |target_port: Option<u16>| match (reverify, target_port) {
        (Some(check), Some(p)) => check(p, pid),
        _ => true,
    };

    // Step 1: SIGTERM the process group, then a bounded grace period.
    if (h.is_alive)(pid) && still_owns(port) {
        (h.kill_group_term)(pid);
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while (h.is_alive)(pid) && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    // Step 2: SIGKILL the process group (assumes pid == pgid, which `launch`
    // guarantees via `process_group(0)`).
    if (h.is_alive)(pid) && still_owns(port) {
        (h.kill_group_kill)(pid);
        std::thread::sleep(Duration::from_millis(300));
    }

    let Some(port) = port else {
        // No port to free — the pid-level signals are the whole job.
        return (!(h.is_alive)(pid), true, String::new());
    };

    // Step 3: wait for the OS to reclaim the socket.
    if (h.wait_port_closed)(port, PORT_FREE_WAIT_BOUND) {
        return (!(h.is_alive)(pid), true, String::new());
    }

    // Step 4: the pid-level kills were not sufficient — reach the children
    // that outlived the parent via the pre-captured PGID. On Windows this
    // sends `taskkill /F /T /PID <pid>`.
    //
    // `getpgid` on an already-dead parent returns `None`, and that is exactly
    // the orphaned-children case this step exists for — so on Unix fall back
    // to `pid` as the group id. `launch` puts Firefox into its own group
    // (pgid == pid, see `commands::launch::build_command`'s
    // `process_group(0)`), and a process group outlives its leader as long as
    // any member is alive, so `killpg(pid)` targets exactly that surviving
    // group. Without this fallback the tree kill is a no-op precisely when it
    // is needed (`process::kill_process_tree` ignores a `None` pgid on Unix).
    #[cfg(unix)]
    let effective_pgid = captured_pgid.or_else(|| Pgid::try_from(pid).ok());
    #[cfg(not(unix))]
    let effective_pgid = captured_pgid;

    // Safety guard (unchanged from iter-95): only fire the pgid kill when the
    // pgid is the SAME as the target pid. If Firefox wasn't spawned in its own
    // process group (older `launch` builds, or a user-supplied wrapper), pgid
    // points at whatever group launched ff-rdp — usually the caller's
    // interactive shell. Killing that group would blast back up the chain and
    // is never what the user wants. On Windows `effective_pgid` is `None` and
    // `kill_process_tree` falls through to `taskkill /F /T /PID`, which is
    // already scoped to the pid subtree.
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

// ---------------------------------------------------------------------------
// Connection target resolution
// ---------------------------------------------------------------------------

/// The result of resolving how to connect: either via daemon or directly.
pub(crate) enum ConnectionTarget {
    /// Connect via daemon at this port on localhost.
    Daemon {
        port: u16,
        /// Auth token to present as the very first frame to the daemon.
        auth_token: String,
    },
    /// Connect directly to Firefox.
    ///
    /// `deferred_warning` carries a daemon-startup diagnostic that should be
    /// printed *only if* the direct fallback also fails.  When the direct
    /// connection succeeds the warning is dropped — its message
    /// (`daemon started but registry not found`, etc.) is benign noise on the
    /// happy path and pushed users to read `daemon.log` for nothing.
    Direct { deferred_warning: Option<String> },
}

/// Find a running daemon whose registry entry matches the given Firefox host/port.
///
/// Returns `Some(info)` if the daemon is alive, `None` otherwise.
/// Automatically removes stale registry files when the recorded PID is dead.
///
/// Note: this only checks PID liveness, not TCP connectivity.  A daemon whose
/// Firefox connection has broken will still appear alive until it exits.  The
/// caller handles connection failures via the normal error path.
pub(crate) fn find_running_daemon(
    firefox_host: &str,
    firefox_port: u16,
) -> Result<Option<DaemonInfo>> {
    let Some(info) = registry::read_registry(firefox_port)? else {
        return Ok(None);
    };

    // Wrong Firefox target — not our daemon.
    if info.firefox_host != firefox_host || info.firefox_port != firefox_port {
        return Ok(None);
    }

    // Check PID liveness and clean up stale entries.
    if !process::is_process_alive(info.pid) {
        eprintln!(
            "daemon: cleaning up stale registry (PID {} is dead)",
            info.pid
        );
        registry::remove_registry(firefox_port).ok();
        return Ok(None);
    }

    Ok(Some(info))
}

/// Resolve how to connect: via daemon (if available or startable) or directly.
///
/// If `no_daemon` is true, always returns [`ConnectionTarget::Direct`].
/// Otherwise, tries to find an existing daemon and returns
/// [`ConnectionTarget::Daemon`].  If no daemon is running, one is spawned and
/// we wait for it to write its registry entry.  Falls back to
/// [`ConnectionTarget::Direct`] with a diagnostic message if anything fails.
pub(crate) fn resolve_connection_target(
    firefox_host: &str,
    firefox_port: u16,
    daemon_timeout_secs: u64,
    no_daemon: bool,
) -> ConnectionTarget {
    if no_daemon {
        return ConnectionTarget::Direct {
            deferred_warning: None,
        };
    }

    // 1. Try to find an already-running daemon (lock-free fast path).
    //    The common case is "daemon already up" — avoid taking the spawn lock
    //    at all so steady-state commands stay contention-free.
    match find_running_daemon(firefox_host, firefox_port) {
        Ok(Some(info)) => {
            return ConnectionTarget::Daemon {
                port: info.proxy_port,
                auth_token: info.auth_token,
            };
        }
        Ok(None) => {} // not running — fall through to the locked spawn path
        Err(e) => {
            return ConnectionTarget::Direct {
                deferred_warning: Some(format!(
                    "warning: failed to check daemon status: {e:#}{}",
                    log_path_hint()
                )),
            };
        }
    }

    // 1a. Fast-fail probe: if Firefox's debug port is unreachable in 100ms,
    //     there is no point spawning a daemon.  Return Direct immediately so
    //     the caller gets the "Firefox isn't running" error faster, without
    //     waiting for the daemon spawn + registry timeout (up to ~5 seconds).
    if !is_firefox_port_open(firefox_host, firefox_port) {
        return ConnectionTarget::Direct {
            deferred_warning: None,
        };
    }

    // 2. Acquire the spawn lock BEFORE the check→spawn→register sequence
    //    (iter-100 Theme D).  Two CLI invocations that both saw "no daemon" in
    //    step 1 would otherwise both spawn one and orphan the loser.  With the
    //    lock, the second invocation blocks here until the first has finished
    //    registering, then re-checks the registry (step 3) and reuses the
    //    winner instead of spawning a duplicate.
    // iter-123 Theme B: the spawn lock is per Firefox port, so an autostart for
    // one port never serializes behind or collides with an autostart for
    // another.
    let _spawn_lock = match registry::acquire_spawn_lock(firefox_port) {
        Ok(lock) => lock,
        Err(e) => {
            // Locking failed (e.g. exotic filesystem) — fall back to a direct
            // connection rather than risk an unserialized double-spawn.
            return direct_with_autostart_warning(format!(
                "could not acquire daemon spawn lock: {e:#} — connecting directly"
            ));
        }
    };

    // 2a. iter-132 Theme E: opportunistically sweep other ports' stale
    // `daemon.*.spawn.lock` files now that we're already on the (rare) spawn
    // path — a normal "daemon already running" invocation never reaches here,
    // so this never costs the steady-state fast path anything. Best-effort;
    // never allowed to affect our own spawn attempt (see the function doc).
    registry::gc_stale_spawn_locks();

    // 3. Re-check under the lock.  A daemon may have been spawned and
    //    registered by a racing invocation between step 1 and acquiring the
    //    lock; if so, reuse it and skip the spawn entirely.
    match find_running_daemon(firefox_host, firefox_port) {
        Ok(Some(info)) => {
            return ConnectionTarget::Daemon {
                port: info.proxy_port,
                auth_token: info.auth_token,
            };
        }
        Ok(None) => {} // still none — we are the elected spawner
        Err(e) => {
            return ConnectionTarget::Direct {
                deferred_warning: Some(format!(
                    "warning: failed to re-check daemon status under lock: {e:#}{}",
                    log_path_hint()
                )),
            };
        }
    }

    // 4. Determine the current executable path so we can re-invoke ourselves
    //    as a daemon.
    let exe_path = match std::env::current_exe() {
        Ok(p) => p,
        Err(e) => {
            return direct_with_autostart_warning(format!(
                "cannot determine executable path: {e} — connecting directly"
            ));
        }
    };

    // 5. Spawn the daemon (still holding the lock).
    if let Err(e) =
        process::spawn_daemon(&exe_path, firefox_host, firefox_port, daemon_timeout_secs)
    {
        return direct_with_autostart_warning(format!(
            "failed to start daemon: {e:#} — connecting directly{}",
            log_path_hint()
        ));
    }

    // 6. Wait for the daemon to write its registry entry (still holding the
    //    lock so no other invocation spawns a competing daemon in the gap).
    //
    //    iter-100 Theme E root-cause instrumentation: a failure here now
    //    distinguishes the three possible causes so the failure mode is
    //    identifiable in the recorded warning rather than a generic message:
    //      * the spawned process is already dead  → "spawn died before the
    //        registry write" (crash on startup: bad port, Firefox refused,
    //        panic before write_registry);
    //      * the process is still alive but no registry file appeared in time
    //        → "registry write raced or was slow".
    //    The Theme D spawn lock above removes the third cause (TOCTOU
    //    double-spawn orphaning the winner) structurally.
    let registry_wait = registry_wait_timeout();
    match process::wait_for_registry(registry_wait, firefox_host, firefox_port) {
        Ok(info) => ConnectionTarget::Daemon {
            port: info.proxy_port,
            auth_token: info.auth_token,
        },
        Err(e) => {
            let cause = classify_registry_wait_failure(firefox_host, firefox_port);
            direct_with_autostart_warning(format!(
                "daemon started but did not register within {}s ({cause}): {e:#} — \
                 connecting directly{}",
                registry_wait.as_secs(),
                log_path_hint()
            ))
        }
    }
}

/// Environment variable overriding how long autostart waits for the freshly
/// spawned daemon to write its registry entry (iter-164).
pub(crate) const REGISTRY_WAIT_ENV: &str = "FF_RDP_DAEMON_START_TIMEOUT_MS";

/// Default registry wait, in milliseconds (iter-164).
///
/// Was a hard-coded 5 s through iter-163. iter-158's `live-sweep` ran at load
/// average 18.6 and a freshly spawned daemon — which has to connect to Firefox,
/// run `listTabs`/`getWatcher`, and install `watchResources` before it writes
/// the registry — did not finish inside 5 s. Autostart then gave up and the
/// caller silently got a *direct* connection instead of the daemon it asked for
/// (iteration-164 defect 2).
///
/// Waiting longer costs nothing on the failure path that actually matters:
/// `resolve_connection_target` already fast-fails in 100 ms when Firefox's
/// debug port is unreachable, so this budget is only ever spent when Firefox is
/// up and a daemon really is starting.
const DEFAULT_REGISTRY_WAIT_MS: u64 = 20_000;

/// How long to wait for a spawned daemon's registry entry (iter-164).
///
/// [`DEFAULT_REGISTRY_WAIT_MS`] unless [`REGISTRY_WAIT_ENV`] holds a positive
/// integer number of milliseconds. A malformed or zero value is ignored in
/// favour of the default rather than producing a zero-length wait.
fn registry_wait_timeout() -> Duration {
    parse_registry_wait(std::env::var(REGISTRY_WAIT_ENV).ok().as_deref())
}

/// Pure half of [`registry_wait_timeout`], split out so it is unit-testable
/// without mutating process-global environment state.
fn parse_registry_wait(raw: Option<&str>) -> Duration {
    let ms = raw
        .and_then(|v| v.trim().parse::<u64>().ok())
        .filter(|ms| *ms > 0)
        .unwrap_or(DEFAULT_REGISTRY_WAIT_MS);
    Duration::from_millis(ms)
}

/// Build a [`ConnectionTarget::Direct`] whose fallback is *also* recorded as a
/// `daemon_autostart_failed` envelope warning (iter-100 Theme E).
///
/// The `deferred_warning` (printed to stderr only if the direct fallback also
/// fails) is kept for the human-facing failure path, while the recorded
/// warning always surfaces in the JSON envelope so scripts/tests can tell
/// daemon mode from a silent direct fallback even when the command succeeds.
fn direct_with_autostart_warning(reason: String) -> ConnectionTarget {
    let deferred = format!("warning: {reason}");
    // Consume `reason` into the recorder (it wants an owned String).
    crate::daemon_status::record_autostart_failed(reason);
    ConnectionTarget::Direct {
        deferred_warning: Some(deferred),
    }
}

/// Classify why the just-spawned daemon failed to register in time
/// (iter-100 Theme E).
///
/// Reads the freshly-written registry (if any) to recover the daemon PID and
/// checks whether that process is still alive.  Returns a short phrase naming
/// the most likely cause so the recorded warning is diagnosable:
///   * no registry + our probe target unreachable → the spawn likely died on
///     startup before writing;
///   * a registry exists but its PID is dead → the daemon crashed after (or
///     during) the write;
///   * otherwise → the registry write raced or was slow.
fn classify_registry_wait_failure(firefox_host: &str, firefox_port: u16) -> &'static str {
    match registry::read_registry(firefox_port) {
        // A registry entry for OUR target exists AND its PID is still alive —
        // the process registered, we just polled before/around the write.
        Ok(Some(info))
            if info.firefox_host == firefox_host
                && info.firefox_port == firefox_port
                && process::is_process_alive(info.pid) =>
        {
            "registry write raced or was slow"
        }
        // Any other case — no matching entry, or a matching entry whose PID is
        // already dead — means the spawn never got far enough to leave a live
        // registered daemon, which almost always means it died during startup.
        _ => "spawn died before the registry write",
    }
}

// ---------------------------------------------------------------------------
// Daemon virtual-actor messages
// ---------------------------------------------------------------------------

/// Send a `drain` request to the daemon for `resource_type` and return the
/// buffered events array.
///
/// The daemon responds with:
/// ```json
/// {"from": "daemon", "events": [...]}
/// ```
/// An empty array is returned when the daemon has no buffered events.
/// Drain daemon events with an optional navigation scope.
///
/// `since_nav_index`:
///  - `0`  → full buffer (no boundary filter)
///  - `-1` → since most-recent navigation
///  - `-2` → since second-to-last, etc.
///
/// Returns `(events, nav_boundary)` where `nav_boundary` is `Some` when the
/// daemon applied a boundary filter and includes `{sequence, url}`.
pub(crate) fn drain_daemon_events_since(
    transport: &mut RdpTransport,
    resource_type: &str,
    since_nav_index: i64,
) -> Result<(Vec<Value>, Option<Value>)> {
    let msg = json!({
        "to": "daemon",
        "type": "drain",
        "resourceType": resource_type,
        "sinceNavIndex": since_nav_index,
    });
    transport
        .send(&msg)
        .context("sending drain request to daemon")?;

    // Read messages until we receive the daemon's drain response.
    // In daemon mode, forwarded Firefox messages (e.g. consoleAPICall push
    // events) may arrive before the daemon's own response; skip them.
    for _ in 0..64 {
        let response = transport
            .recv()
            .context("receiving drain response from daemon")?;
        if response.get("from").and_then(Value::as_str) == Some("daemon") {
            if let Some(err) = response.get("error").and_then(Value::as_str) {
                anyhow::bail!("daemon drain error: {err}");
            }
            let events = response
                .get("events")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            let boundary = response.get("nav_boundary").cloned();
            return Ok((events, boundary));
        }
        // Not a daemon message — discard (forwarded Firefox event).
    }
    anyhow::bail!("did not receive daemon drain response within 64 frames")
}

/// Tell the daemon to start streaming events for `resource_type` directly
/// to this CLI client.  Clears any buffered events for that type so only
/// new events are received.
pub(crate) fn start_daemon_stream(transport: &mut RdpTransport, resource_type: &str) -> Result<()> {
    let msg = json!({
        "to": "daemon",
        "type": "stream",
        "resourceType": resource_type,
    });
    transport
        .send(&msg)
        .context("sending stream request to daemon")?;
    recv_daemon_ack(transport, "stream").map(|_leftovers| ())
}

/// Tell the daemon to stop streaming events for `resource_type` and revert
/// to buffering.
pub(crate) fn stop_daemon_stream(transport: &mut RdpTransport, resource_type: &str) -> Result<()> {
    let msg = json!({
        "to": "daemon",
        "type": "stop-stream",
        "resourceType": resource_type,
    });
    transport
        .send(&msg)
        .context("sending stop-stream request to daemon")?;
    recv_daemon_ack(transport, "stop-stream").map(|_leftovers| ())
}

/// Tell the daemon to stop streaming events for `resource_type` and return
/// any watcher frames that arrived in-flight between the CLI's read timeout
/// and the daemon's `stop-stream` acknowledgement.
///
/// When `drain_network_events` returns due to its idle timeout, the daemon may
/// still have watcher events in-flight that it is forwarding to the CLI client.
/// These frames arrive in the TCP receive buffer between the moment we stop
/// reading and the moment we send `stop-stream`.  The normal `recv_daemon_ack`
/// implementation discards them; this variant collects them so the caller can
/// merge them into the drain result.
pub(crate) fn stop_daemon_stream_draining(
    transport: &mut RdpTransport,
    resource_type: &str,
) -> Result<Vec<Value>> {
    let msg = json!({
        "to": "daemon",
        "type": "stop-stream",
        "resourceType": resource_type,
    });
    transport
        .send(&msg)
        .context("sending stop-stream request to daemon")?;
    recv_daemon_ack(transport, "stop-stream")
}

/// Read frames until we receive a daemon ack (`{from: "daemon", ...}`).
///
/// Returns any non-daemon frames collected while waiting for the ack.  These
/// are watcher events that the daemon's Firefox-reader thread forwarded between
/// the moment the CLI sent a daemon-local request and the moment the daemon
/// processed it.  Callers that need to collect those in-flight events should
/// use the returned `Vec`; callers that don't care can discard it.
fn recv_daemon_ack(transport: &mut RdpTransport, context: &str) -> Result<Vec<Value>> {
    let mut leftovers: Vec<Value> = Vec::new();
    // Limit iterations to avoid spinning forever on a broken connection.
    for _ in 0..64 {
        let response = transport
            .recv()
            .with_context(|| format!("receiving {context} response from daemon"))?;
        if response.get("from").and_then(Value::as_str) == Some("daemon") {
            if let Some(err) = response.get("error").and_then(Value::as_str) {
                anyhow::bail!("daemon {context} error: {err}");
            }
            return Ok(leftovers);
        }
        // Not a daemon message — collect instead of discarding so callers can
        // process in-flight watcher events that arrived before the ack.
        leftovers.push(response);
    }
    anyhow::bail!("did not receive daemon ack for {context} within 64 frames")
}

// ---------------------------------------------------------------------------
// Ref-ID management (iter-60 Part C)
// ---------------------------------------------------------------------------

/// Ask the daemon to allocate `count` consecutive ref IDs.
///
/// Returns `(start, nav_generation)` — the caller must pass `nav_generation`
/// back in the subsequent `register_refs` call so the daemon can detect
/// stale registrations when a navigation races with the JS evaluation.
pub(crate) fn alloc_refs(transport: &mut RdpTransport, count: u64) -> Result<(u64, u64)> {
    let msg = json!({
        "to": "daemon",
        "type": "alloc-refs",
        "count": count,
    });
    transport
        .send(&msg)
        .context("sending alloc-refs to daemon")?;

    for _ in 0..64 {
        let resp = transport.recv().context("receiving alloc-refs response")?;
        if resp.get("from").and_then(Value::as_str) == Some("daemon") {
            if let Some(err) = resp.get("error").and_then(Value::as_str) {
                anyhow::bail!("daemon alloc-refs error: {err}");
            }
            let start = resp
                .get("start")
                .and_then(Value::as_u64)
                .context("alloc-refs response missing 'start'")?;
            let nav_gen = resp
                .get("nav_generation")
                .and_then(Value::as_u64)
                .context("alloc-refs response missing 'nav_generation'")?;
            return Ok((start, nav_gen));
        }
    }
    anyhow::bail!("did not receive alloc-refs response within 64 frames")
}

/// A `(ref_id, resolver_expression)` pair to register with the daemon.
pub(crate) struct RefEntry {
    pub id: String,
    pub resolver: String,
}

/// Register ref IDs with the daemon after an ARIA-tree evaluation.
///
/// `nav_generation` must be the value returned by the preceding `alloc_refs`
/// call.  If the page navigated between alloc and register, the daemon will
/// return a stale error — callers should surface a clear message to the user.
pub(crate) fn register_refs(
    transport: &mut RdpTransport,
    nav_generation: u64,
    entries: &[RefEntry],
) -> Result<()> {
    let refs_json: Vec<Value> = entries
        .iter()
        .map(|e| json!({"id": e.id, "resolver": e.resolver}))
        .collect();

    let msg = json!({
        "to": "daemon",
        "type": "register-refs",
        "nav_generation": nav_generation,
        "refs": refs_json,
    });
    transport
        .send(&msg)
        .context("sending register-refs to daemon")?;

    for _ in 0..64 {
        let resp = transport
            .recv()
            .context("receiving register-refs response")?;
        if resp.get("from").and_then(Value::as_str) == Some("daemon") {
            if let Some(err) = resp.get("error").and_then(Value::as_str) {
                if resp.get("stale").and_then(Value::as_bool) == Some(true) {
                    anyhow::bail!("ref registration skipped: page navigated during dom evaluation");
                }
                anyhow::bail!("daemon register-refs error: {err}");
            }
            return Ok(());
        }
    }
    anyhow::bail!("did not receive register-refs response within 64 frames")
}

/// Format a hint pointing to the daemon log file, or an empty string if
/// the path cannot be determined.
fn log_path_hint() -> String {
    match super::registry::log_path() {
        Ok(p) => format!(" (check {} for details)", p.display()),
        Err(_) => String::new(),
    }
}

// ---------------------------------------------------------------------------
// A5: Fast-fail Firefox port probe
// ---------------------------------------------------------------------------

/// Return `true` if `host:port` accepts a TCP connection within 100 ms.
///
/// Resolves `host` via DNS (so `localhost` works) and tries each resolved
/// address in turn. Used as a quick pre-spawn check: if Firefox's debug port
/// is dark there is no point spawning a daemon (which would wait up to 5 s
/// to time out).
fn is_firefox_port_open(host: &str, port: u16) -> bool {
    use std::net::ToSocketAddrs;
    let Ok(addrs) = (host, port).to_socket_addrs() else {
        // Resolution failed — let the normal path surface the error.
        return true;
    };
    for addr in addrs {
        if TcpStream::connect_timeout(&addr, Duration::from_millis(100)).is_ok() {
            return true;
        }
    }
    false
}

// ---------------------------------------------------------------------------
// A4: daemon status / stop CLI handlers
// ---------------------------------------------------------------------------

/// Connect to the daemon (after auth), send a raw daemon message, and return
/// the daemon's response.
///
/// `port` selects which per-port registry entry to use — callers that operate
/// on an explicit target port (e.g. [`stop_prior_instance`]) must pass that
/// port rather than always defaulting to `cli.port`, so the RPC is sent to the
/// daemon actually addressed (iter-123 Theme B).
///
/// On failure (daemon not found, auth error, etc.) returns an `AppError`.
fn daemon_rpc(cli: &Cli, port: u16, msg: &serde_json::Value) -> Result<Value, AppError> {
    let info = registry::read_registry(port)
        .map_err(|e| AppError::Internal(anyhow::anyhow!("reading daemon registry: {e}")))?
        .ok_or_else(|| AppError::User("no daemon is running".to_owned()))?;

    if !process::is_process_alive(info.pid) {
        registry::remove_registry(port).ok();
        return Err(AppError::User(
            "daemon process is no longer alive".to_owned(),
        ));
    }

    let addr = format!("127.0.0.1:{}", info.proxy_port)
        .parse::<std::net::SocketAddr>()
        .map_err(|e| AppError::Internal(anyhow::anyhow!("parsing daemon addr: {e}")))?;

    let timeout = Duration::from_millis(cli.timeout);
    let stream = TcpStream::connect_timeout(&addr, timeout)
        .map_err(|e| AppError::Connection(format!("could not connect to daemon: {e}")))?;
    stream
        .set_read_timeout(Some(timeout))
        .map_err(|e| AppError::Internal(anyhow::anyhow!("setting read timeout: {e}")))?;

    // Auth handshake.
    let mut writer = FramedWriter::from_stream(
        stream
            .try_clone()
            .map_err(|e| AppError::Internal(anyhow::anyhow!("cloning stream: {e}")))?,
    );
    writer
        .send(&json!({"auth": info.auth_token}))
        .map_err(|e| AppError::Internal(anyhow::anyhow!("sending auth frame: {e}")))?;

    // Read the greeting that the daemon sends after successful auth.
    let mut reader = FramedReader::from_stream(stream);
    reader
        .recv()
        .map_err(|e| AppError::User(format!("daemon auth failed or connection closed: {e}")))?;

    // Send the actual RPC message.
    writer
        .send(msg)
        .map_err(|e| AppError::Internal(anyhow::anyhow!("sending daemon RPC: {e}")))?;

    // Read the daemon's response, skipping any forwarded Firefox push frames
    // (consoleAPICall, network events, etc.) that may arrive in between.
    //
    // We use a deadline rather than a fixed frame cap because under heavy
    // push traffic the response could legitimately arrive after many pushes.
    // The socket already has `cli.timeout` set as its read timeout, so a
    // genuinely-stuck daemon will still surface an error promptly.
    let deadline = std::time::Instant::now() + timeout;
    while std::time::Instant::now() < deadline {
        let response = reader
            .recv()
            .map_err(|e| AppError::Internal(anyhow::anyhow!("receiving daemon response: {e}")))?;
        if response.get("from").and_then(Value::as_str) == Some("daemon") {
            if let Some(err) = response.get("error").and_then(Value::as_str) {
                return Err(AppError::User(format!("daemon error: {err}")));
            }
            return Ok(response);
        }
        // Otherwise it's a forwarded Firefox push frame — drop and keep reading.
    }
    Err(AppError::Internal(anyhow::anyhow!(
        "did not receive daemon response within {}ms",
        timeout.as_millis()
    )))
}

/// `ff-rdp daemon status` — print daemon status as JSON.
pub(crate) fn run_daemon_status(cli: &Cli) -> Result<(), AppError> {
    let result = match registry::read_registry(cli.port)
        .map_err(|e| AppError::Internal(anyhow::anyhow!("reading daemon registry: {e}")))?
    {
        None => json!({
            "running": false,
            "pid": null,
            "port": null,
            "uptime_seconds": null,
            "connections": null,
            "buffer_sizes": null,
        }),
        Some(ref info) if !process::is_process_alive(info.pid) => {
            registry::remove_registry(cli.port).ok();
            json!({
                "running": false,
                "pid": null,
                "port": null,
                "uptime_seconds": null,
                "connections": null,
                "buffer_sizes": null,
            })
        }
        Some(ref info) => {
            // Pull live stats from the daemon. If the RPC fails, surface
            // whatever registry data we have with null stats so callers can
            // still see the PID/port.
            let (uptime_seconds, connections, buffer_sizes, target_count, live_target_count) =
                match daemon_rpc(cli, cli.port, &json!({"to": "daemon", "type": "status"})) {
                    Ok(resp) => (
                        resp.get("uptime_secs").and_then(Value::as_u64),
                        resp.get("stream_subscriber_count").and_then(Value::as_u64),
                        resp.get("buffer_sizes").cloned(),
                        resp.get("target_count").and_then(Value::as_u64),
                        // iter-137 Theme A: targets alive right now, as opposed
                        // to the cumulative `target_count`.  This is what a
                        // proxied `click --frame` / `consent accept` enumerates.
                        resp.get("live_target_count").and_then(Value::as_u64),
                    ),
                    Err(_) => (None, None, None, None, None),
                };
            json!({
                "running": true,
                "pid": info.pid,
                "port": info.proxy_port,
                "uptime_seconds": uptime_seconds,
                "connections": connections,
                "buffer_sizes": buffer_sizes,
                "target_count": target_count,
                "live_target_count": live_target_count,
            })
        }
    };

    let meta = json!({});
    let envelope = output::envelope(&result, 1, &meta);
    OutputPipeline::from_cli(cli)?.finalize(&envelope)
}

/// Injected dependencies for the stop paths (iter-158 Themes B and C).
///
/// Bundles the escalation hooks with a launch-record directory override so the
/// ordering invariant Theme B restores — *the record survives a failed stop* —
/// is unit-testable without spawning a real Firefox or touching the user's
/// `~/.ff-rdp`.
pub(crate) struct StopDeps {
    pub(crate) hooks: EscalationHooks,
    /// Directory holding the per-port launch records. `None` means the real
    /// `daemon_record::record_base_dir()` (`~/.ff-rdp`).
    pub(crate) record_dir: Option<std::path::PathBuf>,
    /// Ownership check applied to a launch record's PID before any signal is
    /// sent to it (iter-191) — see [`RecordOwnerCheck`].
    pub(crate) record_pid_is_ours: RecordOwnerCheck,
}

/// Does this launch record's PID still name the process ff-rdp launched?
///
/// Injected rather than called directly so the "record matches the port, its
/// PID is alive, the PID is not ours" case is unit-testable without waiting
/// for the OS to actually recycle a PID onto this test binary. Production
/// wiring is [`crate::daemon_record::record_pid_is_ours`]; its doc comment
/// carries the reasoning and the 2026-08-23 observation behind it.
pub(crate) type RecordOwnerCheck = fn(&crate::daemon_record::DaemonRecord) -> bool;

impl StopDeps {
    /// Production dependencies.
    pub(crate) fn real() -> Self {
        Self {
            hooks: EscalationHooks::real(),
            record_dir: None,
            record_pid_is_ours: crate::daemon_record::record_pid_is_ours,
        }
    }

    fn read_record(&self, port: u16) -> Result<Option<crate::daemon_record::DaemonRecord>> {
        match &self.record_dir {
            Some(dir) => crate::daemon_record::read_in(dir, port),
            None => crate::daemon_record::read(port),
        }
    }

    fn remove_record(&self, port: u16) {
        let _ = match &self.record_dir {
            Some(dir) => crate::daemon_record::remove_in(dir, port),
            None => crate::daemon_record::remove(port),
        };
    }
}

/// `ff-rdp daemon stop` — gracefully stop the running daemon and free the Firefox port.
///
/// `port` is the Firefox debug port to act on. Top-level callers (the `daemon
/// stop` CLI handler) pass `cli.port`; [`stop_prior_instance`] passes its own
/// explicit `port` parameter, which may differ from `cli.port` (e.g.
/// `launch --debug-port N --replace` where `N != cli.port`) — threading `port`
/// through here (instead of implicitly using `cli.port` everywhere) ensures we
/// always act on the daemon actually addressed, not whichever one happens to be
/// registered under `cli.port` (iter-123 Theme B).
///
/// Stop sequence (iter-90):
/// 1. Check the [`DaemonRecord`] (written by both `launch` and `daemon start`).
///    If present: SIGTERM, wait, SIGKILL, poll port, remove record.
/// 2. If no DaemonRecord: fall through to the existing proxy-daemon registry path
///    (for instances started via `daemon start`).
/// 3. Registry path: send graceful shutdown RPC → SIGTERM → SIGKILL → poll port.
pub(crate) fn run_daemon_stop(cli: &Cli, port: u16) -> Result<(), AppError> {
    let result = stop_daemon_and_build_result(cli, port)?;
    let meta = json!({});
    let envelope = output::envelope(&result, 1, &meta);
    OutputPipeline::from_cli(cli)?.finalize(&envelope)
}

/// Core stop logic shared by [`run_daemon_stop`] (the standalone `daemon
/// stop` CLI command, which prints this as its own top-level envelope) and
/// [`stop_prior_instance`] (`launch --replace`'s internal stop-before-relaunch
/// step, which must NOT print anything — see iter-153).
///
/// Returns the `results` JSON object for the stop outcome. Callers that print
/// it wrap it in an envelope themselves; callers that fold it into another
/// command's output read the fields they need straight from the `Value`.
fn stop_daemon_and_build_result(cli: &Cli, port: u16) -> Result<Value, AppError> {
    stop_daemon_and_build_result_with(cli, port, &StopDeps::real())
}

/// [`stop_daemon_and_build_result`] with its process/record dependencies
/// injected — see [`StopDeps`].
fn stop_daemon_and_build_result_with(
    cli: &Cli,
    port: u16,
    deps: &StopDeps,
) -> Result<Value, AppError> {
    // ----------------------------------------------------------------
    // 1. Check the shared DaemonRecord (written by `launch`).
    //    Only act on records whose `port` matches the target `port` so a
    //    stray `daemon stop` cannot kill an unrelated instance the user did
    //    not address. `read()` already filters out stale (dead-PID)
    //    records, so we don't need to recheck liveness here.
    // ----------------------------------------------------------------
    match deps
        .read_record(port)
        .map_err(|e| AppError::Internal(anyhow::anyhow!("reading daemon record: {e}")))?
    {
        Some(rec) if rec.port == port => {
            // iter-191: the same identity gate `stop_prior_instance_with`
            // applies. `read_in` filtered dead PIDs, which is a liveness test,
            // not an identity one — this path reads the *same* artefact and
            // sends the *same* group-wide signals, so a stale record here
            // authorises the same kill against a recycled PID. `daemon stop`
            // refusing is strictly better than `daemon stop` killing a
            // stranger; the record survives for iter-186's GC.
            if !(deps.record_pid_is_ours)(&rec) {
                return Err(AppError::User(unowned_record_pid_msg(rec.pid, port)));
            }
            // Live instance found via DaemonRecord and matches --port — kill it.
            let (stopped, port_free, escalation_msg) =
                stop_pid_with_full_escalation(rec.pid, Some(rec.port), &deps.hooks, None);

            if !port_free {
                // iter-158 Theme B: the record is the ownership proof, and it
                // must OUTLIVE a failed stop. Removing it here (as this path
                // did unconditionally, `client.rs:963`) meant the next
                // `launch --replace` found no DaemonRecord, fell through to
                // the raw port-owner lookup, and was refused by the
                // fails-closed guard — "no owner-PID marker" fired against an
                // instance ff-rdp launched itself. Keep the record so the
                // retry re-enters the DaemonRecord branch, which is permitted
                // to kill.
                let msg = if escalation_msg.is_empty() {
                    port_still_listening_msg(rec.pid, rec.port)
                } else {
                    escalation_msg
                };
                return Err(AppError::User(msg));
            }
            deps.remove_record(rec.port);

            // iter-96 Theme A: the escalation ladder reported success (port
            // freed AND process gone) — safe to reclaim the temp profile dir
            // now. `cleanup_profile_dir` refuses anything that isn't a
            // ff-rdp-managed dir under `secure_profile_root()`, so a
            // user-supplied `--profile` path is never touched here.
            let profile_removed_path = if stopped {
                crate::util::profile_dir::cleanup_profile_dir(&rec.profile_dir)
                    .removed_path()
                    .map(std::path::Path::to_path_buf)
            } else {
                None
            };

            return Ok(json!({
                "stopped": stopped,
                "pid": rec.pid,
                "port": rec.port,
                "profile_removed": profile_removed_path.is_some(),
                "profile_removed_path": profile_removed_path
                    .as_ref()
                    .map(|p| p.to_string_lossy().into_owned()),
            }));
        }
        _ => {
            // No record, or record is for a different port — fall through
            // to the proxy-daemon registry path below.
        }
    }

    // ----------------------------------------------------------------
    // 2. Proxy-daemon registry path (instances started via `daemon start`).
    // ----------------------------------------------------------------

    // Read registry to get PID and port for process-group killing + port poll.
    // Keyed by the target `port` so `daemon stop` only ever acts on the daemon
    // the caller addressed, even when that differs from `cli.port` (iter-123
    // Theme B).
    let Some(info) = registry::read_registry(port)
        .map_err(|e| AppError::Internal(anyhow::anyhow!("reading daemon registry: {e}")))?
    else {
        // No daemon running — report success (idempotent).
        return Ok(json!({"stopped": false, "reason": "not running"}));
    };

    let firefox_port = info.firefox_port;

    if !process::is_process_alive(info.pid) {
        registry::remove_registry(firefox_port).ok();
        return Ok(json!({"stopped": true, "reason": "already dead"}));
    }

    // iter-142 Theme A: `info.pid` is the *proxy daemon's own* PID
    // (`std::process::id()`, set in `daemon/server.rs`), not Firefox's — the
    // daemon connects to an already-running Firefox as an RDP client, it
    // never spawns one. Killing only `info.pid` stops the proxy but leaves
    // Firefox (and the listening debug port callers actually care about)
    // untouched — exactly the "port still listening" false-negative
    // dogfooding session 63 reproduced 3/3, and the reason the error
    // reported the daemon's PID as if it were Firefox's. Resolve the real
    // Firefox process via the same ownership-verified port-owner lookup
    // `stop_prior_instance` uses, so both the kill and the reported `pid`
    // target the process that is actually holding the port.
    let firefox_pid = crate::port_owner::find_listener(firefox_port)
        .ok()
        .flatten()
        .filter(|owner| crate::util::profile_dir::pid_is_ff_rdp_spawned(owner.pid))
        .map(|owner| owner.pid);

    // 1. Try graceful shutdown via RPC first (asks the proxy daemon to exit).
    let rpc_ok = daemon_rpc(
        cli,
        firefox_port,
        &json!({"to": "daemon", "type": "shutdown"}),
    )
    .is_ok();

    if rpc_ok {
        // Give the daemon up to 2 seconds to exit cleanly after the RPC.
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        loop {
            if !process::is_process_alive(info.pid) {
                break;
            }
            if std::time::Instant::now() >= deadline {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    // iter-191, the scope item this iteration owed an answer on: is the
    // registry path safe from the same recycled-PID defect as the launch
    // record? **No — the RPC handshake above gates nothing.** `rpc_ok` is
    // never consulted before the direct kill below, so a registry whose PID
    // the OS has reissued gets that stranger signalled exactly as branch 1
    // did. What differs is the exposure and therefore the remedy:
    //
    // * `daemon.<port>.json` is removed on every clean daemon stop and is
    //   not subject to iter-186's one-file-per-ephemeral-port leak, so the
    //   stale population is orders of magnitude smaller;
    // * `info.pid` is the *proxy daemon's own* PID, and the escalation runs
    //   with `port: None`, so it cannot even reach the port-wait/tree-kill
    //   steps — the blast radius is the pid-level signals alone.
    //
    // So this path refuses only on positive disproof (`Recycled`), where the
    // launch record refuses on anything short of positive proof. An absent
    // token — every registry written by a pre-iter-191 daemon — keeps the
    // old behaviour rather than stranding a running daemon that `daemon stop`
    // would then be unable to stop.
    let daemon_pid_recycled =
        process::pid_identity(info.pid, info.start_token.as_deref()) == process::PidIdentity::Recycled;
    if daemon_pid_recycled {
        tracing::warn!(
            "daemon stop: registry for port {firefox_port} names pid {} but that PID has since \
             been reused by an unrelated process — not signalling it",
            info.pid
        );
    }

    // 2. If the proxy daemon is still alive, SIGTERM then SIGKILL it directly
    //    — this only stops the proxy, not Firefox (see note above). `None` for
    //    the port: the proxy never binds the Firefox debug port, so there is
    //    nothing for the ladder to wait on here (iter-158 Theme C).
    if !daemon_pid_recycled && process::is_process_alive(info.pid) {
        let _ = stop_pid_with_full_escalation(info.pid, None, &deps.hooks, None);
    }

    // 3. Stop the actual Firefox process (if one was found and verified as
    //    ff-rdp-owned) — this is the process actually holding `firefox_port`
    //    open, so this call runs the full ladder including the port wait and
    //    the tree kill. Escalate against the real Firefox PID when known;
    //    escalating against the daemon PID (the pre-iter-142 behaviour) can
    //    never free the port, since the daemon never held it.
    // iter-191: the `unwrap_or` fallback is the one place a *recycled* daemon
    // PID would reach the full ladder (port wait + tree kill). When the
    // registry PID is disproven there is nothing here ff-rdp may signal, so
    // fall through to the port check alone.
    let escalation_target = match (firefox_pid, daemon_pid_recycled) {
        (Some(pid), _) => Some(pid),
        (None, false) => Some(info.pid),
        (None, true) => None,
    };
    let (port_free, escalation_msg) = match escalation_target {
        Some(target) => {
            let (_, port_free, msg) =
                stop_pid_with_full_escalation(target, Some(firefox_port), &deps.hooks, None);
            (port_free, msg)
        }
        None => (
            (deps.hooks.wait_port_closed)(firefox_port, PORT_FREE_WAIT_BOUND),
            port_still_listening_msg(info.pid, firefox_port),
        ),
    };

    // 4. Clean up the daemon registry regardless of process state.
    registry::remove_registry(firefox_port).ok();

    if !port_free {
        return Err(AppError::User(escalation_msg));
    }

    let daemon_stopped = !process::is_process_alive(info.pid);
    let firefox_stopped = firefox_pid.is_none_or(|pid| !process::is_process_alive(pid));
    let stopped = daemon_stopped && firefox_stopped;
    Ok(json!({"stopped": stopped, "pid": firefox_pid}))
}

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

/// Stop an existing Firefox instance on `port` to make way for a fresh launch.
///
/// Used by `launch --replace` / `launch --force` (iter-86 Theme A / iter-90).
/// Returns the [`StopOutcome`] describing what was stopped if the port is
/// free afterwards, `Err` if it is still in use.
///
/// This function must never print a JSON envelope of its own — it runs
/// *inside* `launch`'s command handler, and a second top-level `println!`
/// here would corrupt `launch --replace`'s stdout with two documents back to
/// back (iter-153). Every path below returns a [`StopOutcome`] value instead
/// of calling `OutputPipeline::finalize`.
///
/// Stop priority (iter-90):
/// 1. DaemonRecord matching the requested port → kill, wait, remove record.
/// 2. Proxy-daemon registry matching the port → graceful `daemon stop` RPC path.
/// 3. Fall back to port-owner lookup.
pub(crate) fn stop_prior_instance(cli: &Cli, port: u16) -> Result<StopOutcome, AppError> {
    stop_prior_instance_with(cli, port, &StopDeps::real())
}

/// [`stop_prior_instance`] with its process/record dependencies injected — see
/// [`StopDeps`].
pub(crate) fn stop_prior_instance_with(
    cli: &Cli,
    port: u16,
    deps: &StopDeps,
) -> Result<StopOutcome, AppError> {
    // 1. Check shared DaemonRecord first (covers instances started via `launch`).
    match deps
        .read_record(port)
        .map_err(|e| AppError::Internal(anyhow::anyhow!("reading daemon record: {e}")))?
    {
        Some(rec) if rec.port == port && (deps.hooks.is_alive)(rec.pid) => {
            // iter-191: `is_alive` above says only that *some* process holds
            // `rec.pid` — not that it is the Firefox this record was written
            // for. A record that outlived its browser (a crash between
            // `launch` and cleanup; one of the per-port records iter-186
            // reclaims) names a number the OS is free to reissue, and
            // everything below this point signals a process *group* derived
            // from it. On 2026-08-23 that fired for real: a record from seven
            // days earlier named a PID then held by an unrelated desktop app,
            // and `launch --replace` sent it SIGTERM and SIGKILL. Nothing died
            // only because that PID happened not to be a group leader — which
            // `launch`'s own children always are, and which Windows does not
            // even model (`kill_process_group` there is a plain
            // `kill_process`). Prove identity before signalling, and refuse
            // exactly as the port-owner branch below does when the proof is
            // missing.
            if !(deps.record_pid_is_ours)(&rec) {
                // Leave the record in place: iter-158 Theme B — a failed stop
                // must not destroy its own ownership proof, and this stop
                // never started. iter-186's GC is what reclaims it.
                return Err(AppError::User(unowned_record_pid_msg(rec.pid, port)));
            }
            let (stopped, port_free, _escalation_msg) =
                stop_pid_with_full_escalation(rec.pid, Some(rec.port), &deps.hooks, None);
            if !port_free {
                // iter-158 Theme B: keep the record. A failed stop must not
                // destroy its own ownership proof — see the matching comment
                // in `stop_daemon_and_build_result_with`. A dogfood lane
                // observed three `launch --replace` attempts produce three
                // errors, twice "no owner-PID marker", because this removal
                // ran before the port check.
                return Err(AppError::User(format!(
                    "port {port} is still in use after stopping the prior instance (pid {}). \
                     Run `ff-rdp doctor` or `lsof -i :{port}` to investigate.",
                    rec.pid
                )));
            }
            deps.remove_record(rec.port);
            return Ok(StopOutcome {
                stopped,
                pid: Some(rec.pid),
            });
        }
        Some(rec) if rec.port == port => {
            // Record exists but PID is dead — clean up and proceed (port may
            // already be free). Distinct from the `!port_free` case above:
            // here the process is genuinely gone and the record is stale, so
            // there is no ownership trail left to preserve.
            deps.remove_record(rec.port);
        }
        _ => {}
    }

    // 2. Proxy-daemon registry — use the graceful stop path. Calls the same
    //    core logic `run_daemon_stop` uses (`stop_daemon_and_build_result`)
    //    directly, WITHOUT going through `run_daemon_stop` itself — that
    //    function prints its own top-level envelope, which is exactly the
    //    iter-153 double-envelope defect this refactor removes.
    //    Keyed by the target `port` (iter-123 Theme B) so we only take the
    //    graceful path when a daemon record actually exists for this port.
    match registry::read_registry(port) {
        Ok(Some(ref info)) if info.firefox_port == port => {
            let result = stop_daemon_and_build_result(cli, port)?;
            let stopped = result
                .get("stopped")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false);
            let pid = result
                .get("pid")
                .and_then(serde_json::Value::as_u64)
                .and_then(|p| u32::try_from(p).ok());
            return Ok(StopOutcome { stopped, pid });
        }
        _ => {}
    }

    // 3. No registry — try to kill whatever is on the port by PID from
    //    the port-owner helper, then wait for the port to free.
    let mut owner_pid: Option<u32> = None;
    if let Ok(Some(owner)) = crate::port_owner::find_listener(port) {
        // iter-110 Theme A0: never signal a process we did not spawn. The
        // port-owner lookup finds whatever is *listening on the RDP port* —
        // which may be a Firefox the user launched by hand on ff-rdp's default
        // port 6000. Killing it (the 2026-07-09 incident) is never acceptable.
        // Require a positive ownership proof — an owner-PID marker naming this
        // PID under our managed profile root — before any signal is sent.
        // Fails closed: no marker ⇒ no kill (see `pid_is_ff_rdp_spawned`).
        if !crate::util::profile_dir::pid_is_ff_rdp_spawned(owner.pid) {
            return Err(AppError::User(format!(
                "port {port} is in use by {} (PID {}), which ff-rdp did not launch \
                 (no owner-PID marker). Refusing to stop a process ff-rdp does not own — \
                 stop it yourself, or pass --port to use a different port.",
                owner.process_name, owner.pid
            )));
        }
        owner_pid = Some(owner.pid);
        // iter-100 Theme D: re-verify port ownership immediately before each
        // signal. `find_listener` resolves a PID at time T; between T and the
        // signal the original process may have exited and the OS may have
        // recycled that PID onto an unrelated process. Only this branch needs
        // the check — the DaemonRecord paths above got their PID from a file
        // ff-rdp wrote — so it is passed as the ladder's optional
        // `reverify` parameter rather than being baked into the ladder
        // (iter-158 Theme C).
        let (_, port_free, escalation_msg) = stop_pid_with_full_escalation(
            owner.pid,
            Some(port),
            &deps.hooks,
            Some(port_still_owned_by),
        );
        if !port_free {
            return Err(AppError::User(escalation_msg));
        }
        return Ok(StopOutcome {
            stopped: true,
            pid: owner_pid,
        });
    }

    // Nothing is listening we could identify — the port may already be free.
    if !process::wait_for_port_closed(port, PORT_FREE_WAIT_BOUND) {
        return Err(AppError::User(format!(
            "port {port} is still in use after stopping the prior instance. \
             Run `ff-rdp doctor` or `lsof -i :{port}` to investigate."
        )));
    }
    Ok(StopOutcome {
        stopped: true,
        pid: owner_pid,
    })
}

/// [`OwnershipCheck`] for the port-owner stop path: does `pid` still own
/// `port`? Used as the `reverify` argument to
/// [`stop_pid_with_full_escalation`] so a recycled PID is never signalled.
fn port_still_owned_by(port: u16, pid: u32) -> bool {
    matches!(
        crate::port_owner::find_listener(port),
        Ok(Some(ref current)) if current.pid == pid
    )
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // iter-158 Themes B and C — the ownership trail and the single ladder
    // -----------------------------------------------------------------------

    /// A recording [`EscalationHooks`] stub: appends each hook's name to a
    /// shared log so a test can assert on the exact call ORDER, which is what
    /// Theme C's fix is about (pgid captured before the first kill).
    ///
    /// `fn` pointers cannot capture, so the log lives in a `static`. Each test
    /// using it clears the log first and runs single-threaded within itself.
    static CALL_LOG: std::sync::Mutex<Vec<&'static str>> = std::sync::Mutex::new(Vec::new());

    /// Serializes the tests that share [`CALL_LOG`] — `cargo test` runs them
    /// on separate threads by default, and a shared static log without this
    /// would interleave one test's calls into another's assertions.
    static CALL_LOG_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn log_call(name: &'static str) {
        if let Ok(mut v) = CALL_LOG.lock() {
            v.push(name);
        }
    }

    /// Acquire the serialization lock and start a fresh log. The returned
    /// guard must stay alive for the whole test.
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

    /// AC `unit_158_stop_ladder_captures_pgid_before_any_kill`: `get_pgid` is
    /// called strictly before the first of `kill_group_term` /
    /// `kill_group_kill` / `kill_process_tree`.
    ///
    /// Pre-158 the pgid was captured "first" only *within* `run_escalation` —
    /// by which time `kill_pid_and_wait_port` had already SIGTERMed the
    /// parent, so `getpgid` on a dying/dead pid could return `None` and the
    /// tree kill became a no-op.
    #[test]
    fn unit_158_stop_ladder_captures_pgid_before_any_kill() {
        let _serialized = begin_call_log();
        let hooks = recording_hooks_dead_parent();
        let _ = stop_pid_with_full_escalation(4242, Some(65000), &hooks, None);

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

    /// AC `unit_158_stop_ladder_reaches_tree_kill_when_parent_is_dead`: with
    /// `is_alive` false and the port never closing, the ladder still reaches
    /// `kill_process_tree`.
    ///
    /// Pre-158 `run_escalation` returned at its `if !(h.is_alive)(pid)` guard,
    /// which its only production caller had already made true by killing the
    /// pid first — so steps 3-7 were dead code and `"port still listening
    /// after 8s"` was the visible symptom.
    #[test]
    fn unit_158_stop_ladder_reaches_tree_kill_when_parent_is_dead() {
        let _serialized = begin_call_log();
        let hooks = recording_hooks_dead_parent();
        let (stopped, port_free, msg) =
            stop_pid_with_full_escalation(4242, Some(65001), &hooks, None);

        let log = take_call_log();
        assert!(
            log.contains(&"kill_process_tree"),
            "the tree kill must run even when the parent is already dead; log was {log:?}"
        );
        assert!(stopped, "a dead parent counts as stopped");
        assert!(!port_free, "the stub never frees the port");
        assert!(
            !msg.is_empty(),
            "a failed stop must carry an escalation message"
        );
    }

    /// On Unix, a dead parent yields `None` from `getpgid` — yet that is
    /// exactly the case the tree kill exists for. The ladder falls back to
    /// `pid` as the group id (`launch` guarantees pgid == pid), so
    /// `kill_process_tree` receives a usable value instead of the `None` that
    /// `process::kill_process_tree` silently ignores.
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
            // The dead-parent case: getpgid fails.
            get_pgid: |_pid| None,
            wait_port_closed: |_port, _timeout| false,
        };
        let _ = stop_pid_with_full_escalation(4242, Some(65002), &hooks, None);
        assert_eq!(
            SEEN_PGID.load(Ordering::SeqCst),
            4242,
            "the tree kill must fall back to the pid as the group id"
        );
    }

    /// With no port to free (the proxy daemon, which never binds Firefox's
    /// debug port) the ladder stops after the pid-level signals and reports
    /// the port free — it must not wait out `PORT_FREE_WAIT_BOUND` on a port
    /// the target never held.
    #[test]
    fn unit_158_stop_ladder_without_port_skips_the_port_wait() {
        let _serialized = begin_call_log();
        let hooks = recording_hooks_dead_parent();
        let (stopped, port_free, msg) = stop_pid_with_full_escalation(4242, None, &hooks, None);
        let log = take_call_log();
        assert!(stopped);
        assert!(port_free, "no port to free ⇒ nothing can be blocking");
        assert!(msg.is_empty());
        assert!(
            !log.contains(&"wait_port_closed"),
            "no port wait may run when there is no port; log was {log:?}"
        );
    }

    /// Hooks whose kill steps are all recorded and whose target never dies —
    /// used by the iter-191 tests, where the assertion is that *nothing* in
    /// this set is ever called.
    fn recording_hooks_live_parent() -> EscalationHooks {
        EscalationHooks {
            is_alive: |_pid| true,
            kill_group_term: |_pid| log_call("kill_group_term"),
            kill_group_kill: |_pid| log_call("kill_group_kill"),
            kill_process_tree: |_pid, _pgid| log_call("kill_process_tree"),
            get_pgid: |pid| Pgid::try_from(pid).ok(),
            wait_port_closed: |_port, _timeout| {
                log_call("wait_port_closed");
                true
            },
        }
    }

    /// A launch record for `port` naming this process, planted in `dir`.
    ///
    /// The PID must be genuinely alive — `daemon_record::read_in` performs its
    /// own liveness check and treats a dead PID as an absent record, which
    /// would skip the branch under test entirely. This test binary is the
    /// convenient live PID; every hook that could signal it is stubbed.
    fn plant_live_record(dir: &std::path::Path, port: u16) {
        let rec = crate::daemon_record::DaemonRecord {
            pid: std::process::id(),
            port,
            headless: true,
            launched_at: chrono::Utc::now(),
            profile_dir: dir.join("profile"),
            start_token: Some("token-from-a-process-that-is-long-gone".to_owned()),
        };
        crate::daemon_record::write_in(dir, &rec).expect("write record");
    }

    /// AC (iter-191): `launch --replace` finds a record that matches the port
    /// and whose PID is alive, but that PID is not the process ff-rdp launched
    /// — **no kill hook may run**.
    ///
    /// This is the 2026-08-23 sweep failure in unit form: a seven-day-old
    /// `launch-record.<port>.json` named a PID the OS had reissued to an
    /// unrelated desktop application, and this branch signalled its process
    /// group without ever consulting an ownership proof.
    #[test]
    fn unit_191_replace_refuses_record_pid_that_is_not_ours() {
        let _serialized = begin_call_log();
        let dir = tempfile::tempdir().expect("tempdir");
        let port = 64_331u16;
        plant_live_record(dir.path(), port);

        let deps = StopDeps {
            hooks: recording_hooks_live_parent(),
            record_dir: Some(dir.path().to_path_buf()),
            record_pid_is_ours: |_rec| false,
        };
        let cli = <Cli as clap::Parser>::try_parse_from(["ff-rdp", "launch"]).expect("parse cli");

        let err = stop_prior_instance_with(&cli, port, &deps)
            .expect_err("an unowned record PID must not be stopped");

        let log = take_call_log();
        assert!(
            !log.iter().any(|c| c.starts_with("kill_")),
            "no signal may be sent to a PID ff-rdp cannot prove it spawned; log was {log:?}"
        );

        let AppError::User(msg) = err else {
            panic!("expected a user-facing refusal, not an internal error");
        };
        // The exact phrases `live_110_replace_never_kills_foreign_firefox`
        // asserts on, and the ones the port-owner branch already emits.
        assert!(
            msg.contains("did not launch"),
            "refusal must say ff-rdp did not launch the process; got: {msg}"
        );
        assert!(
            msg.contains("does not own"),
            "refusal must say ff-rdp will not stop what it does not own; got: {msg}"
        );
        assert!(
            !msg.contains("still in use after stopping the prior instance"),
            "that message claims ff-rdp stopped something; nothing was stopped here. got: {msg}"
        );

        assert!(
            crate::daemon_record::read_in(dir.path(), port)
                .expect("read record")
                .is_some(),
            "a refused stop must leave the record for iter-186's GC (iter-158 Theme B)"
        );
    }

    /// The same gate on `daemon stop`'s copy of the record branch: it reads
    /// the same artefact and sends the same group-wide signals, so it must
    /// refuse the same way rather than killing a stranger.
    #[test]
    fn unit_191_daemon_stop_refuses_record_pid_that_is_not_ours() {
        let _serialized = begin_call_log();
        let dir = tempfile::tempdir().expect("tempdir");
        let port = 64_332u16;
        plant_live_record(dir.path(), port);

        let deps = StopDeps {
            hooks: recording_hooks_live_parent(),
            record_dir: Some(dir.path().to_path_buf()),
            record_pid_is_ours: |_rec| false,
        };
        let cli = <Cli as clap::Parser>::try_parse_from(["ff-rdp", "launch"]).expect("parse cli");

        let err = stop_daemon_and_build_result_with(&cli, port, &deps)
            .expect_err("an unowned record PID must not be stopped");

        let log = take_call_log();
        assert!(
            !log.iter().any(|c| c.starts_with("kill_")),
            "no signal may be sent to a PID ff-rdp cannot prove it spawned; log was {log:?}"
        );
        assert!(
            matches!(&err, AppError::User(m) if m.contains("does not own")),
            "expected the ownership refusal; got: {err:?}"
        );
        assert!(
            crate::daemon_record::read_in(dir.path(), port)
                .expect("read record")
                .is_some(),
            "a refused stop must leave the record in place"
        );
    }

    /// The counterpart to the two tests above: when the record *is* ours the
    /// gate stays out of the way and the ladder runs. Without this, "refuse
    /// everything" would pass the iter-191 assertions while breaking
    /// `launch --replace` outright.
    #[test]
    fn unit_191_replace_still_stops_a_record_that_is_ours() {
        let _serialized = begin_call_log();
        let dir = tempfile::tempdir().expect("tempdir");
        let port = 64_333u16;
        plant_live_record(dir.path(), port);

        let deps = StopDeps {
            hooks: recording_hooks_live_parent(),
            record_dir: Some(dir.path().to_path_buf()),
            record_pid_is_ours: |_rec| true,
        };
        let cli = <Cli as clap::Parser>::try_parse_from(["ff-rdp", "launch"]).expect("parse cli");

        let outcome = stop_prior_instance_with(&cli, port, &deps).expect("an owned record stops");
        assert_eq!(outcome.pid, Some(std::process::id()));
        let log = take_call_log();
        assert!(
            log.iter().any(|c| c.starts_with("kill_")),
            "an owned record must still be signalled; log was {log:?}"
        );
    }

    /// AC `unit_158_record_survives_failed_stop`: a stop that leaves the port
    /// held must NOT delete the `DaemonRecord`. The record is the ownership
    /// proof; deleting it drops the next `launch --replace` into the raw
    /// port-owner branch, whose fails-closed guard then refuses with "no
    /// owner-PID marker" against an instance ff-rdp launched itself.
    ///
    /// Asserted for both stop entry points that had the ordering wrong
    /// (`client.rs:1151` and `client.rs:963` pre-158).
    #[test]
    fn unit_158_record_survives_failed_stop() {
        let dir = tempfile::tempdir().expect("tempdir");
        let port = 64_321u16;
        // The record's PID must be genuinely alive — `daemon_record::read_in`
        // performs its own liveness check and treats a dead PID as absent.
        // This test process is the most convenient live PID; the injected
        // hooks are all no-ops, so nothing ever signals it.
        let rec = crate::daemon_record::DaemonRecord {
            pid: std::process::id(),
            port,
            headless: true,
            launched_at: chrono::Utc::now(),
            profile_dir: dir.path().join("profile"),
            start_token: None,
        };

        let deps = StopDeps {
            hooks: EscalationHooks {
                is_alive: |_pid| true,
                kill_group_term: |_pid| {},
                kill_group_kill: |_pid| {},
                kill_process_tree: |_pid, _pgid| {},
                get_pgid: |pid| Pgid::try_from(pid).ok(),
                // The port stays held no matter what we do.
                wait_port_closed: |_port, _timeout| false,
            },
            record_dir: Some(dir.path().to_path_buf()),
            // These two tests are about record *lifetime*, not ownership —
            // the record they plant genuinely describes this process, so the
            // iter-191 identity gate is stubbed open to keep them focused.
            record_pid_is_ours: |_rec| true,
        };
        let cli = <Cli as clap::Parser>::try_parse_from(["ff-rdp", "launch"]).expect("parse cli");

        // --- stop_prior_instance (launch --replace's path) ---
        crate::daemon_record::write_in(dir.path(), &rec).expect("write record");
        let err = stop_prior_instance_with(&cli, port, &deps)
            .expect_err("a port that stays held must fail the stop");
        assert!(matches!(err, AppError::User(_)), "expected a user error");
        assert!(
            crate::daemon_record::read_in(dir.path(), port)
                .expect("read record")
                .is_some(),
            "stop_prior_instance must keep the DaemonRecord when the stop failed"
        );

        // --- stop_daemon_and_build_result (daemon stop's path) ---
        crate::daemon_record::write_in(dir.path(), &rec).expect("rewrite record");
        let err = stop_daemon_and_build_result_with(&cli, port, &deps)
            .expect_err("a port that stays held must fail the stop");
        assert!(matches!(err, AppError::User(_)), "expected a user error");
        assert!(
            crate::daemon_record::read_in(dir.path(), port)
                .expect("read record")
                .is_some(),
            "stop_daemon_and_build_result must keep the DaemonRecord when the stop failed"
        );
    }

    /// A *successful* stop still removes the record — the ordering fix must
    /// not turn into a leak.
    #[test]
    fn unit_158_record_removed_after_successful_stop() {
        let dir = tempfile::tempdir().expect("tempdir");
        let port = 64_322u16;
        let rec = crate::daemon_record::DaemonRecord {
            pid: std::process::id(),
            port,
            headless: true,
            launched_at: chrono::Utc::now(),
            profile_dir: dir.path().join("profile"),
            start_token: None,
        };
        crate::daemon_record::write_in(dir.path(), &rec).expect("write record");

        let deps = StopDeps {
            hooks: EscalationHooks {
                is_alive: |_pid| true,
                kill_group_term: |_pid| {},
                kill_group_kill: |_pid| {},
                kill_process_tree: |_pid, _pgid| {},
                get_pgid: |pid| Pgid::try_from(pid).ok(),
                wait_port_closed: |_port, _timeout| true,
            },
            record_dir: Some(dir.path().to_path_buf()),
            // These two tests are about record *lifetime*, not ownership —
            // the record they plant genuinely describes this process, so the
            // iter-191 identity gate is stubbed open to keep them focused.
            record_pid_is_ours: |_rec| true,
        };
        let cli = <Cli as clap::Parser>::try_parse_from(["ff-rdp", "launch"]).expect("parse cli");

        let outcome = stop_prior_instance_with(&cli, port, &deps).expect("a freed port succeeds");
        assert_eq!(outcome.pid, Some(std::process::id()));
        assert!(
            crate::daemon_record::read_in(dir.path(), port)
                .expect("read record")
                .is_none(),
            "a successful stop must remove the DaemonRecord"
        );
    }

    /// AC `unit_158_single_stop_ladder_implementation`: the
    /// SIGTERM→wait→SIGKILL→poll sequence exists once, not four times.
    ///
    /// Pre-158 it was written out at `:901` (`kill_pid_and_wait_port`), `:1063`
    /// and `:1077` (both inside `stop_daemon_and_build_result`) and `:1219`
    /// (`stop_prior_instance`'s port-owner branch) — and only one of those
    /// could reach the tree-kill step at all, because its caller had already
    /// killed the pid.
    ///
    /// The plan's AC phrases this as "`process::kill_process_group(` appears in
    /// exactly one non-test function". The implemented ladder is stronger:
    /// there are **zero** open-coded calls, because every signal now goes
    /// through an [`EscalationHooks`] fn pointer, and `process::kill_process_group`
    /// is *named* exactly once — in `EscalationHooks::real`, the single feed
    /// into [`stop_pid_with_full_escalation`]. Both properties are asserted.
    #[test]
    fn unit_158_single_stop_ladder_implementation() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/daemon/client.rs");
        let src = std::fs::read_to_string(&path).expect("read client.rs");

        // Everything before the first `#[cfg(test)]` is the non-test source.
        let non_test = src
            .split_once("#[cfg(test)]")
            .map_or(src.as_str(), |(before, _)| before);

        let code_lines = || {
            non_test.lines().filter(|line| {
                let t = line.trim_start();
                !t.starts_with("//")
            })
        };

        let open_coded: Vec<&str> = code_lines()
            .filter(|line| line.contains("kill_process_group("))
            .collect();
        assert!(
            open_coded.is_empty(),
            "no non-test code may call `process::kill_process_group(` directly — every signal \
             goes through `stop_pid_with_full_escalation`'s hooks; found: {open_coded:?}"
        );

        // `process::kill_process_group_force` is a different helper (SIGKILL);
        // match the SIGTERM one exactly.
        let mentions: Vec<&str> = code_lines()
            .filter(|line| line.contains("process::kill_process_group,"))
            .collect();
        assert_eq!(
            mentions.len(),
            1,
            "`process::kill_process_group` must be named exactly once outside tests (the \
             `EscalationHooks::real()` wiring feeding `stop_pid_with_full_escalation`); \
             found: {mentions:?}"
        );
        assert!(
            mentions[0].contains("kill_group_term: process::kill_process_group"),
            "the single occurrence must be the hook wiring: {:?}",
            mentions[0]
        );

        // And the ladder itself exists exactly once.
        let ladder_defs = code_lines()
            .filter(|line| line.contains("fn stop_pid_with_full_escalation"))
            .count();
        assert_eq!(ladder_defs, 1, "there must be exactly one stop ladder");
    }

    /// AC `unit_153_no_nested_envelope_prints`: `run_daemon_stop` prints its
    /// own top-level JSON envelope (`OutputPipeline::finalize`) — so calling
    /// it from *inside* another command's run corrupts that command's stdout
    /// with a second document back to back. That is exactly what happened
    /// pre-iter-153: `stop_prior_instance` (part of `launch`'s own run) called
    /// `run_daemon_stop` from its registry-path branch.
    ///
    /// Guard the fix by asserting `run_daemon_stop(` is invoked from exactly
    /// one call site anywhere in the crate's source — the standalone
    /// `DaemonCommand::Stop` dispatch arm, which IS the top-level `daemon
    /// stop` command and is therefore allowed to print. Any other call site
    /// (in `stop_prior_instance` or a future helper) would reintroduce the
    /// double-envelope defect.
    #[test]
    fn unit_153_no_nested_envelope_prints() {
        let src_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let allowed_file = src_dir.join("dispatch.rs");

        let mut offending_call_sites: Vec<String> = Vec::new();
        for entry in walkdir::WalkDir::new(&src_dir)
            .into_iter()
            .filter_map(Result::ok)
            .filter(|e| e.path().extension().is_some_and(|ext| ext == "rs"))
        {
            let path = entry.path();
            let Ok(contents) = std::fs::read_to_string(path) else {
                continue;
            };
            for (line_no, line) in contents.lines().enumerate() {
                let trimmed = line.trim_start();
                // Skip the function's own definition and doc/comment lines
                // that merely mention the name.
                if trimmed.starts_with("//") || trimmed.starts_with("///") {
                    continue;
                }
                if line.contains("fn run_daemon_stop") {
                    continue;
                }
                // Match the actual call shape (`run_daemon_stop(cli, …)`),
                // and require it NOT be immediately preceded by `"` — that
                // excludes this test's own source, which necessarily
                // mentions the string `"run_daemon_stop(cli"` in its
                // pattern-matching logic and doc comment, from flagging
                // itself as an offending call site.
                let Some(idx) = line.find("run_daemon_stop(cli") else {
                    continue;
                };
                if line[..idx].ends_with('"') {
                    continue;
                }
                if path == allowed_file {
                    // The one legitimate top-level caller: `daemon stop`'s
                    // own CLI dispatch arm.
                    continue;
                }
                offending_call_sites.push(format!(
                    "{}:{}: {}",
                    path.display(),
                    line_no + 1,
                    line.trim()
                ));
            }
        }

        assert!(
            offending_call_sites.is_empty(),
            "unit_153_no_nested_envelope_prints: FAIL — run_daemon_stop (which prints its own \
             top-level JSON envelope) is called from somewhere other than dispatch.rs's \
             top-level `daemon stop` handler. Any such call happens from inside another \
             command's run and corrupts its stdout with a second envelope (the iter-153 \
             defect). Offending call site(s):\n{}",
            offending_call_sites.join("\n")
        );
    }

    #[test]
    fn no_daemon_flag_always_returns_direct() {
        let target = resolve_connection_target("localhost", 6000, 300, true);
        // --no-daemon should never carry a deferred warning — there is no
        // daemon-startup attempt to report on.
        match target {
            ConnectionTarget::Direct { deferred_warning } => {
                assert!(
                    deferred_warning.is_none(),
                    "no-daemon path should not carry a deferred warning, got: {deferred_warning:?}"
                );
            }
            ConnectionTarget::Daemon { .. } => panic!("--no-daemon must never resolve to Daemon"),
        }
    }

    // ── iter-164 defect 2: autostart gave up too early under load ──────────

    /// The default autostart budget must be materially larger than the 5 s that
    /// iter-158's sweep (load average 18.6) exceeded, or the fix is cosmetic.
    #[test]
    fn unit_164_registry_wait_default_exceeds_the_load_failure_point() {
        assert_eq!(parse_registry_wait(None), Duration::from_secs(20));
        assert!(
            parse_registry_wait(None) > Duration::from_secs(5),
            "5 s is the budget that failed under load; the default must exceed it"
        );
    }

    /// A well-formed override wins; anything else falls back to the default
    /// rather than producing a zero-length (i.e. always-failing) wait.
    #[test]
    fn unit_164_registry_wait_override_parses_or_falls_back() {
        assert_eq!(
            parse_registry_wait(Some("1500")),
            Duration::from_millis(1500)
        );
        assert_eq!(
            parse_registry_wait(Some("  2500  ")),
            Duration::from_millis(2500)
        );
        for bad in ["", "0", "-1", "abc", "1.5"] {
            assert_eq!(
                parse_registry_wait(Some(bad)),
                Duration::from_secs(20),
                "malformed override {bad:?} must fall back to the default"
            );
        }
    }

    /// AC `unit_autostart_failure_surfaces_warning`: when auto-start does not
    /// yield a usable daemon and the CLI falls back to direct, a
    /// `daemon_autostart_failed` warning must be recorded (for the envelope)
    /// **and** the returned `Direct` must carry the deferred human-facing
    /// warning — never a hard error, since direct mode still works.
    #[test]
    fn unit_autostart_failure_surfaces_warning() {
        // Serialize against every other test that touches the process-global
        // warning slot (iter-123) so concurrent record→take sequences never
        // observe each other's writes; then clear residue for determinism.
        let _guard = crate::daemon_status::test_lock();
        let _ = crate::daemon_status::take_warnings();

        let target = direct_with_autostart_warning(
            "daemon started but did not register within 5s (spawn died before the registry write)"
                .to_owned(),
        );

        match target {
            ConnectionTarget::Direct { deferred_warning } => {
                assert!(
                    deferred_warning
                        .as_deref()
                        .is_some_and(|w| w.contains("did not register")),
                    "fallback must carry a deferred human-facing warning; got {deferred_warning:?}"
                );
            }
            ConnectionTarget::Daemon { .. } => {
                panic!("autostart failure must resolve to Direct, never Daemon")
            }
        }

        let warnings = crate::daemon_status::take_warnings();
        assert_eq!(warnings.len(), 1, "exactly one warning must be recorded");
        assert_eq!(
            warnings[0].warning_type,
            crate::daemon_status::AUTOSTART_FAILED_TYPE,
            "warning must be tagged daemon_autostart_failed"
        );
        assert!(
            warnings[0].reason.contains("spawn died"),
            "recorded reason must carry the diagnosed cause; got {:?}",
            warnings[0].reason
        );
    }

    /// The autostart warning surfaces as a top-level `warnings` array via the
    /// output pipeline recorder (`daemon_status::take_warnings_json`).
    #[test]
    fn autostart_warning_serializes_into_envelope_shape() {
        let _guard = crate::daemon_status::test_lock();
        let _ = crate::daemon_status::take_warnings();
        let _ = direct_with_autostart_warning("registry write raced or was slow".to_owned());
        let json = crate::daemon_status::take_warnings_json().expect("warnings present");
        let arr = json.as_array().expect("warnings is an array");
        assert_eq!(arr[0]["type"], crate::daemon_status::AUTOSTART_FAILED_TYPE);
        assert!(
            arr[0]["reason"]
                .as_str()
                .is_some_and(|r| r.contains("raced or was slow"))
        );
    }

    /// AC: `unit_daemon_stop_message_reports_actual_bound`
    ///
    /// The error message produced by `port_still_listening_msg` must reflect
    /// `PORT_FREE_WAIT_BOUND` (8 s), not any hardcoded literal. If the constant
    /// changes, the message stays in sync.
    #[test]
    fn unit_daemon_stop_message_reports_actual_bound() {
        let msg = port_still_listening_msg(12345, 6000);
        let expected_bound = format!("after {} s", PORT_FREE_WAIT_BOUND.as_secs());
        assert!(
            msg.contains(&expected_bound),
            "error message must contain '{expected_bound}' but got: {msg:?}"
        );
        // Regression guard: must not contain the old hardcoded value (3 s) if
        // PORT_FREE_WAIT_BOUND is anything other than 3.
        if PORT_FREE_WAIT_BOUND.as_secs() != 3 {
            assert!(
                !msg.contains("after 3 s"),
                "error message must not mention the old 3 s bound: {msg:?}"
            );
        }
        assert_eq!(PORT_FREE_WAIT_BOUND.as_secs(), 8, "bound should be 8 s");
    }

    /// AC: `pre_fix_repro_daemon_stop_waits_past_3s_for_slow_shutdown`
    ///
    /// Verifies that `wait_for_port_closed` with `PORT_FREE_WAIT_BOUND` (8 s)
    /// succeeds when the port takes >3 s but <8 s to free, and that a 3 s
    /// deadline would have failed. Uses a real TCP listener released from a
    /// background thread after 4 s — no subprocess needed.
    #[test]
    fn pre_fix_repro_daemon_stop_waits_past_3s_for_slow_shutdown() {
        use std::net::TcpListener;

        // Bind an ephemeral port and record which port the OS assigned.
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind failed");
        let port = listener.local_addr().unwrap().port();

        // Release the listener from a background thread after 4 s.
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_secs(4));
            drop(listener); // closes the socket
        });

        // A 3 s deadline must time out (pre-fix behaviour would return false here
        // and immediately error, even though the port frees at 4 s).
        let short_wait = super::super::process::wait_for_port_closed(port, Duration::from_secs(3));
        assert!(
            !short_wait,
            "3 s deadline should have timed out while the port is still held"
        );

        // The 8 s bound (PORT_FREE_WAIT_BOUND) must succeed.
        let long_wait = super::super::process::wait_for_port_closed(port, PORT_FREE_WAIT_BOUND);
        assert!(
            long_wait,
            "PORT_FREE_WAIT_BOUND ({} s) should succeed after the 4 s hold",
            PORT_FREE_WAIT_BOUND.as_secs()
        );
    }

    /// AC: `unit_daemon_stop_uses_killpg_when_kill_pid_fails`
    ///
    /// When pid-level SIGKILL leaves the port held, `run_escalation` must
    /// invoke the `kill_process_tree` hook (the pgid step). Uses injectable
    /// function-pointer hooks so no Firefox process is needed.
    #[test]
    fn unit_daemon_stop_uses_killpg_when_kill_pid_fails() {
        use std::net::TcpListener;
        use std::sync::atomic::{AtomicBool, Ordering};

        // Track whether kill_process_tree was called. Declared at the top of the
        // function body (before any statements) to satisfy clippy::items_after_statements.
        static TREE_KILL_CALLED: AtomicBool = AtomicBool::new(false);

        // Bind a real listener to simulate a port that stays held after pid kills.
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind failed");
        let port = listener.local_addr().unwrap().port();

        let hooks = EscalationHooks {
            // Process always looks alive so escalation proceeds.
            is_alive: |_pid| true,
            // SIGTERM and pid-level SIGKILL are no-ops (don't actually kill the listener).
            kill_group_term: |_pid| {},
            kill_group_kill: |_pid| {},
            // Tree kill: record that we were called, then drop the listener.
            kill_process_tree: |_pid, _pgid| {
                TREE_KILL_CALLED.store(true, Ordering::SeqCst);
                // We can't drop the listener here (it's in the outer scope),
                // so we just record the call; the port poll will time out and
                // return false — which is fine for this test's assertion.
            },
            // No real PGID capture needed.
            get_pgid: |_pid| None,
            // Short timeouts so the test completes quickly.
            wait_port_closed: |test_port, timeout| {
                // The listener stays held, so always return false within a short window.
                // We use 10 ms max to keep the test fast.
                let deadline = std::time::Instant::now() + timeout.min(Duration::from_millis(10));
                loop {
                    if std::net::TcpStream::connect(format!("127.0.0.1:{test_port}")).is_err() {
                        return true;
                    }
                    if std::time::Instant::now() >= deadline {
                        return false;
                    }
                    std::thread::sleep(Duration::from_millis(5));
                }
            },
            // Override the 1-second SIGTERM grace sleep via the wait hook above.
            // (The actual sleep(1) is hardcoded in run_escalation; we live with it
            // in prod. For the test we accepted the 1 s sleep — see comment below.)
        };

        // NOTE: `stop_pid_with_full_escalation` polls `is_alive` for up to 2 s
        // between SIGTERM and SIGKILL. With `is_alive` pinned to `true` this
        // test pays that 2 s. The port_closed hook is capped at 10 ms per call
        // to avoid the 8 s and 500 ms waits on top of it.
        let (_stopped, port_free, msg) =
            stop_pid_with_full_escalation(99999, Some(port), &hooks, None);

        // Port stays held (listener is still open) — escalation reports failure.
        assert!(!port_free, "port should still be held (listener is open)");
        // The pgid kill step must have been invoked.
        assert!(
            TREE_KILL_CALLED.load(Ordering::SeqCst),
            "kill_process_tree hook must be called when pid-level kill leaves port held"
        );
        // Error message must mention the platform's tree-kill escalation path
        // (`port_still_listening_after_escalation_msg` words it per platform).
        #[cfg(unix)]
        assert!(
            msg.contains("pgid"),
            "error message must mention 'pgid' escalation: {msg:?}"
        );
        #[cfg(not(unix))]
        assert!(
            msg.contains("taskkill"),
            "error message must mention 'taskkill' escalation: {msg:?}"
        );

        // Clean up the listener so the port is released.
        drop(listener);
    }

    /// AC: `pre_fix_repro_daemon_stop_kills_process_group_on_port_retention`
    ///
    /// Simplified fixture: spawn a child process in its own process group (via
    /// `process_group(0)`), capture its PGID, kill the child's "parent" (itself —
    /// we simulate by killing the child immediately and making a sibling hold the
    /// listener), then assert that `kill_process_tree(pgid)` frees the port.
    ///
    /// The simpler version: spawn `sleep 60` in a new process group on a fresh port.
    /// The child inherits a bound TcpListener via a background thread that accepts
    /// on the port. Then we kill the child (simulating "parent dies") and verify
    /// `kill_process_tree` reaps it via the PGID.
    ///
    /// Ignored by default — requires Unix process semantics.
    /// Run with: `FF_RDP_LIVE_TESTS=1 cargo test-live -p ff-rdp-cli -- pre_fix_repro_daemon_stop_kills`
    #[test]
    #[cfg(unix)]
    #[ignore = "requires Unix process-group semantics — run with FF_RDP_LIVE_TESTS=1"]
    fn pre_fix_repro_daemon_stop_kills_process_group_on_port_retention() {
        use std::net::TcpListener;
        use std::os::unix::process::CommandExt as _;

        // Pick a free port (bind/release races, but fine for a single-thread test).
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind failed");
        let port = listener.local_addr().unwrap().port();
        drop(listener);

        // Spawn a child in its own pgid that ACTUALLY HOLDS the port — so the
        // only way the port can free is by killing the child's process group.
        // We use a tiny Python one-liner because it's available on every dev
        // machine that runs the live test suite. If `python3` is missing the
        // test fails loudly rather than silently skipping.
        let py = format!(
            "import socket,time;\
             s=socket.socket(socket.AF_INET,socket.SOCK_STREAM);\
             s.setsockopt(socket.SOL_SOCKET,socket.SO_REUSEADDR,0);\
             s.bind(('127.0.0.1',{port}));s.listen(1);\
             print('ready',flush=True);time.sleep(60)"
        );
        let child = std::process::Command::new("python3")
            .arg("-c")
            .arg(&py)
            .stdout(std::process::Stdio::piped())
            .process_group(0) // new pgid = child's pid
            .spawn()
            .expect("failed to spawn python3 port holder");

        let child_pid = child.id();

        // Wait for the child to actually bind the port.
        let mut bound = false;
        for _ in 0..50 {
            if std::net::TcpStream::connect(format!("127.0.0.1:{port}")).is_ok() {
                bound = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        assert!(bound, "child failed to bind port {port} within 5 s");

        let pgid = process::get_process_group_id(child_pid)
            .expect("getpgid should succeed for a live child");
        assert!(pgid > 0, "getpgid should return a positive PGID");
        assert_eq!(
            i64::from(pgid),
            i64::from(child_pid),
            "child should be its own pgid leader"
        );

        // Call the helper under test. This is the ONLY thing freeing the port —
        // the test process holds no listener of its own.
        process::kill_process_tree(child_pid, Some(pgid));

        // Now assert the port is free without us touching anything.
        let port_free = process::wait_for_port_closed(port, Duration::from_secs(5));

        // Best-effort reap to avoid zombies (the kill above should have done it).
        let _ = std::process::Command::new("wait")
            .arg(child_pid.to_string())
            .status();
        drop(child);

        assert!(
            port_free,
            "pre_fix_repro: port {port} should be free after kill_process_tree(pgid={pgid}) \
             — only the child held it, so a freed port proves the pgid kill worked"
        );

        eprintln!(
            "pre_fix_repro_daemon_stop_kills_process_group_on_port_retention: PASS — \
             port {port} freed by kill_process_tree(pgid={pgid})"
        );
    }
}
