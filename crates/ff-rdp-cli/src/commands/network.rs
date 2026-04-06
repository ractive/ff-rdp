use ff_rdp_core::{Grip, LongStringActor, TabActor, WatcherActor, WebConsoleActor};
use serde_json::json;

use crate::cli::args::Cli;
use crate::error::AppError;
use crate::output;
use crate::output_pipeline::OutputPipeline;

use super::connect_tab::connect_and_get_target;
use super::network_events::{build_network_entries, drain_network_events, merge_updates};

pub fn run(cli: &Cli, filter: Option<&str>, method: Option<&str>) -> Result<(), AppError> {
    let mut ctx = connect_and_get_target(cli)?;
    let tab_actor = ctx.target_tab_actor().clone();

    // Get the watcher actor for resource subscriptions.
    let watcher_actor =
        TabActor::get_watcher(ctx.transport_mut(), &tab_actor).map_err(AppError::from)?;

    // Subscribe to network events. This triggers Firefox to send existing
    // network events as `resources-available-array` messages.
    WatcherActor::watch_resources(ctx.transport_mut(), &watcher_actor, &["network-event"])
        .map_err(AppError::from)?;

    // Collect resource events until timeout.
    let (all_resources, all_updates) =
        drain_network_events(ctx.transport_mut()).map_err(AppError::from)?;

    // Merge updates into resources by resource_id.
    let update_map = merge_updates(all_updates);

    // Build JSON output combining resource + update data, applying filters.
    let results: Vec<serde_json::Value> = build_network_entries(&all_resources, &update_map)
        .into_iter()
        .filter(|entry| {
            if let Some(f) = filter {
                let url = entry["url"].as_str().unwrap_or_default();
                if !url.contains(f) {
                    return false;
                }
            }
            if let Some(m) = method {
                let entry_method = entry["method"].as_str().unwrap_or_default();
                if !entry_method.eq_ignore_ascii_case(m) {
                    return false;
                }
            }
            true
        })
        .collect();

    // Unwatch to clean up server-side resources.
    let _ =
        WatcherActor::unwatch_resources(ctx.transport_mut(), &watcher_actor, &["network-event"]);

    let total = results.len();
    let results_json = json!(results);
    let meta = json!({"host": cli.host, "port": cli.port});
    let envelope = output::envelope(&results_json, total, &meta);

    OutputPipeline::new(cli.jq.clone())
        .finalize(&envelope)
        .map_err(AppError::from)
}

/// JavaScript snippet evaluated in the page to extract Performance Resource Timing entries.
const PERF_TIMING_SCRIPT: &str = r#"JSON.stringify(performance.getEntriesByType("resource").map(e => ({
  name: e.name,
  initiatorType: e.initiatorType,
  duration: Math.round(e.duration * 100) / 100,
  transferSize: e.transferSize,
  encodedBodySize: e.encodedBodySize,
  decodedBodySize: e.decodedBodySize,
  startTime: Math.round(e.startTime * 100) / 100,
  responseEnd: Math.round(e.responseEnd * 100) / 100,
  protocol: e.nextHopProtocol
})))"#;

/// Run the network command in cached mode using the Performance Resource Timing API.
///
/// This path evaluates JavaScript in the tab to retrieve resources already loaded
/// by the page, without subscribing to the WatcherActor. The `--method` flag is
/// silently ignored because the Performance API does not expose HTTP method.
pub fn run_cached(cli: &Cli, filter: Option<&str>) -> Result<(), AppError> {
    let mut ctx = connect_and_get_target(cli)?;
    let console_actor = ctx.target.console_actor.clone();

    let eval_result =
        WebConsoleActor::evaluate_js_async(ctx.transport_mut(), &console_actor, PERF_TIMING_SCRIPT)
            .map_err(AppError::from)?;

    // If the evaluation threw, surface the error.
    if let Some(ref exc) = eval_result.exception {
        let msg = exc
            .message
            .as_deref()
            .unwrap_or("performance timing evaluation threw an exception");
        return Err(AppError::User(format!("network --cached: {msg}")));
    }

    // The result is a JSON string produced by JSON.stringify inside the script.
    // For pages with many resources, Firefox returns a LongString grip that
    // must be fetched in full via the StringActor.
    let json_str = match &eval_result.result {
        Grip::Value(serde_json::Value::String(s)) => s.clone(),
        Grip::LongString {
            actor,
            length,
            initial: _,
        } => LongStringActor::full_string(ctx.transport_mut(), actor.as_ref(), *length)
            .map_err(AppError::from)?,
        other => {
            return Err(AppError::User(format!(
                "network --cached: expected string result, got: {}",
                other.to_json()
            )));
        }
    };

    // Parse the JSON array of resource timing entries.
    let entries: Vec<serde_json::Value> = serde_json::from_str(&json_str).map_err(|e| {
        AppError::User(format!(
            "network --cached: failed to parse performance timing JSON: {e}"
        ))
    })?;

    // Apply URL filter and map to output shape.
    let results: Vec<serde_json::Value> = entries
        .into_iter()
        .filter(|entry| {
            if let Some(f) = filter {
                let name = entry.get("name").and_then(serde_json::Value::as_str).unwrap_or_default();
                if !name.contains(f) {
                    return false;
                }
            }
            true
        })
        .map(|entry| {
            json!({
                "url": entry.get("name").cloned().unwrap_or(serde_json::Value::Null),
                "initiator_type": entry.get("initiatorType").cloned().unwrap_or(serde_json::Value::Null),
                "duration_ms": entry.get("duration").cloned().unwrap_or(serde_json::Value::Null),
                "transfer_size": entry.get("transferSize").cloned().unwrap_or(serde_json::Value::Null),
                "encoded_size": entry.get("encodedBodySize").cloned().unwrap_or(serde_json::Value::Null),
                "decoded_size": entry.get("decodedBodySize").cloned().unwrap_or(serde_json::Value::Null),
                "start_time_ms": entry.get("startTime").cloned().unwrap_or(serde_json::Value::Null),
                "response_end_ms": entry.get("responseEnd").cloned().unwrap_or(serde_json::Value::Null),
                "protocol": entry.get("protocol").cloned().unwrap_or(serde_json::Value::Null),
            })
        })
        .collect();

    let total = results.len();
    let results_json = json!(results);
    let meta = json!({"host": cli.host, "port": cli.port});
    let envelope = output::envelope(&results_json, total, &meta);

    OutputPipeline::new(cli.jq.clone())
        .finalize(&envelope)
        .map_err(AppError::from)
}
