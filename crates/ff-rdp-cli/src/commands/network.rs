use ff_rdp_core::{TabActor, WatcherActor};
use serde_json::json;

use crate::cli::args::Cli;
use crate::error::AppError;
use crate::output;
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

        // Subscribe to network events. This triggers Firefox to send existing
        // network events as `resources-available-array` messages.
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

    let total = results.len();
    let results_json = json!(results);
    let meta = json!({"host": cli.host, "port": cli.port});
    let envelope = output::envelope(&results_json, total, &meta);

    OutputPipeline::new(cli.jq.clone())
        .finalize(&envelope)
        .map_err(AppError::from)
}
