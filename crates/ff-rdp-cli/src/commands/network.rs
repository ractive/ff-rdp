use std::collections::HashMap;

use ff_rdp_core::{
    TabActor, WatcherActor, parse_network_resource_updates, parse_network_resources,
};
use serde_json::json;

use crate::cli::args::Cli;
use crate::error::AppError;
use crate::output;
use crate::output_pipeline::OutputPipeline;

use super::connect_tab::connect_and_get_target;

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

    // Collect resource events. After watchResources responds, Firefox sends
    // resource-available-array and resource-updated-array events. We read
    // with a short timeout until no more events arrive.
    let mut all_resources = Vec::new();
    let mut all_updates = Vec::new();

    loop {
        match ctx.transport_mut().recv() {
            Ok(msg) => {
                let msg_type = msg
                    .get("type")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default();

                match msg_type {
                    "resources-available-array" => {
                        all_resources.extend(parse_network_resources(&msg));
                    }
                    "resources-updated-array" => {
                        all_updates.extend(parse_network_resource_updates(&msg));
                    }
                    _ => {}
                }
            }
            Err(ff_rdp_core::ProtocolError::Timeout) => break,
            Err(e) => return Err(AppError::from(e)),
        }
    }

    // Merge updates into resources by resource_id.
    // Take the latest update for each resource_id (updates come in order).
    let mut update_map: HashMap<u64, ff_rdp_core::NetworkResourceUpdate> = HashMap::new();
    for update in all_updates {
        let entry = update_map.entry(update.resource_id).or_default();
        // Merge: later updates fill in fields that earlier updates may not have.
        if update.status.is_some() {
            entry.status = update.status;
        }
        if update.http_version.is_some() {
            entry.http_version = update.http_version;
        }
        if update.mime_type.is_some() {
            entry.mime_type = update.mime_type;
        }
        if update.total_time.is_some() {
            entry.total_time = update.total_time;
        }
        if update.content_size.is_some() {
            entry.content_size = update.content_size;
        }
        if update.transferred_size.is_some() {
            entry.transferred_size = update.transferred_size;
        }
        if update.from_cache.is_some() {
            entry.from_cache = update.from_cache;
        }
        if update.remote_address.is_some() {
            entry.remote_address.clone_from(&update.remote_address);
        }
        if update.security_state.is_some() {
            entry.security_state.clone_from(&update.security_state);
        }
    }

    // Build JSON output combining resource + update data.
    let results: Vec<serde_json::Value> = all_resources
        .iter()
        .filter(|res| {
            if let Some(f) = filter
                && !res.url.contains(f)
            {
                return false;
            }
            if let Some(m) = method
                && !res.method.eq_ignore_ascii_case(m)
            {
                return false;
            }
            true
        })
        .map(|res| {
            let update = update_map.get(&res.resource_id);
            let mut entry = json!({
                "method": res.method,
                "url": res.url,
                "is_xhr": res.is_xhr,
                "cause_type": res.cause_type,
                "content_type": null,
            });
            if let Some(u) = update {
                if let Some(ref status) = u.status
                    && let Ok(code) = status.parse::<u16>()
                {
                    entry["status"] = json!(code);
                }
                if let Some(ref mime) = u.mime_type {
                    entry["content_type"] = json!(mime);
                }
                if let Some(total) = u.total_time {
                    entry["duration_ms"] = json!(total);
                }
                if let Some(size) = u.content_size {
                    entry["size_bytes"] = json!(size);
                }
            }
            entry
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
