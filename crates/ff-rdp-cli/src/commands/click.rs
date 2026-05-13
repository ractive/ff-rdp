use std::time::{Duration, Instant};

/// Per-recv polling interval used while waiting for a matching network request.
/// Keeps the wall-clock deadline honored even when the transport's global
/// read timeout is larger than `--network-timeout`.
const POLL_INTERVAL: Duration = Duration::from_millis(200);

use ff_rdp_core::{
    NetworkResource, NetworkResourceUpdate, ProtocolError, TabActor, WatcherActor,
    parse_network_resource_updates, parse_network_resources,
};
use serde_json::{Value, json};

use crate::cli::args::Cli;
use crate::error::AppError;
use crate::hints::{HintContext, HintSource};
use crate::output;
use crate::output_pipeline::OutputPipeline;

use super::connect_tab::{ConnectedTab, connect_and_get_target};
use super::js_helpers::{JSON_SENTINEL, escape_selector, eval_or_bail, resolve_result};
use super::network_events::build_network_entries;

pub fn run(
    cli: &Cli,
    selector: &str,
    wait_for_network: Option<&str>,
    network_timeout: Option<u64>,
) -> Result<(), AppError> {
    let mut ctx = connect_and_get_target(cli)?;

    // When --wait-for-network is requested in direct mode, subscribe to the
    // watcher before clicking so we don't miss early events.
    let watcher_sub = if wait_for_network.is_some() && !ctx.via_daemon {
        let tab_actor = ctx.target_tab_actor().clone();
        let watcher_actor =
            TabActor::get_watcher(ctx.transport_mut(), &tab_actor).map_err(AppError::from)?;
        WatcherActor::watch_resources(ctx.transport_mut(), &watcher_actor, &["network-event"])
            .map_err(AppError::from)?;
        Some(watcher_actor)
    } else {
        None
    };

    // For daemon mode with --wait-for-network, start streaming before click
    // so events that arrive immediately after the click aren't dropped.
    let daemon_streaming = if wait_for_network.is_some() && ctx.via_daemon {
        use crate::daemon::client::start_daemon_stream;
        start_daemon_stream(ctx.transport_mut(), "network-event").map_err(AppError::from)?;
        true
    } else {
        false
    };

    // Perform the click.
    let click_json = do_click(&mut ctx, selector)?;

    // Gather the network result if requested.
    let network_result = if let Some(pattern) = wait_for_network {
        let timeout_ms = network_timeout.unwrap_or(cli.timeout);
        let matched = if ctx.via_daemon {
            wait_for_matching_request_daemon(&mut ctx, pattern, timeout_ms)?
        } else {
            wait_for_matching_request_direct(&mut ctx, pattern, timeout_ms)?
        };
        Some(matched)
    } else {
        None
    };

    // Clean up subscriptions after we have the result.
    if let Some(ref watcher_actor) = watcher_sub {
        let _ =
            WatcherActor::unwatch_resources(ctx.transport_mut(), watcher_actor, &["network-event"]);
    }
    if daemon_streaming {
        use crate::daemon::client::stop_daemon_stream;
        let _ = stop_daemon_stream(ctx.transport_mut(), "network-event");
    }

    // Build the output.
    let mut result = click_json;
    if let Some(net) = network_result {
        result["network"] = net;
    }

    let mut meta = json!({"host": cli.host, "port": cli.port, "selector": selector});
    crate::connection_meta::merge_into(&mut meta, &cli.host, cli.port, None);
    let envelope = output::envelope(&result, 1, &meta);

    let hint_ctx = HintContext::new(HintSource::Click).with_selector(selector);
    OutputPipeline::from_cli(cli)?
        .finalize_with_hints(&envelope, Some(&hint_ctx))
        .map_err(AppError::from)
}

fn do_click(ctx: &mut ConnectedTab, selector: &str) -> Result<Value, AppError> {
    let console_actor = ctx.target.console_actor.clone();
    let escaped = escape_selector(selector);
    let js = format!(
        r"(function() {{
  var el = document.querySelector('{escaped}');
  if (!el) throw new Error('Element not found: {escaped} — use ff-rdp dom SELECTOR --count to verify the selector matches');
  el.click();
  return '{JSON_SENTINEL}' + JSON.stringify({{clicked: true, tag: el.tagName, text: (el.textContent || '').trim().substring(0, 100)}});
}})()"
    );
    let eval_result = eval_or_bail(ctx, &console_actor, &js, "click failed")?;
    resolve_result(ctx, &eval_result.result)
}

/// Wait for a resolved network request matching `pattern` using the daemon stream.
///
/// The daemon is already streaming events to us (started before the click).
/// We read the stream until we find a completed request whose URL contains
/// `pattern`, or until the timeout fires.
fn wait_for_matching_request_daemon(
    ctx: &mut ConnectedTab,
    pattern: &str,
    timeout_ms: u64,
) -> Result<Value, AppError> {
    let timeout = Duration::from_millis(timeout_ms);
    let started = Instant::now();

    let mut pending: std::collections::HashMap<u64, NetworkResource> =
        std::collections::HashMap::new();

    // Cap per-recv blocking via POLL_INTERVAL so the wall-clock deadline is
    // honored even when the global transport read timeout is larger than the
    // requested --network-timeout.  Restored to the global value before
    // returning so subsequent transport reads behave normally.
    let _ = ctx.transport_mut().set_read_timeout(Some(POLL_INTERVAL));

    let outcome = run_wait_loop(ctx, pattern, timeout, started, timeout_ms, &mut pending);

    let _ = ctx.transport_mut().set_read_timeout(None);

    outcome
}

fn run_wait_loop(
    ctx: &mut ConnectedTab,
    pattern: &str,
    timeout: Duration,
    started: Instant,
    timeout_ms: u64,
    pending: &mut std::collections::HashMap<u64, NetworkResource>,
) -> Result<Value, AppError> {
    loop {
        if started.elapsed() >= timeout {
            return Err(AppError::Timeout(format!(
                "no network request matching '{pattern}' completed within {timeout_ms}ms"
            )));
        }

        match ctx.transport_mut().recv() {
            Ok(msg) => {
                let msg_type = msg.get("type").and_then(Value::as_str).unwrap_or_default();
                match msg_type {
                    "resources-available-array" => {
                        for res in parse_network_resources(&msg) {
                            if res.url.contains(pattern) {
                                pending.insert(res.resource_id, res);
                            }
                        }
                    }
                    "resources-updated-array" => {
                        for update in parse_network_resource_updates(&msg) {
                            if let Some(res) = pending.remove(&update.resource_id) {
                                if update.status.is_some() {
                                    return Ok(build_matched_entry(&res, &update));
                                }
                                // Status not yet available — put it back.
                                pending.insert(res.resource_id, res);
                            }
                        }
                    }
                    _ => {}
                }
            }
            Err(ProtocolError::Timeout) => {
                // Per-read timeout — check wall-clock deadline on next iteration.
            }
            Err(e) => return Err(AppError::from(e)),
        }
    }
}

/// Wait for a resolved network request matching `pattern` in direct (non-daemon) mode.
///
/// The watcher subscription was already set up before the click. We drain
/// events from the transport until we find a completed matching request or
/// the timeout fires.
fn wait_for_matching_request_direct(
    ctx: &mut ConnectedTab,
    pattern: &str,
    timeout_ms: u64,
) -> Result<Value, AppError> {
    // Reuse the same loop logic — the transport delivers watcher events the same
    // way in direct mode; the watcher subscription was set up before the click.
    wait_for_matching_request_daemon(ctx, pattern, timeout_ms)
}

/// Build a single network entry JSON from a matched resource + its update.
fn build_matched_entry(res: &NetworkResource, update: &NetworkResourceUpdate) -> Value {
    let mut entries = build_network_entries(
        std::slice::from_ref(res),
        &std::iter::once((res.resource_id, update.clone())).collect(),
    );
    entries.pop().unwrap_or(Value::Null)
}
