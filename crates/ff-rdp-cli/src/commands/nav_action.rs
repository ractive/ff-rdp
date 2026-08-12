use std::time::{Duration, Instant};

use ff_rdp_core::{ProtocolError, TabActor, WatcherActor};
use serde_json::json;

use crate::cli::args::Cli;
use crate::error::AppError;
use crate::hints::{HintContext, HintSource};
use crate::output;
use crate::output_pipeline::OutputPipeline;

use super::connect_tab::connect_and_get_target;
use super::navigate::{eval_location_href, wait_for_navigation_commit};

/// Which navigation action to perform.
#[derive(Clone, Copy)]
pub enum NavAction {
    /// Reload the current page. `force = true` bypasses the HTTP cache.
    Reload {
        force: bool,
        no_wait: bool,
    },
    Back {
        no_wait: bool,
    },
    Forward {
        no_wait: bool,
    },
}

impl NavAction {
    /// Whether this invocation should skip the commit wait (iter-138 Theme E)
    /// — the escape hatch the Theme B/C error text has always recommended
    /// but, before this iteration, didn't exist for `back`/`forward`/`reload`
    /// (`error: unexpected argument '--no-wait' found`).
    fn no_wait(self) -> bool {
        match self {
            Self::Reload { no_wait, .. } | Self::Back { no_wait } | Self::Forward { no_wait } => {
                no_wait
            }
        }
    }
}

/// Run `back`, `forward`, or a plain (non-`--wait-idle`) `reload`.
///
/// iter-130 Theme B: all three verbs now share `navigate`'s
/// `{committed_url, ready_state, elapsed_ms}` envelope via
/// [`wait_for_navigation_commit`], instead of the bare `{"action": "..."}`
/// this command used to return immediately after dispatch. The action's own
/// request (`reload`/`goBack`/`goForward`) is sent as a **raw** write from
/// inside the `dispatch` closure — see `wait_for_navigation_commit`'s doc
/// comment for why routing it through the old blocking
/// `WindowGlobalTarget::reload`/`go_back`/`go_forward` (which read the ack via
/// `recv_reply_from`) risks silently losing a `document-event` that races
/// ahead of that ack.
///
/// iter-138 Theme E: `--no-wait` on any of the three skips the commit wait
/// entirely and returns the bare `{"action": "..."}` envelope this command
/// used to always return before iter-130 — the escape hatch the Theme B/C
/// timeout message recommends, which previously didn't exist for these three
/// verbs at all.
pub fn run(cli: &Cli, action: NavAction) -> Result<(), AppError> {
    let mut ctx = connect_and_get_target(cli)?;
    let target_actor = ctx.target.actor.clone();

    let (action_name, force_reload) = match action {
        NavAction::Reload { force, .. } => ("reload", force),
        NavAction::Back { .. } => ("back", false),
        NavAction::Forward { .. } => ("forward", false),
    };

    let build_packet = {
        let target_actor = target_actor.clone();
        move |action: NavAction| match action {
            NavAction::Reload { force, .. } => build_reload_packet(&target_actor, force),
            NavAction::Back { .. } => json!({"to": target_actor.as_ref(), "type": "goBack"}),
            NavAction::Forward { .. } => json!({"to": target_actor.as_ref(), "type": "goForward"}),
        }
    };

    let commit_json = if action.no_wait() {
        // --no-wait: dispatch without reading the ack and skip the commit
        // wait entirely, mirroring `navigate --no-wait`.
        let packet = build_packet(action);
        ctx.transport_mut().send(&packet).map_err(AppError::from)?;
        json!({})
    } else {
        // `reload`'s target URL is knowable ahead of time (the current page,
        // reloaded) — capture it so `needs_href_fallback` can tell a genuine
        // reload-of-about:blank apart from a stale event placeholder.
        // `back`/`forward` don't know their landing URL ahead of time, so
        // they pass "" (safe — see `wait_for_navigation_commit`'s doc
        // comment).
        let requested_url = match action {
            NavAction::Reload { .. } => {
                let console_actor = ctx.target.console_actor.clone();
                eval_location_href(ctx.transport_mut(), &console_actor)
            }
            NavAction::Back { .. } | NavAction::Forward { .. } => String::new(),
        };

        wait_for_navigation_commit(&mut ctx, cli.timeout, &requested_url, move |transport| {
            transport
                .send(&build_packet(action))
                .map_err(AppError::from)
        })?
    };

    let mut result = if force_reload {
        json!({"action": action_name, "force": true})
    } else {
        json!({"action": action_name})
    };
    if let (Some(obj), Some(commit_obj)) = (result.as_object_mut(), commit_json.as_object()) {
        for (k, v) in commit_obj {
            obj.insert(k.clone(), v.clone());
        }
    }

    let mut meta = json!({});
    crate::connection_meta::merge_into_if_verbose(
        &mut meta,
        &cli.host,
        cli.port,
        None,
        cli.is_verbose(),
    );
    // iter-134: always present, not gated by --verbose — an
    // agent can tell how this command executed without a
    // separate `daemon status` round-trip.
    crate::connection_meta::merge_route(&mut meta, ctx.via_daemon);
    let envelope = output::envelope(&result, 1, &meta);

    let hint_source = match action {
        NavAction::Reload { .. } => HintSource::Reload,
        NavAction::Back { .. } => HintSource::Back,
        NavAction::Forward { .. } => HintSource::Forward,
    };
    let hint_ctx = HintContext::new(hint_source);
    OutputPipeline::from_cli(cli)?.finalize_with_hints(&envelope, Some(&hint_ctx))
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
            // stderr-ok: (b) warn-and-continue — falls back to 0 in-flight
            // frames; requests_observed above still carries the real count.
            eprintln!("warning: failed to stop daemon stream: {e:#}");
            0
        }
    };

    emit_reload_result(
        cli,
        requests_observed + inflight_count,
        idle_at_ms,
        force,
        true,
    )
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

    emit_reload_result(cli, requests_observed, idle_at_ms, force, false)
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
    via_daemon: bool,
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
    // iter-134: always present, not gated by --verbose.
    crate::connection_meta::merge_route(&mut meta, via_daemon);
    let envelope = output::envelope(&result, 1, &meta);

    let hint_ctx = HintContext::new(HintSource::Reload);
    OutputPipeline::from_cli(cli)?.finalize_with_hints(&envelope, Some(&hint_ctx))
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
