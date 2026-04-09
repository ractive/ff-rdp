use std::time::Duration;

use ff_rdp_core::{
    RdpTransport, TabActor, WatcherActor, WindowGlobalTarget, parse_network_resource_updates,
    parse_network_resources,
};
use serde_json::json;

use crate::cli::args::Cli;
use crate::error::AppError;
use crate::output;
use crate::output_controls::{OutputControls, SortDir};
use crate::output_pipeline::OutputPipeline;

use super::connect_tab::connect_and_get_target;
use super::js_helpers::{escape_selector, poll_js_condition};
use super::network_events::{
    build_network_entries, drain_network_events_timed, drain_network_from_daemon, merge_updates,
};
use super::url_validation::validate_url;

/// Restore the socket read timeout to the value established at connect time.
///
/// Called after `drain_network_events` completes so that subsequent RDP
/// round-trips (e.g. unwatch, wait condition polling) use the original timeout.
/// Failures are logged and swallowed — the drain has already completed.
fn restore_timeout(transport: &mut RdpTransport, original_timeout_ms: u64) {
    if let Err(e) = transport.set_read_timeout(Some(Duration::from_millis(original_timeout_ms))) {
        eprintln!("warning: failed to restore socket read timeout: {e:#}");
    }
}

/// Options controlling an optional wait condition after navigation.
///
/// # False positive risk
///
/// If the *previous* page already satisfies the wait condition (same selector
/// present, or same text visible) before the new page begins loading, the poll
/// loop may observe a truthy result on the old DOM and return immediately —
/// before the navigation has actually completed.  Callers should be aware of
/// this when reusing the same selector or text across navigations.
// Field names intentionally carry the `wait_` prefix to match the CLI flags
// they correspond to (--wait-text, --wait-selector, --wait-timeout).
#[allow(clippy::struct_field_names)]
pub struct WaitAfterNav<'a> {
    /// Wait until this text appears anywhere on the page body.
    pub wait_text: Option<&'a str>,
    /// Wait until an element matching this CSS selector exists in the DOM.
    pub wait_selector: Option<&'a str>,
    /// Timeout in milliseconds for the wait condition (default: 5000).
    pub wait_timeout: u64,
}

impl WaitAfterNav<'_> {
    fn has_condition(&self) -> bool {
        self.wait_text.is_some() || self.wait_selector.is_some()
    }
}

pub fn run(cli: &Cli, url: &str, wait_opts: &WaitAfterNav<'_>) -> Result<(), AppError> {
    if !cli.allow_unsafe_urls {
        validate_url(url)?;
    }
    let mut ctx = connect_and_get_target(cli)?;
    let target_actor = ctx.target.actor.clone();

    WindowGlobalTarget::navigate_to(ctx.transport_mut(), &target_actor, url)
        .map_err(AppError::from)?;

    let wait_result = wait_after_navigate(&mut ctx, wait_opts)?;

    let mut result = json!({"navigated": url});
    if let Some(w) = wait_result
        && let Some(obj) = result.as_object_mut()
    {
        obj.insert("wait".to_string(), w);
    }
    let meta = json!({"host": cli.host, "port": cli.port});
    let envelope = output::envelope(&result, 1, &meta);

    OutputPipeline::from_cli(cli)?
        .finalize(&envelope)
        .map_err(AppError::from)
}

/// Navigate to `url` and capture all network requests made during navigation.
///
/// The flow on a single TCP connection is:
/// 1. Connect and resolve the target tab.
/// 2. Get the WatcherActor via `TabActor::get_watcher`.
/// 3. Subscribe to `"network-event"` resources via `WatcherActor::watch_resources`.
/// 4. Navigate with `WindowGlobalTarget::navigate_to`.
/// 5. Drain `resources-available-array` / `resources-updated-array` events
///    (timeout-bounded, same pattern as the `network` command).
/// 6. Merge updates into resources by `resource_id`.
/// 7. Unwatch resources to clean up server-side state.
/// 8. Optionally wait for a condition (--wait-text / --wait-selector).
/// 9. Emit combined JSON output.
pub fn run_with_network(
    cli: &Cli,
    url: &str,
    wait_opts: &WaitAfterNav<'_>,
    network_timeout_ms: u64,
) -> Result<(), AppError> {
    if !cli.allow_unsafe_urls {
        validate_url(url)?;
    }
    let mut ctx = connect_and_get_target(cli)?;
    let target_actor = ctx.target.actor.clone();

    if ctx.via_daemon {
        // Tell the daemon to stream network events in real-time instead of
        // buffering.  This clears the existing buffer so we only capture
        // events from *this* navigation.
        crate::daemon::client::start_daemon_stream(ctx.transport_mut(), "network-event")
            .map_err(AppError::from)?;

        // Send the navigateTo request without reading its response — same as
        // the non-daemon path.  The daemon will forward the ack and also
        // stream watcher events directly to us.
        ctx.transport_mut()
            .send(&json!({
                "to": target_actor.as_ref(),
                "type": "navigateTo",
                "url": url,
            }))
            .map_err(AppError::from)?;

        // Drain streamed watcher events for the total_timeout wall-clock
        // duration, using short 500ms poll intervals internally.  This
        // captures events that arrive in bursts with gaps (e.g. the page
        // navigation itself may take 1-2 seconds before any network events
        // start, which would incorrectly fire an idle-based timeout early).
        // Always stop streaming before propagating errors from drain so the
        // daemon does not get stuck in streaming mode on failure.
        let drain_result = drain_network_events_timed(
            ctx.transport_mut(),
            Duration::from_millis(network_timeout_ms),
        );

        // Restore the original connection timeout before stopping the stream
        // so any RDP round-trip uses the right timeout.
        restore_timeout(ctx.transport_mut(), cli.timeout);

        // Stop streaming and collect any in-flight watcher frames that arrived
        // between the idle-timeout cutoff and the stop-stream acknowledgement.
        // These are events the daemon forwarded after drain_network_events
        // returned but before it processed our stop-stream request.
        let inflight = match crate::daemon::client::stop_daemon_stream_draining(
            ctx.transport_mut(),
            "network-event",
        ) {
            Ok(frames) => frames,
            Err(e) => {
                eprintln!("warning: failed to stop daemon stream: {e:#}");
                vec![]
            }
        };

        let (mut all_resources, mut all_updates, timeout_reached) =
            drain_result.map_err(AppError::from)?;

        // Parse and merge any in-flight frames collected from stop_daemon_stream.
        for frame in &inflight {
            let msg_type = frame
                .get("type")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default();
            match msg_type {
                "resources-available-array" => {
                    all_resources.extend(parse_network_resources(frame));
                }
                "resources-updated-array" => {
                    all_updates.extend(parse_network_resource_updates(frame));
                }
                _ => {}
            }
        }

        // After stop-stream the daemon reverts to buffering.  Any events that
        // arrived at Firefox between the idle-timeout firing and the daemon
        // removing this client's stream subscription get buffered instead of
        // forwarded.  Drain that residual buffer now so nothing is lost.
        match drain_network_from_daemon(ctx.transport_mut()) {
            Ok((residual_resources, residual_updates)) => {
                all_resources.extend(residual_resources);
                all_updates.extend(residual_updates);
            }
            Err(e) => {
                eprintln!("warning: failed to drain residual daemon buffer after stream: {e:#}");
            }
        }

        let wait_result = wait_after_navigate(&mut ctx, wait_opts)?;

        let update_map = merge_updates(all_updates);
        let network_entries = build_network_entries(&all_resources, &update_map);

        let network_entries = apply_network_controls(cli, network_entries, timeout_reached);

        let mut result = json!({
            "navigated": url,
            "network": network_entries,
        });
        if let Some(w) = wait_result
            && let Some(obj) = result.as_object_mut()
        {
            obj.insert("wait".to_string(), w);
        }
        let meta = json!({"host": cli.host, "port": cli.port});
        let envelope = output::envelope(&result, 1, &meta);
        return OutputPipeline::from_cli(cli)?
            .finalize(&envelope)
            .map_err(AppError::from);
    }

    let tab_actor = ctx.target_tab_actor().clone();

    // Get watcher actor for resource subscriptions.
    let watcher_actor =
        TabActor::get_watcher(ctx.transport_mut(), &tab_actor).map_err(AppError::from)?;

    // Subscribe to network events before navigating so we capture everything.
    WatcherActor::watch_resources(ctx.transport_mut(), &watcher_actor, &["network-event"])
        .map_err(AppError::from)?;

    // Send the navigateTo request without reading its response.  The normal
    // `WindowGlobalTarget::navigate_to` uses `actor_request` which loops
    // reading messages until it finds one from the target actor — silently
    // discarding any `resources-available-array` events from the watcher that
    // arrive in between.  By sending raw, we let `drain_network_events`
    // collect those events (it skips non-network message types harmlessly).
    ctx.transport_mut()
        .send(&json!({
            "to": target_actor.as_ref(),
            "type": "navigateTo",
            "url": url,
        }))
        .map_err(AppError::from)?;

    // Drain resource events for the total_timeout wall-clock duration,
    // using short 500ms poll intervals internally.  This captures events
    // that arrive in bursts with gaps — the navigateTo ack is harmlessly
    // skipped by the drain since it is not a network resource message type.
    let drain_result = drain_network_events_timed(
        ctx.transport_mut(),
        Duration::from_millis(network_timeout_ms),
    );

    // Restore original timeout before any further RDP round-trips (unwatch).
    restore_timeout(ctx.transport_mut(), cli.timeout);

    let (all_resources, all_updates, timeout_reached) = drain_result.map_err(AppError::from)?;

    // Merge updates into resources by resource_id.
    let update_map = merge_updates(all_updates);

    // Build the network entries array (no URL/method filtering here).
    let network_entries = build_network_entries(&all_resources, &update_map);

    // Unwatch to clean up server-side resources.
    let _ =
        WatcherActor::unwatch_resources(ctx.transport_mut(), &watcher_actor, &["network-event"]);

    // NOTE: In the non-daemon path, wait_after_navigate is called *after*
    // draining network events and unwatching resources, so network data is
    // already fully collected before we begin waiting.  The daemon path
    // (above) starts the wait before building entries because there is no
    // subscription lifecycle to tear down.
    let wait_result = wait_after_navigate(&mut ctx, wait_opts)?;

    let network_entries = apply_network_controls(cli, network_entries, timeout_reached);

    let mut result = json!({
        "navigated": url,
        "network": network_entries,
    });
    if let Some(w) = wait_result
        && let Some(obj) = result.as_object_mut()
    {
        obj.insert("wait".to_string(), w);
    }
    let meta = json!({"host": cli.host, "port": cli.port});
    let envelope = output::envelope(&result, 1, &meta);

    OutputPipeline::from_cli(cli)?
        .finalize(&envelope)
        .map_err(AppError::from)
}

/// Apply output controls (sort, limit, fields) to network entries from navigate.
///
/// In detail mode (when the user sets --detail, --jq, --sort, --limit, --fields,
/// or --all), returns the processed array. Otherwise returns a summary object.
///
/// `timeout_reached` is forwarded to [`build_network_summary`] so it can include
/// the hint field when the collection deadline fired while events were still arriving.
fn apply_network_controls(
    cli: &Cli,
    network_entries: Vec<serde_json::Value>,
    timeout_reached: bool,
) -> serde_json::Value {
    let use_detail = cli.detail
        || cli.jq.is_some()
        || cli.sort.is_some()
        || cli.limit.is_some()
        || cli.all
        || cli.fields.is_some();

    if use_detail {
        let controls = OutputControls::from_cli(cli, SortDir::Desc);
        let mut detail = network_entries;
        if cli.sort.is_none() {
            let dir = controls.sort_dir;
            detail.sort_by(|a, b| {
                let da = a["duration_ms"].as_f64().unwrap_or(0.0);
                let db = b["duration_ms"].as_f64().unwrap_or(0.0);
                let cmp = da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal);
                match dir {
                    SortDir::Asc => cmp,
                    SortDir::Desc => cmp.reverse(),
                }
            });
        } else {
            controls.apply_sort(&mut detail);
        }
        let (limited, total, truncated) = controls.apply_limit(detail, Some(20));
        let limited = controls.apply_fields(limited);
        if truncated {
            let shown = limited.len();
            json!({
                "entries": limited,
                "shown": shown,
                "total": total,
                "truncated": true,
                "hint": format!("showing {shown} of {total}, use --all for complete list"),
            })
        } else {
            json!(limited)
        }
    } else {
        super::network::build_network_summary(&network_entries, timeout_reached)
    }
}

/// Poll a JS condition after navigation until it becomes truthy or times out.
///
/// Returns `Ok(Some(json))` when the condition is met, `Ok(None)` when no
/// condition was requested, and `Err` when the timeout expires or evaluation
/// fails with an exception.
fn wait_after_navigate(
    ctx: &mut super::connect_tab::ConnectedTab,
    opts: &WaitAfterNav<'_>,
) -> Result<Option<serde_json::Value>, AppError> {
    if !opts.has_condition() {
        return Ok(None);
    }

    let js = if let Some(sel) = opts.wait_selector {
        let escaped = escape_selector(sel);
        format!("document.querySelector('{escaped}') !== null")
    } else if let Some(text) = opts.wait_text {
        let escaped = serde_json::to_string(text)
            .map_err(|e| AppError::from(anyhow::anyhow!("failed to encode wait-text: {e}")))?;
        format!("(document.body && document.body.innerText.includes({escaped}))")
    } else {
        // has_condition() guarantees at least one is set; this branch is unreachable.
        return Ok(None);
    };

    let console_actor = ctx.target.console_actor.clone();
    let condition = describe_wait_condition(opts);
    let timeout_msg = format!(
        "navigate wait timed out after {}ms — condition not met: {condition}; increase with --wait-timeout",
        opts.wait_timeout
    );

    let elapsed_ms = poll_js_condition(
        ctx,
        &console_actor,
        &js,
        opts.wait_timeout,
        "navigate wait aborted due to JS exception",
        &timeout_msg,
    )?;

    Ok(Some(json!({
        "waited": true,
        "elapsed_ms": elapsed_ms,
        "condition": condition,
    })))
}

fn describe_wait_condition(opts: &WaitAfterNav<'_>) -> String {
    if let Some(sel) = opts.wait_selector {
        format!("selector={sel:?}")
    } else if let Some(text) = opts.wait_text {
        format!("text={text:?}")
    } else {
        "(none)".into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wait_after_nav_no_condition_returns_none() {
        let opts = WaitAfterNav {
            wait_text: None,
            wait_selector: None,
            wait_timeout: 5000,
        };
        assert!(!opts.has_condition());
    }

    #[test]
    fn wait_after_nav_text_has_condition() {
        let opts = WaitAfterNav {
            wait_text: Some("Hello"),
            wait_selector: None,
            wait_timeout: 5000,
        };
        assert!(opts.has_condition());
    }

    #[test]
    fn wait_after_nav_selector_has_condition() {
        let opts = WaitAfterNav {
            wait_text: None,
            wait_selector: Some("button.submit"),
            wait_timeout: 5000,
        };
        assert!(opts.has_condition());
    }

    #[test]
    fn describe_wait_condition_selector() {
        let opts = WaitAfterNav {
            wait_text: None,
            wait_selector: Some("div#main"),
            wait_timeout: 3000,
        };
        assert_eq!(describe_wait_condition(&opts), r#"selector="div#main""#);
    }

    #[test]
    fn describe_wait_condition_text() {
        let opts = WaitAfterNav {
            wait_text: Some("Loaded"),
            wait_selector: None,
            wait_timeout: 3000,
        };
        assert_eq!(describe_wait_condition(&opts), r#"text="Loaded""#);
    }
}
