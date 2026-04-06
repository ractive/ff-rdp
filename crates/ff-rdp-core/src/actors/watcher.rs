use serde_json::{Value, json};

use crate::actor::actor_request;
use crate::error::ProtocolError;
use crate::transport::RdpTransport;
use crate::types::ActorId;

/// Operations on a Watcher actor for subscribing to resource events.
///
/// The Watcher manages resource subscriptions (network events, console
/// messages, etc.) and delivers them as `resources-available-array` and
/// `resources-updated-array` events.
pub struct WatcherActor;

impl WatcherActor {
    /// Subscribe to one or more resource types.
    ///
    /// After calling this, Firefox will send `resources-available-array` events
    /// for both existing and new resources of the requested types.
    ///
    /// Resource types: `"network-event"`, `"console-message"`, `"error-message"`, etc.
    pub fn watch_resources(
        transport: &mut RdpTransport,
        watcher_actor: &ActorId,
        resource_types: &[&str],
    ) -> Result<Value, ProtocolError> {
        let types: Vec<Value> = resource_types.iter().map(|t| json!(t)).collect();
        let params = json!({ "resourceTypes": types });
        actor_request(
            transport,
            watcher_actor.as_ref(),
            "watchResources",
            Some(&params),
        )
    }

    /// Unsubscribe from one or more resource types.
    pub fn unwatch_resources(
        transport: &mut RdpTransport,
        watcher_actor: &ActorId,
        resource_types: &[&str],
    ) -> Result<Value, ProtocolError> {
        let types: Vec<Value> = resource_types.iter().map(|t| json!(t)).collect();
        let params = json!({ "resourceTypes": types });
        actor_request(
            transport,
            watcher_actor.as_ref(),
            "unwatchResources",
            Some(&params),
        )
    }
}

/// A network event resource from a `resources-available-array` message.
#[derive(Debug, Clone)]
pub struct NetworkResource {
    /// The NetworkEventActor ID for fetching details.
    pub actor: ActorId,
    /// HTTP method (GET, POST, etc.).
    pub method: String,
    /// Request URL.
    pub url: String,
    /// Whether this is an XMLHttpRequest / fetch.
    pub is_xhr: bool,
    /// The cause type (document, img, script, etc.).
    pub cause_type: String,
    /// ISO 8601 timestamp when the request started.
    pub started_date_time: String,
    /// Timestamp in milliseconds.
    pub timestamp: f64,
    /// The resource ID used to correlate with update events.
    pub resource_id: u64,
}

/// Updated fields for a network resource from `resources-updated-array`.
#[derive(Debug, Clone, Default)]
pub struct NetworkResourceUpdate {
    /// The resource ID to correlate with the original resource.
    pub resource_id: u64,
    /// HTTP status code (e.g. "200", "404").
    pub status: Option<String>,
    /// HTTP version (e.g. "HTTP/2").
    pub http_version: Option<String>,
    /// Response MIME type.
    pub mime_type: Option<String>,
    /// Total request time in milliseconds.
    pub total_time: Option<u64>,
    /// Content size in bytes.
    pub content_size: Option<u64>,
    /// Transferred size in bytes (after encoding).
    pub transferred_size: Option<u64>,
    /// Whether the response was served from cache.
    pub from_cache: Option<bool>,
    /// Remote server address.
    pub remote_address: Option<String>,
    /// Security state (e.g. "secure", "insecure").
    pub security_state: Option<String>,
}

/// Parse network resources from a `resources-available-array` event.
///
/// The event has the structure:
/// ```json
/// { "type": "resources-available-array", "array": [["network-event", [{ ... }]]] }
/// ```
pub fn parse_network_resources(event: &Value) -> Vec<NetworkResource> {
    let mut resources = Vec::new();

    let Some(array) = event.get("array").and_then(Value::as_array) else {
        return resources;
    };

    for sub in array {
        let sub_arr = match sub.as_array() {
            Some(a) if a.len() == 2 => a,
            _ => continue,
        };

        let resource_type = sub_arr[0].as_str().unwrap_or_default();
        if resource_type != "network-event" {
            continue;
        }

        let Some(items) = sub_arr[1].as_array() else {
            continue;
        };

        for item in items {
            if let Some(res) = parse_single_network_resource(item) {
                resources.push(res);
            }
        }
    }

    resources
}

/// Parse network resource updates from a `resources-updated-array` event.
pub fn parse_network_resource_updates(event: &Value) -> Vec<NetworkResourceUpdate> {
    let mut updates = Vec::new();

    let Some(array) = event.get("array").and_then(Value::as_array) else {
        return updates;
    };

    for sub in array {
        let sub_arr = match sub.as_array() {
            Some(a) if a.len() == 2 => a,
            _ => continue,
        };

        let resource_type = sub_arr[0].as_str().unwrap_or_default();
        if resource_type != "network-event" {
            continue;
        }

        let Some(items) = sub_arr[1].as_array() else {
            continue;
        };

        for item in items {
            let resource_id = item
                .get("resourceId")
                .and_then(Value::as_u64)
                .unwrap_or_default();

            let Some(ru) = item.get("resourceUpdates") else {
                continue;
            };

            let update = NetworkResourceUpdate {
                resource_id,
                status: ru.get("status").and_then(Value::as_str).map(String::from),
                http_version: ru
                    .get("httpVersion")
                    .and_then(Value::as_str)
                    .map(String::from),
                mime_type: ru.get("mimeType").and_then(Value::as_str).map(String::from),
                total_time: ru.get("totalTime").and_then(Value::as_u64),
                content_size: ru.get("contentSize").and_then(Value::as_u64),
                transferred_size: ru.get("transferredSize").and_then(Value::as_u64),
                from_cache: ru.get("fromCache").and_then(Value::as_bool),
                remote_address: ru
                    .get("remoteAddress")
                    .and_then(Value::as_str)
                    .filter(|s| !s.is_empty())
                    .map(String::from),
                security_state: ru
                    .get("securityState")
                    .and_then(Value::as_str)
                    .map(String::from),
            };

            updates.push(update);
        }
    }

    updates
}

fn parse_single_network_resource(item: &Value) -> Option<NetworkResource> {
    let actor = item.get("actor").and_then(Value::as_str)?;
    let method = item
        .get("method")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let url = item.get("url").and_then(Value::as_str).unwrap_or_default();
    let is_xhr = item.get("isXHR").and_then(Value::as_bool).unwrap_or(false);
    let cause_type = item
        .get("cause")
        .and_then(|c| c.get("type"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    let started_date_time = item
        .get("startedDateTime")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let timestamp = item
        .get("timeStamp")
        .and_then(Value::as_f64)
        .unwrap_or_default();
    let resource_id = item
        .get("resourceId")
        .and_then(Value::as_u64)
        .unwrap_or_default();

    Some(NetworkResource {
        actor: ActorId::from(actor),
        method: method.to_owned(),
        url: url.to_owned(),
        is_xhr,
        cause_type: cause_type.to_owned(),
        started_date_time: started_date_time.to_owned(),
        timestamp,
        resource_id,
    })
}

#[cfg(test)]
#[allow(clippy::unreadable_literal)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parse_network_resources_from_available_array() {
        let event = json!({
            "type": "resources-available-array",
            "from": "server1.conn0.watcher4",
            "array": [
                [
                    "network-event",
                    [
                        {
                            "actor": "server1.conn0.netEvent6",
                            "method": "GET",
                            "url": "https://example.com/",
                            "isXHR": false,
                            "cause": {"type": "document"},
                            "startedDateTime": "2026-04-06T01:33:30.344Z",
                            "timeStamp": 1775439210344.0,
                            "resourceId": 21474836486_u64,
                            "resourceType": "network-event"
                        },
                        {
                            "actor": "server1.conn0.netEvent7",
                            "method": "GET",
                            "url": "https://example.com/favicon.ico",
                            "isXHR": false,
                            "cause": {"type": "img"},
                            "startedDateTime": "2026-04-06T01:33:30.374Z",
                            "timeStamp": 1775439210374.0,
                            "resourceId": 21474836488_u64,
                            "resourceType": "network-event"
                        }
                    ]
                ]
            ]
        });

        let resources = parse_network_resources(&event);
        assert_eq!(resources.len(), 2);

        assert_eq!(resources[0].actor.as_ref(), "server1.conn0.netEvent6");
        assert_eq!(resources[0].method, "GET");
        assert_eq!(resources[0].url, "https://example.com/");
        assert!(!resources[0].is_xhr);
        assert_eq!(resources[0].cause_type, "document");

        assert_eq!(resources[1].actor.as_ref(), "server1.conn0.netEvent7");
        assert_eq!(resources[1].url, "https://example.com/favicon.ico");
        assert_eq!(resources[1].cause_type, "img");
    }

    #[test]
    fn parse_network_resources_ignores_non_network_types() {
        let event = json!({
            "array": [
                ["console-message", [{"actor": "foo"}]],
                ["network-event", [{"actor": "server1.conn0.netEvent1", "method": "GET", "url": "https://test.com"}]]
            ]
        });

        let resources = parse_network_resources(&event);
        assert_eq!(resources.len(), 1);
        assert_eq!(resources[0].url, "https://test.com");
    }

    #[test]
    fn parse_network_resources_empty_array() {
        let event = json!({"array": []});
        assert!(parse_network_resources(&event).is_empty());
    }

    #[test]
    fn parse_network_resources_missing_array() {
        let event = json!({"type": "something"});
        assert!(parse_network_resources(&event).is_empty());
    }

    #[test]
    fn parse_network_resource_updates_from_updated_array() {
        let event = json!({
            "type": "resources-updated-array",
            "array": [
                [
                    "network-event",
                    [
                        {
                            "resourceId": 21474836486_u64,
                            "resourceUpdates": {
                                "status": "200",
                                "httpVersion": "HTTP/2",
                                "mimeType": "text/html",
                                "totalTime": 45,
                                "contentSize": 528,
                                "transferredSize": 371,
                                "fromCache": false,
                                "remoteAddress": "93.184.215.14",
                                "securityState": "secure"
                            }
                        },
                        {
                            "resourceId": 21474836488_u64,
                            "resourceUpdates": {
                                "status": "404",
                                "httpVersion": "HTTP/2",
                                "mimeType": "text/html",
                                "totalTime": 0,
                                "fromCache": true
                            }
                        }
                    ]
                ]
            ]
        });

        let updates = parse_network_resource_updates(&event);
        assert_eq!(updates.len(), 2);

        assert_eq!(updates[0].resource_id, 21474836486);
        assert_eq!(updates[0].status.as_deref(), Some("200"));
        assert_eq!(updates[0].http_version.as_deref(), Some("HTTP/2"));
        assert_eq!(updates[0].content_size, Some(528));
        assert_eq!(updates[0].from_cache, Some(false));
        assert_eq!(updates[0].remote_address.as_deref(), Some("93.184.215.14"));

        assert_eq!(updates[1].resource_id, 21474836488);
        assert_eq!(updates[1].status.as_deref(), Some("404"));
        assert_eq!(updates[1].from_cache, Some(true));
    }

    #[test]
    fn parse_updates_filters_empty_remote_address() {
        let event = json!({
            "array": [["network-event", [{
                "resourceId": 1,
                "resourceUpdates": {
                    "remoteAddress": "",
                    "status": "200"
                }
            }]]]
        });

        let updates = parse_network_resource_updates(&event);
        assert_eq!(updates.len(), 1);
        assert!(updates[0].remote_address.is_none());
    }
}
