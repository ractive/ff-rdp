use std::net::TcpStream;
use std::time::Duration;

use anyhow::{Context, Result};
use serde_json::{Value, json};

use ff_rdp_core::{FramedReader, FramedWriter, RdpTransport};

use super::process;
use super::registry::{self, DaemonInfo};
use crate::cli::args::Cli;
use crate::error::AppError;
use crate::output;
use crate::output_pipeline::OutputPipeline;

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
        return ConnectionTarget::Direct {
            deferred_warning: None,
        };
    }

    // 1. Try to find an already-running daemon.
    match find_running_daemon(firefox_host, firefox_port) {
        Ok(Some(info)) => {
            return ConnectionTarget::Daemon {
                port: info.proxy_port,
                auth_token: info.auth_token,
            };
        }
        Ok(None) => {} // not running — fall through to spawn
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
    //
    //     This probe is performed only when there is no running daemon
    //     (`Ok(None)` above), so we would otherwise try to spawn one.
    //     It is skipped for registry errors (already returned above) and when
    //     a daemon is already running (already returned above).
    if !is_firefox_port_open(firefox_host, firefox_port) {
        return ConnectionTarget::Direct {
            deferred_warning: None,
        };
    }

    // 2. Determine the current executable path so we can re-invoke ourselves
    //    as a daemon.
    let exe_path = match std::env::current_exe() {
        Ok(p) => p,
        Err(e) => {
            return ConnectionTarget::Direct {
                deferred_warning: Some(format!(
                    "warning: cannot determine executable path: {e}, connecting directly"
                )),
            };
        }
    };

    // 3. Spawn the daemon.
    if let Err(e) =
        process::spawn_daemon(&exe_path, firefox_host, firefox_port, daemon_timeout_secs)
    {
        return ConnectionTarget::Direct {
            deferred_warning: Some(format!(
                "warning: failed to start daemon: {e:#}, connecting directly{}",
                log_path_hint()
            )),
        };
    }

    // 4. Wait for the daemon to write its registry entry.
    match process::wait_for_registry(Duration::from_secs(5), firefox_host, firefox_port) {
        Ok(info) => ConnectionTarget::Daemon {
            port: info.proxy_port,
            auth_token: info.auth_token,
        },
        Err(e) => ConnectionTarget::Direct {
            // Deferred so it is silent on the happy path: in the common case
            // the registry-write race resolves before we'd care, the direct
            // connection succeeds, and the warning is dropped.
            deferred_warning: Some(format!(
                "warning: daemon started but registry not found: {e:#}, connecting directly{}",
                log_path_hint()
            )),
        },
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
// A5: Fast-fail Firefox port probe
// ---------------------------------------------------------------------------

/// Return `true` if `host:port` accepts a TCP connection within 100 ms.
///
/// Used as a quick pre-spawn check: if Firefox's debug port is dark there is
/// no point spawning a daemon (which would wait up to 5 s to time out).
fn is_firefox_port_open(host: &str, port: u16) -> bool {
    let addr_str = format!("{host}:{port}");
    let Ok(addr) = addr_str.parse::<std::net::SocketAddr>() else {
        // Fallback: unresolved hostname — do not block, allow normal path.
        return true;
    };
    TcpStream::connect_timeout(&addr, Duration::from_millis(100)).is_ok()
}

// ---------------------------------------------------------------------------
// A4: daemon status / stop CLI handlers
// ---------------------------------------------------------------------------

/// Connect to the daemon (after auth), send a raw daemon message, and return
/// the daemon's response.
///
/// On failure (daemon not found, auth error, etc.) returns an `AppError`.
fn daemon_rpc(cli: &Cli, msg: &serde_json::Value) -> Result<Value, AppError> {
    let info = registry::read_registry()
        .map_err(|e| AppError::Internal(anyhow::anyhow!("reading daemon registry: {e}")))?
        .ok_or_else(|| AppError::User("no daemon is running".to_owned()))?;

    if !process::is_process_alive(info.pid) {
        registry::remove_registry().ok();
        return Err(AppError::User(
            "daemon process is no longer alive".to_owned(),
        ));
    }

    let addr = format!("127.0.0.1:{}", info.proxy_port)
        .parse::<std::net::SocketAddr>()
        .map_err(|e| AppError::Internal(anyhow::anyhow!("parsing daemon addr: {e}")))?;

    let timeout = Duration::from_millis(cli.timeout);
    let stream = TcpStream::connect_timeout(&addr, timeout)
        .map_err(|e| AppError::User(format!("could not connect to daemon: {e}")))?;
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

    // Read the daemon's response, skipping any forwarded Firefox frames.
    for _ in 0..64 {
        let response = reader
            .recv()
            .map_err(|e| AppError::Internal(anyhow::anyhow!("receiving daemon response: {e}")))?;
        if response.get("from").and_then(Value::as_str) == Some("daemon") {
            if let Some(err) = response.get("error").and_then(Value::as_str) {
                return Err(AppError::User(format!("daemon error: {err}")));
            }
            return Ok(response);
        }
    }
    Err(AppError::Internal(anyhow::anyhow!(
        "did not receive daemon response within 64 frames"
    )))
}

/// `ff-rdp daemon status` — print daemon status as JSON.
pub(crate) fn run_daemon_status(cli: &Cli) -> Result<(), AppError> {
    let running;
    let result = match registry::read_registry()
        .map_err(|e| AppError::Internal(anyhow::anyhow!("reading daemon registry: {e}")))?
    {
        None => {
            running = false;
            json!({
                "running": false,
                "pid": null,
                "port": null,
                "uptime_seconds": null,
                "connections": null,
                "firefox_connected": null,
            })
        }
        Some(ref info) if !process::is_process_alive(info.pid) => {
            registry::remove_registry().ok();
            running = false;
            json!({
                "running": false,
                "pid": null,
                "port": null,
                "uptime_seconds": null,
                "connections": null,
                "firefox_connected": null,
            })
        }
        Some(ref info) => {
            running = true;
            // Try to get live stats from the daemon.
            let uptime_seconds = match daemon_rpc(cli, &json!({"to": "daemon", "type": "status"})) {
                Ok(resp) => resp.get("uptime_secs").and_then(Value::as_u64),
                Err(_) => None,
            };
            json!({
                "running": true,
                "pid": info.pid,
                "port": info.proxy_port,
                "uptime_seconds": uptime_seconds,
                "connections": null,
                "firefox_connected": true,
            })
        }
    };

    let _ = running; // used implicitly via result
    let meta = json!({
        "host": cli.host,
        "port": cli.port,
    });
    let envelope = output::envelope(&result, 1, &meta);
    Ok(OutputPipeline::from_cli(cli)?.finalize(&envelope)?)
}

/// `ff-rdp daemon stop` — gracefully stop the running daemon.
pub(crate) fn run_daemon_stop(cli: &Cli) -> Result<(), AppError> {
    // Read registry to get PID for fallback SIGTERM.
    let Some(info) = registry::read_registry()
        .map_err(|e| AppError::Internal(anyhow::anyhow!("reading daemon registry: {e}")))?
    else {
        // No daemon running — report success (idempotent).
        let meta = json!({"host": cli.host, "port": cli.port});
        let envelope = output::envelope(
            &json!({"stopped": false, "reason": "not running"}),
            1,
            &meta,
        );
        return Ok(OutputPipeline::from_cli(cli)?.finalize(&envelope)?);
    };

    if !process::is_process_alive(info.pid) {
        registry::remove_registry().ok();
        let meta = json!({"host": cli.host, "port": cli.port});
        let envelope = output::envelope(
            &json!({"stopped": true, "reason": "already dead"}),
            1,
            &meta,
        );
        return Ok(OutputPipeline::from_cli(cli)?.finalize(&envelope)?);
    }

    // Try graceful shutdown via RPC first.
    let rpc_ok = daemon_rpc(cli, &json!({"to": "daemon", "type": "shutdown"})).is_ok();

    if rpc_ok {
        // Give the daemon up to 2 seconds to exit cleanly.
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

    // If still alive, send SIGTERM as fallback.
    if process::is_process_alive(info.pid) {
        process::kill_process(info.pid);
        std::thread::sleep(Duration::from_millis(500));
    }

    // Clean up registry.
    registry::remove_registry().ok();

    let stopped = !process::is_process_alive(info.pid);
    let meta = json!({"host": cli.host, "port": cli.port});
    let envelope = output::envelope(&json!({"stopped": stopped}), 1, &meta);
    Ok(OutputPipeline::from_cli(cli)?.finalize(&envelope)?)
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
}
