use std::time::{Duration, Instant};

use ff_rdp_core::{ProtocolError, TabActor, WatcherActor, WindowGlobalTarget};
use serde_json::json;

use crate::cli::args::Cli;
use crate::error::AppError;
use crate::hints::{HintContext, HintSource};
use crate::output;
use crate::output_pipeline::OutputPipeline;

use super::connect_tab::connect_and_get_target;

/// Which navigation action to perform.
#[derive(Clone, Copy)]
pub enum NavAction {
    /// Reload the current page. `force = true` bypasses the HTTP cache.
    Reload {
        force: bool,
    },
    Back,
    Forward,
}

pub fn run(cli: &Cli, action: NavAction) -> Result<(), AppError> {
    let mut ctx = connect_and_get_target(cli)?;
    let target_actor = ctx.target.actor.clone();

    let (action_name, force_reload) = match action {
        NavAction::Reload { force } => {
            WindowGlobalTarget::reload(ctx.transport_mut(), &target_actor, force)
                .map_err(AppError::from)?;
            ("reload", force)
        }
        NavAction::Back => {
            WindowGlobalTarget::go_back(ctx.transport_mut(), &target_actor)
                .map_err(AppError::from)?;
            ("back", false)
        }
        NavAction::Forward => {
            WindowGlobalTarget::go_forward(ctx.transport_mut(), &target_actor)
                .map_err(AppError::from)?;
            ("forward", false)
        }
    };

    let result = if force_reload {
        json!({"action": action_name, "force": true})
    } else {
        json!({"action": action_name})
    };
    let mut meta = json!({});
    crate::connection_meta::merge_into_if_verbose(
        &mut meta,
        &cli.host,
        cli.port,
        None,
        cli.is_verbose(),
    );
    let envelope = output::envelope(&result, 1, &meta);

    let hint_source = match action {
        NavAction::Reload { .. } => HintSource::Reload,
        NavAction::Back => HintSource::Back,
        NavAction::Forward => HintSource::Forward,
    };
    let hint_ctx = HintContext::new(hint_source);
    OutputPipeline::from_cli(cli)?
        .finalize_with_hints(&envelope, Some(&hint_ctx))
        .map_err(AppError::from)
}

/// Reload the page and wait until network activity has been idle for `idle_ms`
/// or the total wall-clock time exceeds `timeout_ms`.
///
/// ## Protocol flow
///
/// **Daemon mode** (default): uses the daemon's streaming API so network events
/// are forwarded directly to this client instead of being buffered.
///
/// **Direct mode**: subscribes to the watcher's `"network-event"` resource type
/// and drains events from the raw transport.
///
/// Both paths:
/// 1. Set up network event capture.
/// 2. Send the `reload` request.
/// 3. Drain events until idle or timeout.
/// 4. Emit `{reloaded: true, idle_at_ms: N, requests_observed: M}`.
pub fn run_reload_wait_idle(
    cli: &Cli,
    idle_ms: u64,
    timeout_ms: u64,
    force: bool,
) -> Result<(), AppError> {
    let mut ctx = connect_and_get_target(cli)?;
    let target_actor = ctx.target.actor.clone();

    if ctx.via_daemon {
        return run_reload_wait_idle_daemon(
            &mut ctx,
            cli,
            &target_actor,
            idle_ms,
            timeout_ms,
            force,
        );
    }

    run_reload_wait_idle_direct(&mut ctx, cli, &target_actor, idle_ms, timeout_ms, force)
}

/// Reload + wait-idle through the daemon proxy.
///
/// The daemon intercepts watcher events and buffers them by default, so the
/// direct `watch_resources` approach never delivers events to this client.
/// Instead we use `start_daemon_stream` / `stop_daemon_stream_draining` to
/// receive events in real-time (same pattern as `navigate --with-network`).
fn run_reload_wait_idle_daemon(
    ctx: &mut super::connect_tab::ConnectedTab,
    cli: &Cli,
    target_actor: &ff_rdp_core::ActorId,
    idle_ms: u64,
    timeout_ms: u64,
    force: bool,
) -> Result<(), AppError> {
    // Tell the daemon to stream network events directly to us.
    crate::daemon::client::start_daemon_stream(ctx.transport_mut(), "network-event")
        .map_err(AppError::from)?;

    // Send reload without reading the ack — events will be streamed inline.
    let reload_packet = build_reload_packet(target_actor, force);
    send_reload_tolerant(ctx.transport_mut(), &reload_packet)?;

    let (requests_observed, idle_at_ms) =
        drain_idle_events(ctx.transport_mut(), idle_ms, timeout_ms, cli.timeout)?;

    // Stop streaming and collect any in-flight frames.
    let inflight_count = match crate::daemon::client::stop_daemon_stream_draining(
        ctx.transport_mut(),
        "network-event",
    ) {
        Ok(frames) => count_network_events_in_frames(&frames),
        Err(e) => {
            eprintln!("warning: failed to stop daemon stream: {e:#}");
            0
        }
    };

    emit_reload_result(cli, requests_observed + inflight_count, idle_at_ms, force)
}

/// True when an I/O error kind signals the peer closed the connection
/// (as opposed to a real transport failure worth surfacing).
///
/// Windows tends to surface a half-closed socket as `ConnectionReset` or
/// `ConnectionAborted` where Unix accepts a final `write` into the send buffer
/// and only reveals the close on the next `read` as `UnexpectedEof`. We treat
/// all of these — plus `BrokenPipe` — as a clean teardown so a `send` that
/// races the server's close does not abort the whole wait-idle flow. This was
/// the iter-108 Windows CI red in `reload_wait_idle_no_traffic_returns_idle_quickly`:
/// the mock closes the connection right after its (empty) followup batch, so on
/// Windows the fire-and-forget `reload` send failed with `ConnectionReset` and
/// the command exited non-zero with an empty stderr (the JSON error envelope
/// went to stdout).
fn is_conn_closed_kind(kind: std::io::ErrorKind) -> bool {
    matches!(
        kind,
        std::io::ErrorKind::UnexpectedEof
            | std::io::ErrorKind::ConnectionReset
            | std::io::ErrorKind::ConnectionAborted
            | std::io::ErrorKind::BrokenPipe
    )
}

/// Send the fire-and-forget `reload` packet, tolerating a connection that the
/// peer has already closed.
///
/// The reload ack is intentionally never read here (events are streamed / drained
/// afterwards), so if the connection is already tearing down we swallow the
/// teardown error and let the subsequent drain loop observe EOF and return idle.
/// Any other send error is a genuine failure and is propagated.
fn send_reload_tolerant(
    transport: &mut ff_rdp_core::RdpTransport,
    reload_packet: &serde_json::Value,
) -> Result<(), AppError> {
    match transport.send(reload_packet) {
        Ok(()) => Ok(()),
        Err(ProtocolError::SendFailed(ref e)) if is_conn_closed_kind(e.kind()) => Ok(()),
        Err(e) => Err(AppError::from(e)),
    }
}

/// Build the JSON `reload` packet, optionally including the
/// `options.force=true` field for a hard reload (Theme B, iter-80).
fn build_reload_packet(target_actor: &ff_rdp_core::ActorId, force: bool) -> serde_json::Value {
    if force {
        json!({
            "to": target_actor.as_ref(),
            "type": "reload",
            "options": {"force": true},
        })
    } else {
        json!({
            "to": target_actor.as_ref(),
            "type": "reload",
        })
    }
}

/// Reload + wait-idle with a direct Firefox connection (no daemon).
fn run_reload_wait_idle_direct(
    ctx: &mut super::connect_tab::ConnectedTab,
    cli: &Cli,
    target_actor: &ff_rdp_core::ActorId,
    idle_ms: u64,
    timeout_ms: u64,
    force: bool,
) -> Result<(), AppError> {
    let tab_actor = ctx.target_tab_actor().clone();
    let watcher_actor =
        TabActor::get_watcher(ctx.transport_mut(), &tab_actor).map_err(AppError::from)?;

    // Subscribe to network events before reloading so we don't miss early requests.
    WatcherActor::watch_resources(ctx.transport_mut(), &watcher_actor, &["network-event"])
        .map_err(AppError::from)?;

    // Send reload without reading the ack.
    let reload_packet = build_reload_packet(target_actor, force);
    send_reload_tolerant(ctx.transport_mut(), &reload_packet)?;

    let (requests_observed, idle_at_ms) =
        drain_idle_events(ctx.transport_mut(), idle_ms, timeout_ms, cli.timeout)?;

    // Unwatch to clean up server-side state.
    let _ =
        WatcherActor::unwatch_resources(ctx.transport_mut(), &watcher_actor, &["network-event"]);

    emit_reload_result(cli, requests_observed, idle_at_ms, force)
}

/// Drain network events from `transport` until idle or timeout.
///
/// Returns `(requests_observed, idle_at_ms)`.
fn drain_idle_events(
    transport: &mut ff_rdp_core::RdpTransport,
    idle_ms: u64,
    timeout_ms: u64,
    cli_timeout: u64,
) -> Result<(u64, u64), AppError> {
    let poll_interval = Duration::from_millis(100);
    transport
        .set_read_timeout(Some(poll_interval))
        .map_err(AppError::from)?;

    let start = Instant::now();
    let total_deadline = Duration::from_millis(timeout_ms);
    let idle_threshold = Duration::from_millis(idle_ms);

    let mut requests_observed: u64 = 0;
    let mut last_event_at: Option<Instant> = None;

    loop {
        if start.elapsed() >= total_deadline {
            break;
        }

        if let Some(t) = last_event_at
            && t.elapsed() >= idle_threshold
        {
            break;
        }

        match transport.recv() {
            Ok(msg) => {
                let msg_type = msg
                    .get("type")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default();
                if msg_type == "resources-available-array" || msg_type == "resources-updated-array"
                {
                    requests_observed += count_network_events(&msg);
                    last_event_at = Some(Instant::now());
                }
            }
            Err(ProtocolError::Timeout) => {}
            Err(ProtocolError::RecvFailed(ref e)) if is_conn_closed_kind(e.kind()) => {
                break;
            }
            Err(e) => {
                let _ = transport.set_read_timeout(Some(Duration::from_millis(cli_timeout)));
                return Err(AppError::from(e));
            }
        }
    }

    let idle_at_ms = u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX);

    // Restore original connection timeout.
    let _ = transport.set_read_timeout(Some(Duration::from_millis(cli_timeout)));

    Ok((requests_observed, idle_at_ms))
}

/// Count individual network resources in a watcher batch message.
fn count_network_events(msg: &serde_json::Value) -> u64 {
    msg.get("array")
        .and_then(serde_json::Value::as_array)
        .map_or(0, |arr| {
            arr.iter()
                .filter_map(|pair| pair.as_array())
                .filter_map(|p| p.get(1))
                .filter_map(serde_json::Value::as_array)
                .map(Vec::len)
                .sum::<usize>()
        }) as u64
}

/// Count network events across multiple collected frames.
fn count_network_events_in_frames(frames: &[serde_json::Value]) -> u64 {
    frames.iter().map(count_network_events).sum()
}

fn emit_reload_result(
    cli: &Cli,
    requests_observed: u64,
    idle_at_ms: u64,
    force: bool,
) -> Result<(), AppError> {
    let result = if force {
        json!({
            "reloaded": true,
            "idle_at_ms": idle_at_ms,
            "requests_observed": requests_observed,
            "force": true,
        })
    } else {
        json!({
            "reloaded": true,
            "idle_at_ms": idle_at_ms,
            "requests_observed": requests_observed,
        })
    };
    let mut meta = json!({});
    crate::connection_meta::merge_into_if_verbose(
        &mut meta,
        &cli.host,
        cli.port,
        None,
        cli.is_verbose(),
    );
    let envelope = output::envelope(&result, 1, &meta);

    let hint_ctx = HintContext::new(HintSource::Reload);
    OutputPipeline::from_cli(cli)?
        .finalize_with_hints(&envelope, Some(&hint_ctx))
        .map_err(AppError::from)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::ErrorKind;

    #[test]
    fn conn_closed_kinds_are_treated_as_teardown() {
        // These four kinds all mean "the peer went away" and must be swallowed
        // by the tolerant reload send / drain loop so a racing close does not
        // abort a wait-idle flow (iter-108 Windows CI red).
        for kind in [
            ErrorKind::UnexpectedEof,
            ErrorKind::ConnectionReset,
            ErrorKind::ConnectionAborted,
            ErrorKind::BrokenPipe,
        ] {
            assert!(
                is_conn_closed_kind(kind),
                "{kind:?} should be classified as a connection-closed teardown"
            );
        }
    }

    #[test]
    fn real_io_errors_are_not_teardown() {
        // A genuine failure (timeout, permission, etc.) must still propagate so
        // it is not silently masked as a clean close.
        for kind in [
            ErrorKind::TimedOut,
            ErrorKind::PermissionDenied,
            ErrorKind::NotConnected,
            ErrorKind::AddrInUse,
        ] {
            assert!(
                !is_conn_closed_kind(kind),
                "{kind:?} must not be classified as a connection-closed teardown"
            );
        }
    }

    #[test]
    fn send_reload_tolerant_swallows_teardown_but_propagates_real_errors() {
        // ConnectionReset on send → treated as a clean close (Ok).
        let reset = ProtocolError::SendFailed(std::io::Error::from(ErrorKind::ConnectionReset));
        assert!(matches!(classify_send_result(Err(reset)), Ok(())));

        // BrokenPipe on send → also Ok.
        let broken = ProtocolError::SendFailed(std::io::Error::from(ErrorKind::BrokenPipe));
        assert!(matches!(classify_send_result(Err(broken)), Ok(())));

        // A genuine send failure (e.g. TimedOut mapped to SendFailed) propagates.
        let timed = ProtocolError::SendFailed(std::io::Error::from(ErrorKind::PermissionDenied));
        assert!(classify_send_result(Err(timed)).is_err());

        // A non-send protocol error propagates unchanged.
        let other = ProtocolError::InvalidPacket("boom".to_string());
        assert!(classify_send_result(Err(other)).is_err());

        // Ok stays Ok.
        assert!(matches!(classify_send_result(Ok(())), Ok(())));
    }

    /// Mirror of the match inside [`send_reload_tolerant`] so the swallow /
    /// propagate policy is unit-testable without a live transport. Kept in sync
    /// with `send_reload_tolerant`.
    fn classify_send_result(res: Result<(), ProtocolError>) -> Result<(), AppError> {
        match res {
            Ok(()) => Ok(()),
            Err(ProtocolError::SendFailed(ref e)) if is_conn_closed_kind(e.kind()) => Ok(()),
            Err(e) => Err(AppError::from(e)),
        }
    }
}
