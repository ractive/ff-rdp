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
    Reload,
    Back,
    Forward,
}

pub fn run(cli: &Cli, action: NavAction) -> Result<(), AppError> {
    let mut ctx = connect_and_get_target(cli)?;
    let target_actor = ctx.target.actor.clone();

    let action_name = match action {
        NavAction::Reload => {
            WindowGlobalTarget::reload(ctx.transport_mut(), &target_actor)
                .map_err(AppError::from)?;
            "reload"
        }
        NavAction::Back => {
            WindowGlobalTarget::go_back(ctx.transport_mut(), &target_actor)
                .map_err(AppError::from)?;
            "back"
        }
        NavAction::Forward => {
            WindowGlobalTarget::go_forward(ctx.transport_mut(), &target_actor)
                .map_err(AppError::from)?;
            "forward"
        }
    };

    let result = json!({"action": action_name});
    let meta = json!({"host": cli.host, "port": cli.port});
    let envelope = output::envelope(&result, 1, &meta);

    let hint_source = match action {
        NavAction::Reload => HintSource::Reload,
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
pub fn run_reload_wait_idle(cli: &Cli, idle_ms: u64, timeout_ms: u64) -> Result<(), AppError> {
    let mut ctx = connect_and_get_target(cli)?;
    let target_actor = ctx.target.actor.clone();

    if ctx.via_daemon {
        return run_reload_wait_idle_daemon(&mut ctx, cli, &target_actor, idle_ms, timeout_ms);
    }

    run_reload_wait_idle_direct(&mut ctx, cli, &target_actor, idle_ms, timeout_ms)
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
) -> Result<(), AppError> {
    // Tell the daemon to stream network events directly to us.
    crate::daemon::client::start_daemon_stream(ctx.transport_mut(), "network-event")
        .map_err(AppError::from)?;

    // Send reload without reading the ack — events will be streamed inline.
    ctx.transport_mut()
        .send(&json!({
            "to": target_actor.as_ref(),
            "type": "reload",
        }))
        .map_err(AppError::from)?;

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

    emit_reload_result(cli, requests_observed + inflight_count, idle_at_ms)
}

/// Reload + wait-idle with a direct Firefox connection (no daemon).
fn run_reload_wait_idle_direct(
    ctx: &mut super::connect_tab::ConnectedTab,
    cli: &Cli,
    target_actor: &ff_rdp_core::ActorId,
    idle_ms: u64,
    timeout_ms: u64,
) -> Result<(), AppError> {
    let tab_actor = ctx.target_tab_actor().clone();
    let watcher_actor =
        TabActor::get_watcher(ctx.transport_mut(), &tab_actor).map_err(AppError::from)?;

    // Subscribe to network events before reloading so we don't miss early requests.
    WatcherActor::watch_resources(ctx.transport_mut(), &watcher_actor, &["network-event"])
        .map_err(AppError::from)?;

    // Send reload without reading the ack.
    ctx.transport_mut()
        .send(&json!({
            "to": target_actor.as_ref(),
            "type": "reload",
        }))
        .map_err(AppError::from)?;

    let (requests_observed, idle_at_ms) =
        drain_idle_events(ctx.transport_mut(), idle_ms, timeout_ms, cli.timeout)?;

    // Unwatch to clean up server-side state.
    let _ =
        WatcherActor::unwatch_resources(ctx.transport_mut(), &watcher_actor, &["network-event"]);

    emit_reload_result(cli, requests_observed, idle_at_ms)
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
            Err(ProtocolError::RecvFailed(ref e))
                if e.kind() == std::io::ErrorKind::UnexpectedEof
                    || e.kind() == std::io::ErrorKind::ConnectionReset
                    || e.kind() == std::io::ErrorKind::BrokenPipe =>
            {
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

fn emit_reload_result(cli: &Cli, requests_observed: u64, idle_at_ms: u64) -> Result<(), AppError> {
    let result = json!({
        "reloaded": true,
        "idle_at_ms": idle_at_ms,
        "requests_observed": requests_observed,
    });
    let meta = json!({"host": cli.host, "port": cli.port});
    let envelope = output::envelope(&result, 1, &meta);

    let hint_ctx = HintContext::new(HintSource::Reload);
    OutputPipeline::from_cli(cli)?
        .finalize_with_hints(&envelope, Some(&hint_ctx))
        .map_err(AppError::from)
}
