use std::collections::HashMap;

use ff_rdp_core::transport::RdpTransport;
use ff_rdp_core::{
    Grip, LongStringActor, NetworkResource, NetworkResourceUpdate, ProtocolError, WebConsoleActor,
    parse_network_resource_updates, parse_network_resources,
};
use serde_json::{Value, json};

use crate::daemon::client::drain_daemon_events;
use crate::error::AppError;

/// Drain `resources-available-array` and `resources-updated-array` events from
/// the transport until a [`ProtocolError::Timeout`] occurs, then return the
/// collected resources and update entries.
///
/// This is the common event-drain used by both the `network` command and the
/// `navigate --with-network` command.
pub(crate) fn drain_network_events(
    transport: &mut RdpTransport,
) -> Result<(Vec<NetworkResource>, Vec<NetworkResourceUpdate>), ProtocolError> {
    let mut all_resources = Vec::new();
    let mut all_updates = Vec::new();

    loop {
        match transport.recv() {
            Ok(msg) => {
                let msg_type = msg.get("type").and_then(Value::as_str).unwrap_or_default();

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
            Err(ProtocolError::Timeout) => break,
            Err(e) => return Err(e),
        }
    }

    Ok((all_resources, all_updates))
}

/// Merge a list of [`NetworkResourceUpdate`] entries by `resource_id`, folding
/// later values over earlier ones so that the last-seen value for each field wins.
pub(crate) fn merge_updates(
    all_updates: Vec<NetworkResourceUpdate>,
) -> HashMap<u64, NetworkResourceUpdate> {
    let mut update_map: HashMap<u64, NetworkResourceUpdate> = HashMap::new();
    for update in all_updates {
        let entry = update_map.entry(update.resource_id).or_default();
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
    update_map
}

/// Drain buffered network events from the daemon and split them into
/// available resources and update entries.
///
/// The daemon stores individual items from both `resources-available-array`
/// (items with an `actor` field) and `resources-updated-array` (items with a
/// `resourceUpdates` field) in a single buffer keyed by `"network-event"`.
/// This function separates them and reconstructs the wrapper format expected
/// by [`parse_network_resources`] and [`parse_network_resource_updates`].
pub(crate) fn drain_network_from_daemon(
    transport: &mut RdpTransport,
) -> Result<(Vec<NetworkResource>, Vec<NetworkResourceUpdate>), AppError> {
    let drained = drain_daemon_events(transport, "network-event").map_err(AppError::from)?;

    let mut available_items: Vec<Value> = Vec::new();
    let mut update_items: Vec<Value> = Vec::new();
    for item in drained {
        if item.get("resourceUpdates").is_some() {
            update_items.push(item);
        } else {
            available_items.push(item);
        }
    }

    // Reconstruct the wrapper format so the existing parsers can be reused.
    let available_msg = json!({"array": [["network-event", available_items]]});
    let update_msg = json!({"array": [["network-event", update_items]]});

    let resources = parse_network_resources(&available_msg);
    let resource_updates = parse_network_resource_updates(&update_msg);

    Ok((resources, resource_updates))
}

/// Map a single PerformanceResourceTiming JSON entry (from `performance.getEntriesByType`)
/// to the same JSON shape produced by [`build_network_entries`].
pub(crate) fn map_perf_resource_to_network_entry(entry: &Value) -> Value {
    let url = entry.get("name").cloned().unwrap_or(Value::Null);
    let initiator_type = entry
        .get("initiatorType")
        .and_then(Value::as_str)
        .unwrap_or("");
    let is_xhr = initiator_type == "xmlhttprequest" || initiator_type == "fetch";

    let duration = entry.get("duration").cloned().unwrap_or(Value::Null);

    let size_bytes = entry
        .get("decodedBodySize")
        .and_then(Value::as_f64)
        .filter(|&v| v > 0.0)
        .map_or(Value::Null, |v| json!(v));

    let transfer_size = entry
        .get("transferSize")
        .and_then(Value::as_f64)
        .filter(|&v| v > 0.0)
        .map_or(Value::Null, |v| json!(v));

    json!({
        "method": "GET",
        "url": url,
        "is_xhr": is_xhr,
        "cause_type": initiator_type,
        "content_type": null,
        "duration_ms": duration,
        "size_bytes": size_bytes,
        "transfer_size": transfer_size,
        "status": null,
        "source": "performance-api",
    })
}

/// Evaluate `performance.getEntriesByType('resource')` in the page via JS and
/// return the entries mapped to the same JSON shape as [`build_network_entries`].
///
/// Returns `Ok(vec![])` on any failure — this is a best-effort fallback only.
pub(crate) fn performance_api_fallback(ctx: &mut super::connect_tab::ConnectedTab) -> Vec<Value> {
    const SCRIPT: &str =
        "JSON.stringify(performance.getEntriesByType('resource').map(e => e.toJSON()))";

    let console_actor = ctx.target.console_actor.clone();
    let Ok(eval_result) =
        WebConsoleActor::evaluate_js_async(ctx.transport_mut(), &console_actor, SCRIPT)
    else {
        return vec![];
    };

    // If the eval threw an exception treat it as an empty result.
    if eval_result.exception.is_some() {
        return vec![];
    }

    // The result is a JSON string — possibly a LongString grip for large pages.
    let json_str = match &eval_result.result {
        Grip::Value(Value::String(s)) => s.clone(),
        Grip::LongString {
            actor,
            length,
            initial: _,
        } => match LongStringActor::full_string(ctx.transport_mut(), actor.as_ref(), *length) {
            Ok(s) => s,
            Err(_) => return vec![],
        },
        _ => return vec![],
    };

    let Ok(entries) = serde_json::from_str::<Vec<Value>>(&json_str) else {
        return vec![];
    };

    entries
        .iter()
        .map(map_perf_resource_to_network_entry)
        .collect()
}

/// Build the JSON array of network entries combining resource + update data.
///
/// Applies the same field mapping used by the `network` command output.
pub(crate) fn build_network_entries(
    resources: &[NetworkResource],
    update_map: &HashMap<u64, NetworkResourceUpdate>,
) -> Vec<Value> {
    resources
        .iter()
        .map(|res| {
            let update = update_map.get(&res.resource_id);
            let mut entry = serde_json::json!({
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
                    entry["status"] = serde_json::json!(code);
                }
                if let Some(ref mime) = u.mime_type {
                    entry["content_type"] = serde_json::json!(mime);
                }
                if let Some(total) = u.total_time {
                    entry["duration_ms"] = serde_json::json!(total);
                }
                if let Some(size) = u.content_size {
                    entry["size_bytes"] = serde_json::json!(size);
                }
            }
            entry
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn map_perf_resource_xhr_initiator_type() {
        let entry = json!({
            "name": "https://example.com/api/data",
            "initiatorType": "xmlhttprequest",
            "duration": 123.4,
            "decodedBodySize": 2048,
            "transferSize": 2100,
        });
        let result = map_perf_resource_to_network_entry(&entry);
        assert_eq!(result["method"], "GET");
        assert_eq!(result["url"], "https://example.com/api/data");
        assert_eq!(result["is_xhr"], true);
        assert_eq!(result["cause_type"], "xmlhttprequest");
        assert_eq!(result["content_type"], Value::Null);
        assert_eq!(result["duration_ms"], 123.4);
        assert_eq!(result["size_bytes"], 2048.0);
        assert_eq!(result["transfer_size"], 2100.0);
        assert_eq!(result["status"], Value::Null);
        assert_eq!(result["source"], "performance-api");
    }

    #[test]
    fn map_perf_resource_fetch_initiator_type() {
        let entry = json!({
            "name": "https://example.com/api/fetch",
            "initiatorType": "fetch",
            "duration": 50.0,
            "decodedBodySize": 512,
            "transferSize": 600,
        });
        let result = map_perf_resource_to_network_entry(&entry);
        assert_eq!(result["is_xhr"], true);
        assert_eq!(result["cause_type"], "fetch");
    }

    #[test]
    fn map_perf_resource_script_initiator_type_not_xhr() {
        let entry = json!({
            "name": "https://example.com/bundle.js",
            "initiatorType": "script",
            "duration": 200.0,
            "decodedBodySize": 40000,
            "transferSize": 12000,
        });
        let result = map_perf_resource_to_network_entry(&entry);
        assert_eq!(result["is_xhr"], false);
        assert_eq!(result["cause_type"], "script");
        assert_eq!(result["url"], "https://example.com/bundle.js");
    }

    #[test]
    fn map_perf_resource_zero_sizes_become_null() {
        let entry = json!({
            "name": "https://example.com/cached",
            "initiatorType": "img",
            "duration": 0.5,
            "decodedBodySize": 0,
            "transferSize": 0,
        });
        let result = map_perf_resource_to_network_entry(&entry);
        assert_eq!(result["size_bytes"], Value::Null);
        assert_eq!(result["transfer_size"], Value::Null);
        assert_eq!(result["duration_ms"], 0.5);
    }

    #[test]
    fn map_perf_resource_missing_size_fields_become_null() {
        let entry = json!({
            "name": "https://example.com/resource",
            "initiatorType": "link",
            "duration": 10.0,
        });
        let result = map_perf_resource_to_network_entry(&entry);
        assert_eq!(result["size_bytes"], Value::Null);
        assert_eq!(result["transfer_size"], Value::Null);
    }
}
