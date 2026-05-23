use std::collections::HashMap;
use std::time::{Duration, Instant};

use ff_rdp_core::transport::RdpTransport;
use ff_rdp_core::{
    Grip, LongStringActor, NetworkResource, NetworkResourceUpdate, ProtocolError, WebConsoleActor,
    parse_network_resource_updates, parse_network_resources, sanitize_for_terminal,
};
use serde_json::{Value, json};

use crate::daemon::client::drain_daemon_events_since;
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

/// Drain network events with a total time limit instead of an idle timeout.
///
/// Unlike [`drain_network_events`] which stops after a single idle timeout,
/// this function collects events for up to `total_timeout` of wall-clock time,
/// using a short per-read poll interval.  This is better for navigation where
/// events arrive in bursts with gaps between them (e.g. the initial navigation
/// request takes 1-2 seconds before any network events start flowing).
///
/// The third element of the returned tuple is `timeout_reached`: `true` when
/// the wall-clock deadline fired while events were still arriving (i.e. the
/// last `recv()` before the deadline check returned an event, not an idle
/// timeout), `false` when collection stopped because the connection went idle.
pub(crate) fn drain_network_events_timed(
    transport: &mut RdpTransport,
    total_timeout: Duration,
) -> Result<(Vec<NetworkResource>, Vec<NetworkResourceUpdate>, bool), ProtocolError> {
    let start = Instant::now();
    let poll_interval = Duration::from_millis(500);

    // Set a short read timeout for responsive polling.
    transport.set_read_timeout(Some(poll_interval))?;

    let mut all_resources = Vec::new();
    let mut all_updates = Vec::new();
    // True after a recv that returned actual data; reset to false on idle timeout.
    // When the deadline fires, this tells us whether events were still arriving.
    let mut last_recv_was_event = false;

    loop {
        // Check wall-clock deadline before each read so we stop even when
        // messages arrive faster than the poll interval (continuous traffic).
        if start.elapsed() >= total_timeout {
            break;
        }

        last_recv_was_event = false;
        match transport.recv() {
            Ok(msg) => {
                let msg_type = msg.get("type").and_then(Value::as_str).unwrap_or_default();
                match msg_type {
                    "resources-available-array" => {
                        last_recv_was_event = true;
                        all_resources.extend(parse_network_resources(&msg));
                    }
                    "resources-updated-array" => {
                        last_recv_was_event = true;
                        all_updates.extend(parse_network_resource_updates(&msg));
                    }
                    _ => {}
                }
            }
            Err(ProtocolError::Timeout) => {
                // Per-read timeout with no message — the top-of-loop check
                // will enforce the total deadline on the next iteration.
            }
            Err(e) => return Err(e),
        }
    }

    Ok((all_resources, all_updates, last_recv_was_event))
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
    drain_network_from_daemon_since(transport, 0).map(|(r, u, _)| (r, u))
}

/// Result type for [`drain_network_from_daemon_since`].
///
/// `(resources, updates, nav_boundary)` — `nav_boundary` is the JSON
/// `{sequence, url}` object from the daemon when a boundary was applied.
pub(crate) type DaemonNetworkDrainResult = (
    Vec<NetworkResource>,
    Vec<NetworkResourceUpdate>,
    Option<Value>,
);

/// Like [`drain_network_from_daemon`] but scoped to a navigation window.
///
/// `since_nav_index`:
///  - `0`  → full buffer (all navigations)
///  - `-1` → since the most-recent navigation
///  - `-2` → since second-to-last, etc.
///
/// Returns `(resources, updates, nav_boundary)` where `nav_boundary` is the
/// JSON object `{sequence, url}` from the daemon when a boundary was applied.
pub(crate) fn drain_network_from_daemon_since(
    transport: &mut RdpTransport,
    since_nav_index: i64,
) -> Result<DaemonNetworkDrainResult, AppError> {
    let (drained, boundary) =
        drain_daemon_events_since(transport, "network-event", since_nav_index)
            .map_err(AppError::from)?;

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

    Ok((resources, resource_updates, boundary))
}

/// Serialize parsed [`NetworkResource`] and [`NetworkResourceUpdate`] structs
/// back to the flat-item JSON format that the daemon buffer stores.
///
/// The daemon buffer holds individual items from both `resources-available-array`
/// and `resources-updated-array` events.  Items without a `resourceUpdates` field
/// are treated as available resources; items with it are treated as updates.
/// This function reconstructs those items from parsed structs so that
/// `navigate --with-network` can store its captured events back into the daemon
/// buffer for subsequent `ff-rdp network` reads (iter-61j G).
pub(crate) fn serialize_network_resources_for_buffer(
    resources: &[NetworkResource],
    updates: &[(u64, &NetworkResourceUpdate)],
) -> Vec<Value> {
    let mut items = Vec::with_capacity(resources.len() + updates.len());
    for r in resources {
        items.push(json!({
            "actor": r.actor.as_ref(),
            "resourceId": r.resource_id,
            "method": r.method,
            "url": r.url,
            "isXHR": r.is_xhr,
            "causeType": r.cause_type,
            "startedDateTime": r.started_date_time,
            "timeStamp": r.timestamp,
        }));
    }
    for (rid, u) in updates {
        // Reconstruct the resourceUpdates wrapper that `drain_network_from_daemon_since`
        // looks for to identify update items.
        let mut upd = json!({
            "resourceId": rid,
        });
        if let Some(ref s) = u.status {
            upd["status"] = json!(s);
        }
        if let Some(ref h) = u.http_version {
            upd["httpVersion"] = json!(h);
        }
        if let Some(ref m) = u.mime_type {
            upd["mimeType"] = json!(m);
        }
        if let Some(t) = u.total_time {
            upd["totalTime"] = json!(t);
        }
        if let Some(c) = u.content_size {
            upd["contentSize"] = json!(c);
        }
        if let Some(ts) = u.transferred_size {
            upd["transferredSize"] = json!(ts);
        }
        if let Some(fc) = u.from_cache {
            upd["fromCache"] = json!(fc);
        }
        if let Some(ref ra) = u.remote_address {
            upd["remoteAddress"] = json!(ra);
        }
        if let Some(ref ss) = u.security_state {
            upd["securityState"] = json!(ss);
        }
        // The `resourceUpdates` wrapper is what distinguishes update items from
        // resource items in `drain_network_from_daemon_since`.
        items.push(json!({"resourceUpdates": [upd]}));
    }
    items
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
        .and_then(Value::as_u64)
        .filter(|&v| v > 0)
        .map_or(Value::Null, |v| json!(v));

    let transfer_size = entry
        .get("transferSize")
        .and_then(Value::as_u64)
        .filter(|&v| v > 0)
        .map_or(Value::Null, |v| json!(v));

    json!({
        "method": null,
        "url": url,
        "is_xhr": is_xhr,
        "cause_type": initiator_type,
        "content_type": null,
        "duration_ms": duration,
        "size_bytes": size_bytes,
        "transfer_size": transfer_size,
        "status": null,
        "source": "performance-api",
        "note": "method/status not available from performance-api source",
    })
}

/// Evaluate `performance.getEntriesByType('resource')` in the page via JS and
/// return the entries mapped to the same JSON shape as [`build_network_entries`].
///
/// Returns an empty vec on any failure — this is a best-effort fallback only.
/// Errors are printed to stderr so the caller can diagnose why the fallback
/// returned nothing (e.g. daemon JS forwarding broken, page not yet loaded).
pub(crate) fn performance_api_fallback(ctx: &mut super::connect_tab::ConnectedTab) -> Vec<Value> {
    const SCRIPT: &str =
        "JSON.stringify(performance.getEntriesByType('resource').map(e => e.toJSON()))";

    let console_actor = ctx.target.console_actor.clone();
    let eval_result =
        match WebConsoleActor::evaluate_js_async(ctx.transport_mut(), &console_actor, SCRIPT) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("hint: performance-api fallback eval failed: {e:#}");
                return vec![];
            }
        };

    // If the eval threw an exception treat it as an empty result.
    if let Some(ref exc) = eval_result.exception {
        let msg = exc.message.as_deref().unwrap_or("(no message)");
        eprintln!(
            "hint: performance-api fallback JS exception: {}",
            sanitize_for_terminal(msg)
        );
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
            Err(e) => {
                eprintln!("hint: performance-api fallback failed to fetch long string: {e:#}");
                return vec![];
            }
        },
        other => {
            eprintln!("hint: performance-api fallback returned unexpected grip type: {other:?}");
            return vec![];
        }
    };

    match serde_json::from_str::<Vec<Value>>(&json_str) {
        Ok(entries) => entries
            .iter()
            .map(map_perf_resource_to_network_entry)
            .collect(),
        Err(e) => {
            eprintln!("hint: performance-api fallback failed to parse JSON result: {e:#}");
            vec![]
        }
    }
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
        .map(|res| build_single_entry(res, update_map))
        .collect()
}

/// Like [`build_network_entries`] but includes `_resource_id` in each entry
/// so callers can look up the corresponding [`NetworkEventActor`] for
/// per-entry header fetching.  The field is an internal marker and must be
/// stripped before emitting output to the user.
pub(crate) fn build_network_entries_with_ids(
    resources: &[NetworkResource],
    update_map: &HashMap<u64, NetworkResourceUpdate>,
) -> Vec<Value> {
    resources
        .iter()
        .map(|res| {
            let mut entry = build_single_entry(res, update_map);
            entry["_resource_id"] = serde_json::json!(res.resource_id);
            entry
        })
        .collect()
}

fn build_single_entry(
    res: &NetworkResource,
    update_map: &HashMap<u64, NetworkResourceUpdate>,
) -> Value {
    let update = update_map.get(&res.resource_id);
    let mut entry = serde_json::json!({
        "method": res.method,
        "url": res.url,
        "is_xhr": res.is_xhr,
        "cause_type": res.cause_type,
        "content_type": null,
        "source": "watcher",
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
        if let Some(transferred) = u.transferred_size {
            entry["transfer_size"] = serde_json::json!(transferred);
        }
    }
    entry
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(result["method"], Value::Null);
        assert_eq!(result["url"], "https://example.com/api/data");
        assert_eq!(result["is_xhr"], true);
        assert_eq!(result["cause_type"], "xmlhttprequest");
        assert_eq!(result["content_type"], Value::Null);
        assert_eq!(result["duration_ms"], 123.4);
        assert_eq!(result["size_bytes"], 2048);
        assert_eq!(result["transfer_size"], 2100);
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

    #[test]
    fn build_network_entries_with_ids_includes_resource_id() {
        use ff_rdp_core::{ActorId, NetworkResource, NetworkResourceUpdate};

        let res = NetworkResource {
            actor: ActorId::from("server1.conn0.netEvent1"),
            method: "POST".to_string(),
            url: "https://example.com/api".to_string(),
            is_xhr: true,
            cause_type: "fetch".to_string(),
            started_date_time: "2026-01-01T00:00:00Z".to_string(),
            timestamp: 0.0,
            resource_id: 42,
        };
        let update = NetworkResourceUpdate {
            resource_id: 42,
            status: Some("200".to_string()),
            total_time: Some(100),
            ..Default::default()
        };
        let update_map = std::collections::HashMap::from([(42u64, update)]);
        let entries = build_network_entries_with_ids(&[res], &update_map);
        assert_eq!(entries.len(), 1);
        // The _resource_id field must be present for header fetching.
        assert_eq!(entries[0]["_resource_id"], 42u64);
        // Regular fields are also present.
        assert_eq!(entries[0]["method"], "POST");
        assert_eq!(entries[0]["url"], "https://example.com/api");
        assert_eq!(entries[0]["status"], 200);
    }

    #[test]
    fn build_network_entries_without_ids_excludes_resource_id() {
        use ff_rdp_core::{ActorId, NetworkResource};

        let res = NetworkResource {
            actor: ActorId::from("server1.conn0.netEvent2"),
            method: "GET".to_string(),
            url: "https://example.com/".to_string(),
            is_xhr: false,
            cause_type: "doc".to_string(),
            started_date_time: "2026-01-01T00:00:00Z".to_string(),
            timestamp: 0.0,
            resource_id: 99,
        };
        let entries = build_network_entries(&[res], &std::collections::HashMap::new());
        assert!(
            entries[0].get("_resource_id").is_none(),
            "build_network_entries must not include _resource_id"
        );
    }

    #[test]
    fn map_perf_resource_method_and_status_are_null_not_hardcoded() {
        let entry = json!({
            "name": "https://example.com/data.json",
            "initiatorType": "fetch",
            "duration": 30.0,
        });
        let result = map_perf_resource_to_network_entry(&entry);
        // B1: method must be null, not "GET", for performance-api entries.
        assert_eq!(result["method"], Value::Null);
        assert_eq!(result["status"], Value::Null);
        assert_eq!(result["source"], "performance-api");
        // A per-record note must explain the missing fields.
        let note = result["note"].as_str().expect("note should be a string");
        assert!(
            note.contains("method") && note.contains("status"),
            "note should mention both method and status: {note:?}"
        );
    }
}
