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
            eprintln!("warning: failed to check daemon status: {e:#}");
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
        eprintln!("warning: failed to start daemon: {e:#}, connecting directly");
        return ConnectionTarget::Direct;
    }

    // 4. Wait for the daemon to write its registry entry.
    match process::wait_for_registry(Duration::from_secs(5)) {
        Ok(info) => ConnectionTarget::Daemon {
            port: info.proxy_port,
        },
        Err(e) => {
            eprintln!("warning: daemon started but registry not found: {e:#}, connecting directly");
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

    let response = transport
        .recv()
        .context("receiving drain response from daemon")?;

    let events = response
        .get("events")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    Ok(events)
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
