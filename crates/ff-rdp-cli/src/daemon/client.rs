use std::time::Duration;

use anyhow::{Context, Result};
use serde_json::{Value, json};

use ff_rdp_core::RdpTransport;

use super::process;
use super::registry::{self, DaemonInfo};

// ---------------------------------------------------------------------------
// Connection target resolution
// ---------------------------------------------------------------------------

/// The result of resolving how to connect: either via daemon or directly.
pub(crate) enum ConnectionTarget {
    /// Connect via daemon at this port on localhost.
    Daemon { port: u16 },
    /// Connect directly to Firefox.
    Direct,
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
    let Some(info) = registry::read_registry()? else {
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
        registry::remove_registry().ok();
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
        return ConnectionTarget::Direct;
    }

    // 1. Try to find an already-running daemon.
    match find_running_daemon(firefox_host, firefox_port) {
        Ok(Some(info)) => {
            return ConnectionTarget::Daemon {
                port: info.proxy_port,
            };
        }
        Ok(None) => {} // not running — fall through to spawn
        Err(e) => {
            eprintln!(
                "warning: failed to check daemon status: {e:#}{}",
                log_path_hint()
            );
            return ConnectionTarget::Direct;
        }
    }

    // 2. Determine the current executable path so we can re-invoke ourselves
    //    as a daemon.
    let exe_path = match std::env::current_exe() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("warning: cannot determine executable path: {e}, connecting directly");
            return ConnectionTarget::Direct;
        }
    };

    // 3. Spawn the daemon.
    if let Err(e) =
        process::spawn_daemon(&exe_path, firefox_host, firefox_port, daemon_timeout_secs)
    {
        eprintln!(
            "warning: failed to start daemon: {e:#}, connecting directly{}",
            log_path_hint()
        );
        return ConnectionTarget::Direct;
    }

    // 4. Wait for the daemon to write its registry entry.
    match process::wait_for_registry(Duration::from_secs(5), firefox_host, firefox_port) {
        Ok(info) => ConnectionTarget::Daemon {
            port: info.proxy_port,
        },
        Err(e) => {
            eprintln!(
                "warning: daemon started but registry not found: {e:#}, connecting directly{}",
                log_path_hint()
            );
            ConnectionTarget::Direct
        }
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
pub(crate) fn drain_daemon_events(
    transport: &mut RdpTransport,
    resource_type: &str,
) -> Result<Vec<Value>> {
    let msg = json!({
        "to": "daemon",
        "type": "drain",
        "resourceType": resource_type,
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
            return Ok(events);
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

/// Format a hint pointing to the daemon log file, or an empty string if
/// the path cannot be determined.
fn log_path_hint() -> String {
    match super::registry::log_path() {
        Ok(p) => format!(" (check {} for details)", p.display()),
        Err(_) => String::new(),
    }
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
        assert!(matches!(target, ConnectionTarget::Direct));
    }
}
