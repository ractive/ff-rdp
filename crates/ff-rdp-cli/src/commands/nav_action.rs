use std::time::{Duration, Instant};

use ff_rdp_core::{ProtocolError, TabActor, WatcherActor, WindowGlobalTarget};
use serde_json::json;

use crate::cli::args::Cli;
use crate::error::AppError;
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

    OutputPipeline::from_cli(cli)?
        .finalize(&envelope)
        .map_err(AppError::from)
}

/// Reload the page and wait until network activity has been idle for `idle_ms`
/// or the total wall-clock time exceeds `timeout_ms`.
///
/// ## Protocol flow
///
/// 1. Get the WatcherActor for the target tab.
/// 2. Subscribe to `"network-event"` resources.
/// 3. Send the `reload` request without consuming its response so the watcher
///    stream can deliver network events right from the start.
/// 4. Loop collecting events with a short poll interval:
///    - Track the time of the last received network event.
///    - Return "idle" when no event has arrived in `idle_ms`.
///    - Return "timeout" when the total wall-clock time exceeds `timeout_ms`.
/// 5. Unwatch resources to clean up server-side state.
/// 6. Emit `{reloaded: true, idle_at_ms: N, requests_observed: M}`.
pub fn run_reload_wait_idle(cli: &Cli, idle_ms: u64, timeout_ms: u64) -> Result<(), AppError> {
    let mut ctx = connect_and_get_target(cli)?;
    let target_actor = ctx.target.actor.clone();

    let tab_actor = ctx.target_tab_actor().clone();
    let watcher_actor =
        TabActor::get_watcher(ctx.transport_mut(), &tab_actor).map_err(AppError::from)?;

    // Subscribe to network events before reloading so we don't miss early requests.
    WatcherActor::watch_resources(ctx.transport_mut(), &watcher_actor, &["network-event"])
        .map_err(AppError::from)?;

    // Send reload without reading the ack — the ack and any early network events
    // will be collected by the idle-drain loop below.
    ctx.transport_mut()
        .send(&json!({
            "to": target_actor.as_ref(),
            "type": "reload",
        }))
        .map_err(AppError::from)?;

    // Idle-drain: collect network events until idle_ms passes with no new event
    // OR timeout_ms of total wall-clock time elapses.
    let poll_interval = Duration::from_millis(100);
    ctx.transport_mut()
        .set_read_timeout(Some(poll_interval))
        .map_err(AppError::from)?;

    let start = Instant::now();
    let total_deadline = Duration::from_millis(timeout_ms);
    let idle_threshold = Duration::from_millis(idle_ms);

    let mut requests_observed: u64 = 0;
    let mut last_event_at = Instant::now();

    loop {
        // Check total timeout first.
        if start.elapsed() >= total_deadline {
            break;
        }

        // Check idle threshold: if we've had at least one request and the last
        // event was more than idle_ms ago, declare idle.
        if requests_observed > 0 && last_event_at.elapsed() >= idle_threshold {
            break;
        }

        match ctx.transport_mut().recv() {
            Ok(msg) => {
                let msg_type = msg
                    .get("type")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default();
                if msg_type == "resources-available-array" || msg_type == "resources-updated-array"
                {
                    // Count each individual network resource in the batch.
                    let count = msg
                        .get("array")
                        .and_then(serde_json::Value::as_array)
                        .map_or(1, |arr| {
                            arr.iter()
                                .filter_map(|pair| pair.as_array())
                                .filter_map(|p| p.get(1))
                                .filter_map(serde_json::Value::as_array)
                                .map(Vec::len)
                                .sum::<usize>()
                        }) as u64;
                    requests_observed += count;
                    last_event_at = Instant::now();
                }
                // Non-network messages (e.g. the reload ack) are harmlessly ignored.
            }
            Err(ProtocolError::Timeout) => {
                // Poll interval expired with no message.
                // The idle/total checks at the top of the loop will handle termination.
            }
            Err(ProtocolError::RecvFailed(ref e))
                if e.kind() == std::io::ErrorKind::UnexpectedEof
                    || e.kind() == std::io::ErrorKind::ConnectionReset
                    || e.kind() == std::io::ErrorKind::BrokenPipe =>
            {
                // Connection closed — treat as idle.
                break;
            }
            Err(e) => {
                // Restore timeout before returning the error.
                let _ = ctx
                    .transport_mut()
                    .set_read_timeout(Some(Duration::from_millis(cli.timeout)));
                return Err(AppError::from(e));
            }
        }
    }

    let idle_at_ms = u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX);

    // Restore original connection timeout.
    let _ = ctx
        .transport_mut()
        .set_read_timeout(Some(Duration::from_millis(cli.timeout)));

    // Unwatch to clean up server-side state.
    let _ =
        WatcherActor::unwatch_resources(ctx.transport_mut(), &watcher_actor, &["network-event"]);

    let result = json!({
        "reloaded": true,
        "idle_at_ms": idle_at_ms,
        "requests_observed": requests_observed,
    });
    let meta = json!({"host": cli.host, "port": cli.port});
    let envelope = output::envelope(&result, 1, &meta);

    OutputPipeline::from_cli(cli)?
        .finalize(&envelope)
        .map_err(AppError::from)
}
