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

/// Core escalation logic, injectable for testing.
///
/// Escalation sequence:
/// 1. Capture the PGID **before** the escalation starts so it survives the
///    parent's death.
/// 2. Wait up to `PORT_FREE_WAIT_BOUND` for the port to free.
/// 3. On timeout: SIGTERM the process group, wait 1 s grace.
/// 4. SIGKILL the process group (pid-level).
/// 5. Re-poll for ~500 ms.
/// 6. If still held: SIGKILL the **captured** PGID (kills every child even if
///    the parent has already exited and the PGID assumption on step 4 broke).
/// 7. Re-poll for ~500 ms.
///
/// Returns `(port_free, error_message)`.
pub(crate) fn run_escalation(pid: u32, port: u16, h: &EscalationHooks) -> (bool, String) {
    // Capture PGID up-front. This is intentionally done before any kill so
    // the value is reliable even when the parent exits mid-escalation.
    let captured_pgid = (h.get_pgid)(pid);

    if (h.wait_port_closed)(port, PORT_FREE_WAIT_BOUND) {
        return (true, String::new());
    }

    // Bound elapsed — escalate, but only if the original PID is still alive.
    // PIDs can be recycled by the OS; signaling a stale PGID risks killing an
    // unrelated process group.
    if !(h.is_alive)(pid) {
        return (false, port_still_listening_msg(pid, port));
    }

    // Step 3: SIGTERM the process group.
    (h.kill_group_term)(pid);
    std::thread::sleep(Duration::from_secs(1));

    // Step 4: SIGKILL the process group (assumes pid==pgid).
    (h.kill_group_kill)(pid);
    if (h.wait_port_closed)(port, Duration::from_millis(500)) {
        return (true, String::new());
    }

    // Step 6: The pid-level kill wasn't sufficient — use the pre-captured
    // PGID to reach any child processes that may have survived (e.g. because
    // the parent exited before the kill was delivered, breaking the pid==pgid
    // assumption). On Windows this sends `taskkill /F /T /PID <pid>`.
    //
    // Safety guard: only fire the pgid kill when the captured pgid is the
    // SAME as the target pid. If Firefox wasn't spawned in its own process
    // group (older `launch` builds, or a user-supplied wrapper), pgid will
    // point at whatever group launched ff-rdp — usually the caller's
    // interactive shell. Killing that group would blast back up the chain
    // and is never what the user wants. Newer `launch` puts Firefox into a
    // pgid==pid group, so this guard passes on the happy path. On Windows
    // `captured_pgid` is `None` and `kill_process_tree` falls through to
    // `taskkill /F /T /PID`, which is already scoped to the pid subtree.
    let pgid_safe_to_kill = match captured_pgid {
        Some(group_id) => i64::from(group_id) == i64::from(pid),
        None => true, // Windows path is pid-scoped, no group risk.
    };
    if pgid_safe_to_kill {
        (h.kill_process_tree)(pid, captured_pgid);
    }
    if (h.wait_port_closed)(port, Duration::from_millis(500)) {
        return (true, String::new());
    }

    (
        false,
        port_still_listening_after_escalation_msg(pid, port, pgid_safe_to_kill),
    )
}

/// Wait for `port` to become free, with SIGTERM+SIGKILL escalation on timeout.
///
/// Returns `true` if the port is free; `false` (with an error message) if it
/// remains in use after the full escalation sequence.
///
/// Escalation:
/// 1. Capture PGID up-front (before the parent can die).
/// 2. Wait up to `PORT_FREE_WAIT_BOUND` for the port to free.
/// 3. On timeout: SIGTERM the process group, wait 1 s grace.
/// 4. SIGKILL the process group (pid-level).
/// 5. Re-poll for ~500 ms.
/// 6. SIGKILL the captured PGID (kill_process_tree — reaches surviving children).
/// 7. Re-poll for ~500 ms.
///
/// Returns `(port_free, error_message)`.
fn wait_port_free_with_escalation(pid: u32, port: u16) -> (bool, String) {
    run_escalation(pid, port, &EscalationHooks::real())
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
    match process::wait_for_registry(Duration::from_secs(5), firefox_host, firefox_port) {
        Ok(info) => ConnectionTarget::Daemon {
            port: info.proxy_port,
            auth_token: info.auth_token,
        },
        Err(e) => {
            let cause = classify_registry_wait_failure(firefox_host, firefox_port);
            direct_with_autostart_warning(format!(
                "daemon started but did not register within 5s ({cause}): {e:#} — \
                 connecting directly{}",
                log_path_hint()
            ))
        }
    }
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

/// Store pre-collected network events into the daemon buffer (iter-61j G).
///
/// Called by `navigate --with-network` after streaming completes so that
/// subsequent `ff-rdp network` calls can read the captured events from the
/// daemon buffer instead of falling back to the Performance API.
///
/// `nav_url` is the URL that was navigated to.  The daemon records a navigation
/// boundary **atomically with** the inserts (iter-106 Theme D), so the stored
/// batch is always visible under the default `--since -1` scope regardless of
/// how the reader loop's asynchronous `tabNavigated` boundary for the same
/// navigation interleaves with this call.  The earlier "do not pass a nav_url"
/// guidance produced exactly the cross-invocation "empty results" bug this
/// boundary fixes: the reader-loop boundary could land *after* the inserts and
/// scope `--since -1` past every stored event.
pub(crate) fn store_network_events(
    transport: &mut RdpTransport,
    nav_url: &str,
    events: &[serde_json::Value],
) -> Result<()> {
    let msg = json!({
        "to": "daemon",
        "type": "store-events",
        "resourceType": "network-event",
        "navUrl": nav_url,
        "events": events,
    });
    transport
        .send(&msg)
        .context("sending store-events to daemon")?;
    // Drain up to 64 frames to skip any in-flight push events before the ack.
    for _ in 0..64 {
        let resp = transport
            .recv()
            .context("receiving store-events response")?;
        if resp.get("from").and_then(serde_json::Value::as_str) == Some("daemon") {
            if let Some(err) = resp.get("error").and_then(serde_json::Value::as_str) {
                anyhow::bail!("daemon store-events error: {err}");
            }
            return Ok(());
        }
    }
    anyhow::bail!("did not receive store-events response within 64 frames")
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
            let (uptime_seconds, connections, buffer_sizes, target_count) =
                match daemon_rpc(cli, cli.port, &json!({"to": "daemon", "type": "status"})) {
                    Ok(resp) => (
                        resp.get("uptime_secs").and_then(Value::as_u64),
                        resp.get("stream_subscriber_count").and_then(Value::as_u64),
                        resp.get("buffer_sizes").cloned(),
                        resp.get("target_count").and_then(Value::as_u64),
                    ),
                    Err(_) => (None, None, None, None),
                };
            json!({
                "running": true,
                "pid": info.pid,
                "port": info.proxy_port,
                "uptime_seconds": uptime_seconds,
                "connections": connections,
                "buffer_sizes": buffer_sizes,
                "target_count": target_count,
            })
        }
    };

    let meta = json!({});
    let envelope = output::envelope(&result, 1, &meta);
    Ok(OutputPipeline::from_cli(cli)?.finalize(&envelope)?)
}

/// Stop a Firefox instance identified by PID and port.
///
/// Shared logic used by both `run_daemon_stop` (when a [`DaemonRecord`] is
/// present) and the legacy daemon-registry stop path.
///
/// Stop sequence:
/// 1. SIGTERM the Firefox process group.
/// 2. Wait up to 2 s for graceful exit.
/// 3. SIGKILL if still alive.
/// 4. Poll the port until free (max 3 s).
///
/// Returns `(stopped, port_free, escalation_msg)`. `escalation_msg` is non-empty
/// only when `port_free` is false — it explains the SIGTERM+SIGKILL escalation
/// path the wait took before giving up, and should be surfaced in the user-facing
/// error rather than a generic "port still listening" message.
fn kill_pid_and_wait_port(pid: u32, port: u16) -> (bool, bool, String) {
    process::kill_process_group(pid);
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    while process::is_process_alive(pid) && std::time::Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(100));
    }
    if process::is_process_alive(pid) {
        process::kill_process_group_force(pid);
        std::thread::sleep(Duration::from_millis(300));
    }
    let (port_free, escalation_msg) = wait_port_free_with_escalation(pid, port);
    // Recompute `stopped` AFTER the escalation step — `wait_port_free_with_escalation`
    // may send SIGTERM/SIGKILL on bound timeout, so the process can transition
    // from alive to dead inside that call.
    let stopped = !process::is_process_alive(pid);
    (stopped, port_free, escalation_msg)
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
    // ----------------------------------------------------------------
    // 1. Check the shared DaemonRecord (written by `launch`).
    //    Only act on records whose `port` matches the target `port` so a
    //    stray `daemon stop` cannot kill an unrelated instance the user did
    //    not address. `read()` already filters out stale (dead-PID)
    //    records, so we don't need to recheck liveness here.
    // ----------------------------------------------------------------
    match crate::daemon_record::read()
        .map_err(|e| AppError::Internal(anyhow::anyhow!("reading daemon record: {e}")))?
    {
        Some(rec) if rec.port == port => {
            // Live instance found via DaemonRecord and matches --port — kill it.
            let (stopped, port_free, escalation_msg) = kill_pid_and_wait_port(rec.pid, rec.port);
            crate::daemon_record::remove().ok();

            if !port_free {
                let msg = if escalation_msg.is_empty() {
                    port_still_listening_msg(rec.pid, rec.port)
                } else {
                    escalation_msg
                };
                return Err(AppError::User(msg));
            }

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

            let meta = json!({});
            let envelope = output::envelope(
                &json!({
                    "stopped": stopped,
                    "pid": rec.pid,
                    "port": rec.port,
                    "profile_removed": profile_removed_path.is_some(),
                    "profile_removed_path": profile_removed_path
                        .as_ref()
                        .map(|p| p.to_string_lossy().into_owned()),
                }),
                1,
                &meta,
            );
            return Ok(OutputPipeline::from_cli(cli)?.finalize(&envelope)?);
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
        let meta = json!({});
        let envelope = output::envelope(
            &json!({"stopped": false, "reason": "not running"}),
            1,
            &meta,
        );
        return Ok(OutputPipeline::from_cli(cli)?.finalize(&envelope)?);
    };

    let firefox_port = info.firefox_port;

    if !process::is_process_alive(info.pid) {
        registry::remove_registry(firefox_port).ok();
        let meta = json!({});
        let envelope = output::envelope(
            &json!({"stopped": true, "reason": "already dead"}),
            1,
            &meta,
        );
        return Ok(OutputPipeline::from_cli(cli)?.finalize(&envelope)?);
    }

    // 1. Try graceful shutdown via RPC first.
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

    // 2. If still alive, SIGTERM the Firefox process group (not just the daemon PID).
    //    Firefox spawns GPU/RDD child processes in the same group; killing only the
    //    daemon leaves those children alive and holding the port open.
    if process::is_process_alive(info.pid) {
        process::kill_process_group(info.pid);
        // Wait up to 2 s for the process group to exit.
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while process::is_process_alive(info.pid) && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    // 3. If SIGTERM was not enough, SIGKILL the group as a last resort.
    if process::is_process_alive(info.pid) {
        process::kill_process_group_force(info.pid);
        std::thread::sleep(Duration::from_millis(300));
    }

    // 4. Clean up the daemon registry regardless of process state.
    registry::remove_registry(firefox_port).ok();

    // 5. Poll the Firefox debug port until it stops accepting connections
    //    (max PORT_FREE_WAIT_BOUND with SIGTERM+SIGKILL escalation on timeout).
    //    This confirms that the OS has reclaimed the socket, so a subsequent
    //    `launch` on the same port will succeed immediately without a "port in use" error.
    let (port_free, escalation_msg) = wait_port_free_with_escalation(info.pid, firefox_port);

    if !port_free {
        return Err(AppError::User(escalation_msg));
    }

    let stopped = !process::is_process_alive(info.pid);
    let meta = json!({});
    let envelope = output::envelope(&json!({"stopped": stopped}), 1, &meta);
    Ok(OutputPipeline::from_cli(cli)?.finalize(&envelope)?)
}

/// Stop an existing Firefox instance on `port` to make way for a fresh launch.
///
/// Used by `launch --replace` / `launch --force` (iter-86 Theme A / iter-90).
/// Returns `Ok(())` if the port is free afterwards, `Err` if it is still in use.
///
/// Stop priority (iter-90):
/// 1. DaemonRecord matching the requested port → kill, wait, remove record.
/// 2. Proxy-daemon registry matching the port → graceful `daemon stop` RPC path.
/// 3. Fall back to port-owner lookup.
pub(crate) fn stop_prior_instance(cli: &Cli, port: u16) -> Result<(), AppError> {
    // 1. Check shared DaemonRecord first (covers instances started via `launch`).
    match crate::daemon_record::read()
        .map_err(|e| AppError::Internal(anyhow::anyhow!("reading daemon record: {e}")))?
    {
        Some(rec) if rec.port == port && process::is_process_alive(rec.pid) => {
            let (_stopped, port_free, _escalation_msg) = kill_pid_and_wait_port(rec.pid, rec.port);
            crate::daemon_record::remove().ok();
            if !port_free {
                return Err(AppError::User(format!(
                    "port {port} is still in use after stopping the prior instance (pid {}). \
                     Run `ff-rdp doctor` or `lsof -i :{port}` to investigate.",
                    rec.pid
                )));
            }
            return Ok(());
        }
        Some(rec) if rec.port == port => {
            // Record exists but PID is dead — clean up and proceed (port may already be free).
            crate::daemon_record::remove().ok();
        }
        _ => {}
    }

    // 2. Proxy-daemon registry — use the graceful stop path.
    //    Keyed by the target `port` (iter-123 Theme B) so we only take the
    //    graceful path when a daemon record actually exists for this port.
    match registry::read_registry(port) {
        Ok(Some(ref info)) if info.firefox_port == port => {
            run_daemon_stop(cli, port)?;
            return Ok(());
        }
        _ => {}
    }

    // 3. No registry — try to kill whatever is on the port by PID from
    //    the port-owner helper, then wait for the port to free.
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
        // iter-100 Theme D: re-verify port ownership immediately before the
        // kill.  `find_listener` resolves a PID at time T; between T and the
        // signal the original process may have exited and the OS may have
        // recycled that PID onto an unrelated process.  Re-query the port
        // owner right before signalling and only proceed if the SAME pid still
        // owns the port — otherwise we would blindly SIGKILL a recycled PID.
        let still_owner = matches!(
            crate::port_owner::find_listener(port),
            Ok(Some(ref current)) if current.pid == owner.pid
        );
        if still_owner {
            process::kill_process_group(owner.pid);
            let deadline = std::time::Instant::now() + Duration::from_secs(2);
            while process::is_process_alive(owner.pid) && std::time::Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(100));
            }
            // Re-verify ownership once more before escalating to SIGKILL — the
            // SIGTERM grace may have freed the port (and the PID could now be
            // recycled), in which case a force-kill would target the wrong
            // process.
            let still_owner_after_term = matches!(
                crate::port_owner::find_listener(port),
                Ok(Some(ref current)) if current.pid == owner.pid
            );
            if still_owner_after_term && process::is_process_alive(owner.pid) {
                process::kill_process_group_force(owner.pid);
                std::thread::sleep(Duration::from_millis(300));
            }
        }
    }

    // Poll until free — signals were already sent above, so just wait.
    if !process::wait_for_port_closed(port, PORT_FREE_WAIT_BOUND) {
        return Err(AppError::User(format!(
            "port {port} is still in use after stopping the prior instance. \
             Run `ff-rdp doctor` or `lsof -i :{port}` to investigate."
        )));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

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

        // NOTE: `run_escalation` has a hardcoded `sleep(1)` between SIGTERM and
        // SIGKILL. This test still sleeps for that 1 s. The port_closed hook is
        // capped at 10 ms per call to avoid the 8 s and 500 ms waits on top of it.
        let (port_free, msg) = run_escalation(99999, port, &hooks);

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
