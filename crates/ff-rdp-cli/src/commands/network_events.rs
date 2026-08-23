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

/// How long the resource stream must stay silent before
/// [`drain_network_events_timed`] concludes the page has finished loading.
///
/// Long enough to bridge the gaps inside a normal page load — the document
/// request lands 1-2 s before its subresources, and lazily-triggered requests
/// (fonts, XHR fired from `DOMContentLoaded`) follow the same pattern — and
/// short enough that a quiet page returns in a couple of seconds rather than
/// burning the whole `--timeout`.
const NETWORK_IDLE_QUIET_PERIOD: Duration = Duration::from_secs(2);

/// Drain network events until the stream goes idle, bounded by `total_timeout`.
///
/// Collection stops at the **first** of:
///  - `NETWORK_IDLE_QUIET_PERIOD` elapsing with no
///    `resources-available-array` / `resources-updated-array` frame, once at
///    least one event has been seen; or
///  - `total_timeout` of wall-clock time (the hard ceiling).
///
/// iter-159: this used to run the wall clock out unconditionally — the loop's
/// only exit was `start.elapsed() >= total_timeout` — so `navigate
/// --with-network --timeout 30000` sat for the full 30 s on a page that had
/// finished in 800 ms. The idle cutoff only applies after the first event, so
/// a slow first byte still gets the whole budget.
///
/// The third element of the returned tuple keeps its meaning: `timeout_reached`
/// is `true` only when the wall-clock ceiling fired while events were still
/// arriving, `false` when collection stopped because the stream went quiet.
pub(crate) fn drain_network_events_timed(
    transport: &mut RdpTransport,
    total_timeout: Duration,
) -> Result<(Vec<NetworkResource>, Vec<NetworkResourceUpdate>, bool), ProtocolError> {
    let start = Instant::now();
    // Poll faster than the quiet period so the idle cutoff has resolution.
    let poll_interval = Duration::from_millis(250);

    // Set a short read timeout for responsive polling.
    transport.set_read_timeout(Some(poll_interval))?;

    let mut all_resources = Vec::new();
    let mut all_updates = Vec::new();
    // True after a recv that returned actual data; reset to false on idle timeout.
    // When the deadline fires, this tells us whether events were still arriving.
    let mut last_recv_was_event = false;
    // Instant of the most recent resource frame, or `None` before the first.
    let mut last_event_at: Option<Instant> = None;

    loop {
        // Check wall-clock deadline before each read so we stop even when
        // messages arrive faster than the poll interval (continuous traffic).
        if start.elapsed() >= total_timeout {
            break;
        }

        // Idle cutoff: once events have started, a quiet stream means the page
        // is done.  Before the first event there is nothing to be idle about —
        // a slow-to-respond origin must keep the full budget.
        if let Some(last) = last_event_at
            && last.elapsed() >= NETWORK_IDLE_QUIET_PERIOD
        {
            last_recv_was_event = false;
            break;
        }

        last_recv_was_event = false;
        match transport.recv() {
            Ok(msg) => {
                let msg_type = msg.get("type").and_then(Value::as_str).unwrap_or_default();
                match msg_type {
                    "resources-available-array" => {
                        last_recv_was_event = true;
                        last_event_at = Some(Instant::now());
                        all_resources.extend(parse_network_resources(&msg));
                    }
                    "resources-updated-array" => {
                        last_recv_was_event = true;
                        last_event_at = Some(Instant::now());
                        all_updates.extend(parse_network_resource_updates(&msg));
                    }
                    _ => {}
                }
            }
            Err(ProtocolError::Timeout) => {
                // Per-read timeout with no message — the top-of-loop checks
                // enforce both the idle cutoff and the total deadline.
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
        fold_update(&mut update_map, update);
    }
    update_map
}

/// Fold one [`NetworkResourceUpdate`] into `update_map`, letting each
/// `Some` field overwrite the value already recorded for that `resource_id`.
///
/// Split out of [`merge_updates`] (iter-181) so a long-lived subscription can
/// fold updates as they arrive instead of re-merging a growing `Vec` on every
/// look — see [`crate::commands::network_watch`].
pub(crate) fn fold_update(
    update_map: &mut HashMap<u64, NetworkResourceUpdate>,
    update: NetworkResourceUpdate,
) {
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
                // stderr-ok: (b) best-effort fallback diagnostic — see the
                // doc comment above; caller gets an empty vec either way.
                eprintln!("hint: performance-api fallback eval failed: {e:#}");
                return vec![];
            }
        };

    // If the eval threw an exception treat it as an empty result.
    if let Some(ref exc) = eval_result.exception {
        let msg = exc.message.as_deref().unwrap_or("(no message)");
        // stderr-ok: (b) best-effort fallback diagnostic — see the doc
        // comment above; caller gets an empty vec either way.
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
                // stderr-ok: (b) best-effort fallback diagnostic — see the
                // doc comment above; caller gets an empty vec either way.
                eprintln!("hint: performance-api fallback failed to fetch long string: {e:#}");
                return vec![];
            }
        },
        other => {
            // stderr-ok: (b) best-effort fallback diagnostic — see the doc
            // comment above; caller gets an empty vec either way.
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
            // stderr-ok: (b) best-effort fallback diagnostic — see the doc
            // comment above; caller gets an empty vec either way.
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
