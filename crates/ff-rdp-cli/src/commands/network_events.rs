use std::collections::HashMap;

use ff_rdp_core::transport::RdpTransport;
use ff_rdp_core::{
    NetworkResource, NetworkResourceUpdate, ProtocolError, parse_network_resource_updates,
    parse_network_resources,
};
use serde_json::Value;

/// Drain `resources-available-array` and `resources-updated-array` events from
/// the transport until a [`ProtocolError::Timeout`] occurs, then return the
/// collected resources and update entries.
///
/// This is the common event-drain used by both the `network` command and the
/// `navigate --with-network` command.
pub fn drain_network_events(
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
pub fn merge_updates(
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

/// Build the JSON array of network entries combining resource + update data.
///
/// Applies the same field mapping used by the `network` command output.
pub fn build_network_entries(
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
