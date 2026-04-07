use ff_rdp_core::{TabActor, WatcherActor};
use serde_json::json;

use crate::cli::args::Cli;
use crate::error::AppError;
use crate::output;
use crate::output_controls::{OutputControls, SortDir};
use crate::output_pipeline::OutputPipeline;

use super::connect_tab::connect_and_get_target;
use super::network_events::{
    build_network_entries, drain_network_events, drain_network_from_daemon, merge_updates,
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

    let meta = json!({"host": cli.host, "port": cli.port});

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
        return OutputPipeline::new(cli.jq.clone())
            .finalize(&envelope)
            .map_err(AppError::from);
    }

    // Summary mode (default).
    let summary = build_network_summary(&results);
    let envelope = output::envelope(&summary, 1, &meta);
    OutputPipeline::new(cli.jq.clone())
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
