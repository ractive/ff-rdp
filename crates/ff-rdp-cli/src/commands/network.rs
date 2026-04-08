use std::collections::HashMap;
use std::io::Write;

use ff_rdp_core::{
    NetworkResource, ProtocolError, RdpTransport, TabActor, WatcherActor,
    parse_network_resource_updates, parse_network_resources,
};
use serde_json::{Value, json};

use crate::cli::args::Cli;
use crate::error::AppError;
use crate::output;
use crate::output_controls::{OutputControls, SortDir};
use crate::output_pipeline::OutputPipeline;

use super::connect_tab::{ConnectedTab, connect_and_get_target};
use super::network_events::{
    build_network_entries, drain_network_events, drain_network_from_daemon, merge_updates,
    performance_api_fallback,
};

pub fn run(cli: &Cli, filter: Option<&str>, method: Option<&str>) -> Result<(), AppError> {
    let mut ctx = connect_and_get_target(cli)?;

    let (all_resources, all_updates) = if ctx.via_daemon {
        // The daemon has already subscribed to network-event resources and is
        // buffering them.  Drain the buffer without touching watcher state.
        drain_network_from_daemon(ctx.transport_mut())?
    } else {
        let tab_actor = ctx.target_tab_actor().clone();

        // Get the watcher actor for resource subscriptions.
        let watcher_actor =
            TabActor::get_watcher(ctx.transport_mut(), &tab_actor).map_err(AppError::from)?;

        // Subscribe to network events. The watchResources response from Firefox
        // 149+ includes existing network events as a `resources` field in the
        // ack itself (not as separate resources-available-array events).  We
        // parse the ack for inline resources, then drain for any subsequent
        // events (updates, late-arriving resources).
        WatcherActor::watch_resources(ctx.transport_mut(), &watcher_actor, &["network-event"])
            .map_err(AppError::from)?;

        // Collect resource events until timeout.
        let result = drain_network_events(ctx.transport_mut()).map_err(AppError::from)?;

        // Unwatch to clean up server-side resources.
        let _ = WatcherActor::unwatch_resources(
            ctx.transport_mut(),
            &watcher_actor,
            &["network-event"],
        );

        result
    };

    // Merge updates into resources by resource_id.
    let update_map = merge_updates(all_updates);

    // Build JSON output combining resource + update data.
    let apply_filters = |entries: Vec<serde_json::Value>| -> Vec<serde_json::Value> {
        entries
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
            .collect()
    };

    let watcher_entries = build_network_entries(&all_resources, &update_map);
    let watcher_was_empty = watcher_entries.is_empty();
    let filtered_watcher = apply_filters(watcher_entries);

    // If the watcher returned nothing (page already loaded before subscribing),
    // try the Performance API as a fallback.  In daemon mode the watcher buffer
    // may be empty because the page loaded before our drain — the Performance
    // API fallback applies equally in that case.
    let (results, used_perf_fallback) = if watcher_was_empty {
        let fallback = performance_api_fallback(&mut ctx);
        let filtered_fallback = apply_filters(fallback);
        let used = !filtered_fallback.is_empty();
        (filtered_fallback, used)
    } else {
        (filtered_watcher, false)
    };

    // When both the watcher and the Performance API returned nothing, print a
    // hint so the user knows how to get data.
    if results.is_empty() && watcher_was_empty {
        eprintln!(
            "hint: no network events captured. \
             Navigate first or use `--follow` to stream events in real time."
        );
    }

    let meta = if used_perf_fallback {
        json!({"host": cli.host, "port": cli.port, "source": "performance-api"})
    } else {
        json!({"host": cli.host, "port": cli.port})
    };

    // Decide whether to show summary or detail mode.
    // Detail mode is used when:
    //   - --detail flag is set
    //   - --jq is set (user wants raw data to process)
    //   - --sort, --limit, --fields are explicitly set (user wants detail controls)
    let use_detail = cli.detail
        || cli.jq.is_some()
        || cli.sort.is_some()
        || cli.limit.is_some()
        || cli.all
        || cli.fields.is_some();

    if use_detail {
        let controls = OutputControls::from_cli(cli, SortDir::Desc);
        let mut detail = results;
        // Default sort by duration_ms desc when no explicit sort is provided.
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
        let shown = limited.len();
        let limited = controls.apply_fields(limited);
        let envelope =
            output::envelope_with_truncation(&json!(limited), shown, total, truncated, &meta);
        return OutputPipeline::from_cli(cli)?
            .finalize(&envelope)
            .map_err(AppError::from);
    }

    // Summary mode (default).
    let summary = build_network_summary(&results);
    let envelope = output::envelope(&summary, 1, &meta);
    OutputPipeline::from_cli(cli)?
        .finalize(&envelope)
        .map_err(AppError::from)
}

/// Build a summary view of network requests.
///
/// Returns a JSON object with:
/// - `total_requests`: total count
/// - `total_transfer_bytes`: sum of `transfer_size` across all entries
/// - `by_cause_type`: count per `cause_type` field
/// - `slowest`: top-20 slowest requests (url, duration_ms, status, transfer_size)
pub fn build_network_summary(entries: &[serde_json::Value]) -> serde_json::Value {
    let total_requests = entries.len();

    let total_transfer_bytes: f64 = entries
        .iter()
        .filter_map(|e| e["transfer_size"].as_f64())
        .sum();

    // Normalise -0.0 → 0.0: IEEE 754 defines -0.0 == 0.0, so this is safe.
    // An empty (or all-null) entries slice sums to 0.0 but floating-point
    // addition can produce negative zero in some edge cases.
    let total_transfer_bytes = if total_transfer_bytes == 0.0 {
        0.0_f64
    } else {
        total_transfer_bytes
    };

    let mut by_cause_type: std::collections::BTreeMap<String, usize> =
        std::collections::BTreeMap::new();
    for entry in entries {
        let cause = entry["cause_type"].as_str().unwrap_or("other").to_string();
        *by_cause_type.entry(cause).or_insert(0) += 1;
    }

    let mut sorted_by_duration: Vec<&serde_json::Value> = entries.iter().collect();
    sorted_by_duration.sort_by(|a, b| {
        let da = a["duration_ms"].as_f64().unwrap_or(0.0);
        let db = b["duration_ms"].as_f64().unwrap_or(0.0);
        db.partial_cmp(&da).unwrap_or(std::cmp::Ordering::Equal)
    });

    let slowest: Vec<serde_json::Value> = sorted_by_duration
        .iter()
        .take(20)
        .map(|e| {
            json!({
                "url": e["url"],
                "duration_ms": e["duration_ms"],
                "status": e["status"],
                "transfer_size": e["transfer_size"],
            })
        })
        .collect();

    json!({
        "total_requests": total_requests,
        "total_transfer_bytes": total_transfer_bytes,
        "by_cause_type": by_cause_type,
        "slowest": slowest,
    })
}

/// Stream network events in real time.
///
/// Subscribes to `network-event` resources via the WatcherActor (direct mode)
/// or daemon stream protocol (daemon mode), then loops reading events and
/// printing each entry as a JSON line (NDJSON) to stdout.
///
/// Both request arrivals (`resources-available-array`) and response completions
/// (`resources-updated-array`) are emitted.  Each request appears first with
/// `event: "request"`, then again with `event: "response"` once the response
/// arrives.
///
/// Exits cleanly when the connection is closed (e.g. Firefox exits).
pub fn run_follow(cli: &Cli, filter: Option<&str>, method: Option<&str>) -> Result<(), AppError> {
    let mut ctx = connect_and_get_target(cli)?;
    if ctx.via_daemon {
        run_follow_daemon(&mut ctx, filter, method, cli.jq.as_deref())
    } else {
        run_follow_direct(&mut ctx, filter, method, cli.jq.as_deref())
    }
}

fn run_follow_direct(
    ctx: &mut ConnectedTab,
    filter: Option<&str>,
    method: Option<&str>,
    jq_filter: Option<&str>,
) -> Result<(), AppError> {
    let tab_actor = ctx.target_tab_actor().clone();
    let watcher_actor =
        TabActor::get_watcher(ctx.transport_mut(), &tab_actor).map_err(AppError::from)?;

    WatcherActor::watch_resources(ctx.transport_mut(), &watcher_actor, &["network-event"])
        .map_err(AppError::from)?;

    let result = network_follow_loop(ctx.transport_mut(), filter, method, jq_filter);

    // Best-effort cleanup — ignore errors since we may be exiting anyway.
    let _ =
        WatcherActor::unwatch_resources(ctx.transport_mut(), &watcher_actor, &["network-event"]);

    result
}

fn run_follow_daemon(
    ctx: &mut ConnectedTab,
    filter: Option<&str>,
    method: Option<&str>,
    jq_filter: Option<&str>,
) -> Result<(), AppError> {
    use crate::daemon::client::{start_daemon_stream, stop_daemon_stream};

    start_daemon_stream(ctx.transport_mut(), "network-event").map_err(AppError::from)?;

    let result = network_follow_loop(ctx.transport_mut(), filter, method, jq_filter);

    // Best-effort cleanup — ignore errors since we may be exiting anyway.
    let _ = stop_daemon_stream(ctx.transport_mut(), "network-event");

    result
}

/// Emit a single NDJSON line for `entry`, applying `jq_filter` if set.
fn emit_ndjson(entry: &Value, jq_filter: Option<&str>) -> Result<(), AppError> {
    if let Some(filter) = jq_filter {
        let values = output::apply_jq_filter(entry, filter).map_err(AppError::from)?;
        for v in values {
            println!(
                "{}",
                serde_json::to_string(&v).map_err(|e| AppError::Internal(e.into()))?
            );
        }
    } else {
        println!(
            "{}",
            serde_json::to_string(entry).map_err(|e| AppError::Internal(e.into()))?
        );
    }
    Ok(())
}

/// Inner loop for `--follow` mode.
///
/// Maintains a map of in-flight requests keyed by `resource_id`.  When a
/// `resources-available-array` message arrives, each resource is emitted with
/// `event: "request"` (after filter/method checks) and stored in `pending`.
/// When a `resources-updated-array` message arrives, matching entries from
/// `pending` are emitted with `event: "response"`.
fn network_follow_loop(
    transport: &mut RdpTransport,
    filter: Option<&str>,
    method: Option<&str>,
    jq_filter: Option<&str>,
) -> Result<(), AppError> {
    // Track in-flight requests so we can correlate updates with their requests.
    // Only resources that pass the filters are stored here.
    let mut pending: HashMap<u64, NetworkResource> = HashMap::new();

    loop {
        match transport.recv() {
            Ok(msg) => {
                let msg_type = msg.get("type").and_then(Value::as_str).unwrap_or_default();
                match msg_type {
                    "resources-available-array" => {
                        let resources = parse_network_resources(&msg);
                        for res in resources {
                            // Apply filters before emitting or tracking.
                            if let Some(f) = filter
                                && !res.url.contains(f)
                            {
                                continue;
                            }
                            if let Some(m) = method
                                && !res.method.eq_ignore_ascii_case(m)
                            {
                                continue;
                            }
                            let entry = json!({
                                "event": "request",
                                "method": res.method,
                                "url": res.url,
                                "is_xhr": res.is_xhr,
                                "cause_type": res.cause_type,
                                "resource_id": res.resource_id,
                            });
                            emit_ndjson(&entry, jq_filter)?;
                            let _ = std::io::stdout().flush();
                            pending.insert(res.resource_id, res);
                        }
                    }
                    "resources-updated-array" => {
                        let updates = parse_network_resource_updates(&msg);
                        for update in updates {
                            // Only emit updates for requests that passed the filters.
                            // Remove from pending so memory doesn't grow without bound.
                            let Some(res) = pending.remove(&update.resource_id) else {
                                continue;
                            };
                            let mut entry = json!({
                                "event": "response",
                                "method": res.method,
                                "url": res.url,
                                "is_xhr": res.is_xhr,
                                "cause_type": res.cause_type,
                                "resource_id": update.resource_id,
                            });
                            if let Some(ref status) = update.status {
                                if let Ok(code) = status.parse::<u16>() {
                                    entry["status"] = json!(code);
                                } else {
                                    entry["status"] = json!(status);
                                }
                            }
                            if let Some(ref mime) = update.mime_type {
                                entry["content_type"] = json!(mime);
                            }
                            if let Some(total) = update.total_time {
                                entry["duration_ms"] = json!(total);
                            }
                            if let Some(size) = update.content_size {
                                entry["size_bytes"] = json!(size);
                            }
                            if let Some(transferred) = update.transferred_size {
                                entry["transfer_size"] = json!(transferred);
                            }
                            emit_ndjson(&entry, jq_filter)?;
                            let _ = std::io::stdout().flush();
                        }
                    }
                    _ => {}
                }
            }
            Err(ProtocolError::Timeout) => {
                // Normal poll timeout — keep waiting for more events.
            }
            Err(ProtocolError::RecvFailed(ref e))
                if e.kind() == std::io::ErrorKind::UnexpectedEof
                    || e.kind() == std::io::ErrorKind::ConnectionReset
                    || e.kind() == std::io::ErrorKind::BrokenPipe =>
            {
                // Connection closed cleanly (Firefox exited, daemon stopped, etc.).
                return Ok(());
            }
            Err(e) => return Err(AppError::from(e)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn build_network_summary_empty() {
        let s = build_network_summary(&[]);
        assert_eq!(s["total_requests"], 0);
        assert_eq!(s["total_transfer_bytes"], 0.0);
        assert!(s["slowest"].as_array().unwrap().is_empty());
    }

    #[test]
    fn build_network_summary_total_transfer_bytes_not_negative_zero() {
        // An empty slice sums to 0.0; IEEE 754 can sometimes produce -0.0.
        // Verify the returned value serialises as "0.0" (positive zero) and
        // that the IEEE bit pattern is positive zero, not negative zero.
        let s = build_network_summary(&[]);
        let v = s["total_transfer_bytes"]
            .as_f64()
            .expect("total_transfer_bytes is f64");
        assert!(v == 0.0, "expected 0.0, got {v}");
        // f64::is_sign_negative distinguishes -0.0 from +0.0.
        assert!(
            !v.is_sign_negative(),
            "total_transfer_bytes should be positive zero, not negative zero"
        );
        // Serialised form must not contain a minus sign.
        let json_str = serde_json::to_string(&s["total_transfer_bytes"]).unwrap();
        assert!(
            !json_str.starts_with('-'),
            "serialised total_transfer_bytes should not start with '-', got {json_str:?}"
        );
    }

    #[test]
    fn build_network_summary_null_transfer_sizes_give_zero_not_negative_zero() {
        // Entries where transfer_size is null contribute nothing to the sum.
        // The result must be positive 0.0, not -0.0.
        let entries = vec![
            json!({"url": "a", "duration_ms": 10.0, "status": 200, "cause_type": "doc"}),
            json!({"url": "b", "duration_ms": 20.0, "status": 200, "cause_type": "doc"}),
        ];
        let s = build_network_summary(&entries);
        let v = s["total_transfer_bytes"]
            .as_f64()
            .expect("total_transfer_bytes is f64");
        assert!(v == 0.0, "expected 0.0, got {v}");
        assert!(!v.is_sign_negative(), "should be +0.0, not -0.0");
    }

    #[test]
    fn build_network_summary_counts_and_bytes() {
        let entries = vec![
            json!({"url": "a", "duration_ms": 100.0, "status": 200, "transfer_size": 500.0, "cause_type": "script"}),
            json!({"url": "b", "duration_ms": 50.0, "status": 404, "transfer_size": 100.0, "cause_type": "script"}),
            json!({"url": "c", "duration_ms": 200.0, "status": 200, "transfer_size": 1000.0, "cause_type": "img"}),
        ];
        let s = build_network_summary(&entries);
        assert_eq!(s["total_requests"], 3);
        assert_eq!(s["total_transfer_bytes"], 1600.0);
        assert_eq!(s["by_cause_type"]["script"], 2);
        assert_eq!(s["by_cause_type"]["img"], 1);
        // Slowest first: c (200ms), a (100ms), b (50ms)
        let slowest = s["slowest"].as_array().unwrap();
        assert_eq!(slowest[0]["url"], "c");
        assert_eq!(slowest[1]["url"], "a");
        assert_eq!(slowest[2]["url"], "b");
    }
}
