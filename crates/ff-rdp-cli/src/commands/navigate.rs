use ff_rdp_core::{TabActor, WatcherActor, WindowGlobalTarget};
use serde_json::json;

use crate::cli::args::Cli;
use crate::error::AppError;
use crate::output;
use crate::output_pipeline::OutputPipeline;

use super::connect_tab::connect_and_get_target;
use super::network_events::{
    build_network_entries, drain_network_events, drain_network_from_daemon, merge_updates,
};

pub fn run(cli: &Cli, url: &str) -> Result<(), AppError> {
    let mut ctx = connect_and_get_target(cli)?;
    let target_actor = ctx.target.actor.clone();

    WindowGlobalTarget::navigate_to(ctx.transport_mut(), &target_actor, url)
        .map_err(AppError::from)?;

    let result = json!({"navigated": url});
    let meta = json!({"host": cli.host, "port": cli.port});
    let envelope = output::envelope(&result, 1, &meta);

    OutputPipeline::new(cli.jq.clone())
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
/// 8. Emit combined JSON output.
pub fn run_with_network(cli: &Cli, url: &str) -> Result<(), AppError> {
    let mut ctx = connect_and_get_target(cli)?;
    let target_actor = ctx.target.actor.clone();

    if ctx.via_daemon {
        // The daemon has already subscribed to network-event resources.
        // Navigate first, then drain the daemon buffer for events from this
        // navigation.  The daemon continues buffering after the drain so
        // subsequent commands see events from future navigations too.
        WindowGlobalTarget::navigate_to(ctx.transport_mut(), &target_actor, url)
            .map_err(AppError::from)?;

        let (all_resources, all_updates) = drain_network_from_daemon(ctx.transport_mut())?;

        let update_map = merge_updates(all_updates);
        let network_entries = build_network_entries(&all_resources, &update_map);

        let total = network_entries.len();
        let result = json!({
            "navigated": url,
            "network": network_entries,
        });
        let meta = json!({"host": cli.host, "port": cli.port});
        let envelope = output::envelope(&result, total, &meta);
        return OutputPipeline::new(cli.jq.clone())
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

    // Navigate to the target URL.
    WindowGlobalTarget::navigate_to(ctx.transport_mut(), &target_actor, url)
        .map_err(AppError::from)?;

    // Drain resource events until the timeout fires (no more events).
    let (all_resources, all_updates) =
        drain_network_events(ctx.transport_mut()).map_err(AppError::from)?;

    // Merge updates into resources by resource_id.
    let update_map = merge_updates(all_updates);

    // Build the network entries array (no URL/method filtering here).
    let network_entries = build_network_entries(&all_resources, &update_map);

    // Unwatch to clean up server-side resources.
    let _ =
        WatcherActor::unwatch_resources(ctx.transport_mut(), &watcher_actor, &["network-event"]);

    let total = network_entries.len();
    let result = json!({
        "navigated": url,
        "network": network_entries,
    });
    let meta = json!({"host": cli.host, "port": cli.port});
    let envelope = output::envelope(&result, total, &meta);

    OutputPipeline::new(cli.jq.clone())
        .finalize(&envelope)
        .map_err(AppError::from)
}
