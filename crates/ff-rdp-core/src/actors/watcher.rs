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

    /// Subscribe to target events of the given type.
    ///
    /// After calling this, Firefox will send `target-available-form` events
    /// when new targets of the specified type appear (e.g., after navigation),
    /// and `target-destroyed-form` events when targets disappear.
    ///
    /// Target types: `"frame"`, `"worker"`, `"process"`, etc.
    pub fn watch_targets(
        transport: &mut RdpTransport,
        watcher_actor: &ActorId,
        target_type: &str,
    ) -> Result<Value, ProtocolError> {
        let params = json!({ "targetType": target_type });
        actor_request(
            transport,
            watcher_actor.as_ref(),
            "watchTargets",
            Some(&params),
        )
    }

    /// Unsubscribe from target events of the given type.
    pub fn unwatch_targets(
        transport: &mut RdpTransport,
        watcher_actor: &ActorId,
        target_type: &str,
    ) -> Result<Value, ProtocolError> {
        let params = json!({ "targetType": target_type });
        actor_request(
            transport,
            watcher_actor.as_ref(),
            "unwatchTargets",
            Some(&params),
        )
    }
}

/// A target event from a `target-available-form` or `target-destroyed-form` message.
#[derive(Debug, Clone)]
pub struct TargetEvent {
    /// The target actor ID.
    pub actor: ActorId,
    /// Target URL (if available).
    pub url: Option<String>,
    /// Target title (if available).
    pub title: Option<String>,
    /// The target type (e.g., "frame").
    pub target_type: String,
    /// Whether this is a top-level target.
    pub is_top_level: bool,
}

/// Parse a `target-available-form` or `target-destroyed-form` message into a [`TargetEvent`].
///
/// The event structure is:
/// ```json
/// {"type": "target-available-form", "target": {...}}
/// ```
///
/// Returns `None` if the `target.actor` field is absent or not a string.
pub fn parse_target_event(msg: &Value) -> Option<TargetEvent> {
    let target = msg.get("target")?;
    let actor = target.get("actor").and_then(Value::as_str)?;
    let url = target.get("url").and_then(Value::as_str).map(String::from);
    let title = target
        .get("title")
        .and_then(Value::as_str)
        .map(String::from);
    let target_type = target
        .get("targetType")
        .and_then(Value::as_str)
        .unwrap_or("frame")
        .to_owned();
    let is_top_level = target
        .get("isTopLevelTarget")
        .and_then(Value::as_bool)
        .unwrap_or(false);

    Some(TargetEvent {
        actor: ActorId::from(actor),
        url,
        title,
        target_type,
        is_top_level,
    })
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
            let Some(resource_id) = item.get("resourceId").and_then(Value::as_u64) else {
                continue;
            };

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

/// A console message resource from a `resources-available-array` message.
#[derive(Debug, Clone)]
pub struct ConsoleResource {
    /// Message level: "log", "warn", "error", "info", "debug", "trace".
    pub level: String,
    /// The message text (joined from arguments if needed).
    pub message: String,
    /// Source file where the message was emitted.
    pub source: String,
    /// Line number in the source file.
    pub line: u32,
    /// Column number in the source file.
    pub column: u32,
    /// Timestamp in milliseconds since epoch.
    pub timestamp: f64,
    /// The resource ID used to correlate with update events.
    pub resource_id: Option<u64>,
}

/// Parse console resources from a `resources-available-array` event.
///
/// Handles both `"console-message"` and `"error-message"` resource types.
///
/// The event has the structure:
/// ```json
/// { "type": "resources-available-array", "array": [["console-message", [{ ... }]]] }
/// ```
pub fn parse_console_resources(event: &Value) -> Vec<ConsoleResource> {
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
        if resource_type != "console-message" && resource_type != "error-message" {
            continue;
        }

        let Some(items) = sub_arr[1].as_array() else {
            continue;
        };

        for item in items {
            if let Some(res) = parse_single_console_resource(item) {
                resources.push(res);
            }
        }
    }

    resources
}

fn parse_single_console_resource(item: &Value) -> Option<ConsoleResource> {
    let resource_id = item.get("resourceId").and_then(Value::as_u64);

    // Try console-message format first: item has a "message" sub-object.
    if let Some(msg) = item.get("message") {
        let level = msg
            .get("level")
            .and_then(Value::as_str)
            .unwrap_or("log")
            .to_owned();

        // Arguments is an array of values; join them as strings.
        let message = msg
            .get("arguments")
            .and_then(Value::as_array)
            .map(|args| {
                args.iter()
                    .map(|a| match a.as_str() {
                        Some(s) => s.to_owned(),
                        None => a.to_string(),
                    })
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .unwrap_or_default();

        let source = msg
            .get("filename")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();

        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let line = msg
            .get("lineNumber")
            .and_then(Value::as_u64)
            .unwrap_or_default() as u32;

        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let column = msg
            .get("columnNumber")
            .and_then(Value::as_u64)
            .unwrap_or_default() as u32;

        let timestamp = msg
            .get("timeStamp")
            .and_then(Value::as_f64)
            .unwrap_or_default();

        return Some(ConsoleResource {
            level,
            message,
            source,
            line,
            column,
            timestamp,
            resource_id,
        });
    }

    // Try error-message format: item has a "pageError" sub-object.
    if let Some(err) = item.get("pageError") {
        let level = "error".to_owned();

        let message = err
            .get("errorMessage")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();

        let source = err
            .get("sourceName")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();

        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let line = err
            .get("lineNumber")
            .and_then(Value::as_u64)
            .unwrap_or_default() as u32;

        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let column = err
            .get("columnNumber")
            .and_then(Value::as_u64)
            .unwrap_or_default() as u32;

        let timestamp = err
            .get("timeStamp")
            .and_then(Value::as_f64)
            .unwrap_or_default();

        return Some(ConsoleResource {
            level,
            message,
            source,
            line,
            column,
            timestamp,
            resource_id,
        });
    }

    None
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
    let resource_id = item.get("resourceId").and_then(Value::as_u64)?;

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
    fn parse_target_event_valid() {
        let msg = json!({
            "type": "target-available-form",
            "target": {
                "actor": "server1.conn0.windowGlobalTarget42",
                "url": "https://example.com/",
                "title": "Example Domain",
                "targetType": "frame",
                "isTopLevelTarget": true
            }
        });

        let event = parse_target_event(&msg).expect("should parse");
        assert_eq!(event.actor.as_ref(), "server1.conn0.windowGlobalTarget42");
        assert_eq!(event.url.as_deref(), Some("https://example.com/"));
        assert_eq!(event.title.as_deref(), Some("Example Domain"));
        assert_eq!(event.target_type, "frame");
        assert!(event.is_top_level);
    }

    #[test]
    fn parse_target_event_missing_actor_returns_none() {
        // No "actor" field — must return None.
        let msg = json!({
            "type": "target-available-form",
            "target": {
                "url": "https://example.com/",
                "targetType": "frame"
            }
        });

        assert!(parse_target_event(&msg).is_none());
    }

    #[test]
    fn parse_target_event_missing_target_returns_none() {
        // No "target" key at all.
        let msg = json!({"type": "target-available-form"});
        assert!(parse_target_event(&msg).is_none());
    }

    #[test]
    fn parse_target_event_non_top_level_target() {
        let msg = json!({
            "type": "target-available-form",
            "target": {
                "actor": "server1.conn0.windowGlobalTarget99",
                "url": "https://sub.example.com/iframe.html",
                "targetType": "frame",
                "isTopLevelTarget": false
            }
        });

        let event = parse_target_event(&msg).expect("should parse");
        assert_eq!(event.actor.as_ref(), "server1.conn0.windowGlobalTarget99");
        assert!(!event.is_top_level);
        assert!(event.title.is_none());
    }

    #[test]
    fn parse_target_event_defaults_target_type_to_frame() {
        // When "targetType" is absent, defaults to "frame".
        let msg = json!({
            "target": {
                "actor": "server1.conn0.windowGlobalTarget5",
                "url": "https://example.com/"
            }
        });

        let event = parse_target_event(&msg).expect("should parse");
        assert_eq!(event.target_type, "frame");
        assert!(!event.is_top_level);
    }

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
                ["network-event", [{"actor": "server1.conn0.netEvent1", "method": "GET", "url": "https://test.com", "resourceId": 1}]]
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
    fn parse_network_resources_skips_missing_resource_id() {
        // Items without a resourceId must be skipped entirely (not mapped to 0).
        let event = json!({
            "array": [["network-event", [
                {"actor": "a1", "method": "GET", "url": "https://no-id.example.com"},
                {"actor": "a2", "method": "GET", "url": "https://has-id.example.com", "resourceId": 42}
            ]]]
        });

        let resources = parse_network_resources(&event);
        assert_eq!(resources.len(), 1);
        assert_eq!(resources[0].url, "https://has-id.example.com");
        assert_eq!(resources[0].resource_id, 42);
    }

    #[test]
    fn parse_network_resource_updates_skips_missing_resource_id() {
        // Updates without a resourceId must be skipped (not mapped to 0).
        let event = json!({
            "array": [["network-event", [
                {"resourceUpdates": {"status": "200"}},
                {"resourceId": 99, "resourceUpdates": {"status": "404"}}
            ]]]
        });

        let updates = parse_network_resource_updates(&event);
        assert_eq!(updates.len(), 1);
        assert_eq!(updates[0].resource_id, 99);
        assert_eq!(updates[0].status.as_deref(), Some("404"));
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

    // --- Console resource parsing tests ---

    #[test]
    fn parse_console_resources_console_message_format() {
        let event = json!({
            "type": "resources-available-array",
            "from": "server1.conn0.watcher4",
            "array": [
                [
                    "console-message",
                    [
                        {
                            "resourceType": "console-message",
                            "resourceId": 100_u64,
                            "message": {
                                "arguments": ["hello", "world"],
                                "level": "log",
                                "filename": "https://example.com/app.js",
                                "lineNumber": 5,
                                "columnNumber": 1,
                                "timeStamp": 1775439071165.699_f64
                            }
                        },
                        {
                            "resourceType": "console-message",
                            "resourceId": 101_u64,
                            "message": {
                                "arguments": ["warning msg"],
                                "level": "warn",
                                "filename": "https://example.com/app.js",
                                "lineNumber": 15,
                                "columnNumber": 41,
                                "timeStamp": 1775439071166.011_f64
                            }
                        }
                    ]
                ]
            ]
        });

        let resources = parse_console_resources(&event);
        assert_eq!(resources.len(), 2);

        assert_eq!(resources[0].level, "log");
        assert_eq!(resources[0].message, "hello world");
        assert_eq!(resources[0].source, "https://example.com/app.js");
        assert_eq!(resources[0].line, 5);
        assert_eq!(resources[0].column, 1);
        assert!(resources[0].timestamp > 0.0);
        assert_eq!(resources[0].resource_id, Some(100));

        assert_eq!(resources[1].level, "warn");
        assert_eq!(resources[1].message, "warning msg");
        assert_eq!(resources[1].line, 15);
        assert_eq!(resources[1].resource_id, Some(101));
    }

    #[test]
    fn parse_console_resources_error_message_format() {
        let event = json!({
            "type": "resources-available-array",
            "array": [
                [
                    "error-message",
                    [
                        {
                            "resourceType": "error-message",
                            "resourceId": 200_u64,
                            "pageError": {
                                "errorMessage": "ReferenceError: foo is not defined",
                                "sourceName": "https://example.com/app.js",
                                "lineNumber": 42,
                                "columnNumber": 5,
                                "timeStamp": 1775439071166.0_f64
                            }
                        }
                    ]
                ]
            ]
        });

        let resources = parse_console_resources(&event);
        assert_eq!(resources.len(), 1);

        assert_eq!(resources[0].level, "error");
        assert_eq!(resources[0].message, "ReferenceError: foo is not defined");
        assert_eq!(resources[0].source, "https://example.com/app.js");
        assert_eq!(resources[0].line, 42);
        assert_eq!(resources[0].column, 5);
        assert_eq!(resources[0].resource_id, Some(200));
    }

    #[test]
    fn parse_console_resources_mixed_types_in_same_event() {
        // Both console-message and error-message sub-arrays can appear together.
        let event = json!({
            "array": [
                ["network-event", [{"actor": "a1", "method": "GET", "url": "https://x.com", "resourceId": 1_u64}]],
                ["console-message", [{
                    "resourceType": "console-message",
                    "message": {
                        "arguments": ["hi"],
                        "level": "log",
                        "filename": "test.js",
                        "lineNumber": 1,
                        "columnNumber": 1,
                        "timeStamp": 1000.0
                    }
                }]],
                ["error-message", [{
                    "resourceType": "error-message",
                    "pageError": {
                        "errorMessage": "SyntaxError",
                        "sourceName": "test.js",
                        "lineNumber": 2,
                        "columnNumber": 0,
                        "timeStamp": 2000.0
                    }
                }]]
            ]
        });

        let resources = parse_console_resources(&event);
        assert_eq!(resources.len(), 2);
        assert_eq!(resources[0].level, "log");
        assert_eq!(resources[0].message, "hi");
        assert_eq!(resources[1].level, "error");
        assert_eq!(resources[1].message, "SyntaxError");
    }

    #[test]
    fn parse_console_resources_ignores_network_events() {
        let event = json!({
            "array": [
                ["network-event", [{"actor": "a1", "method": "GET", "url": "https://x.com", "resourceId": 1_u64}]]
            ]
        });

        let resources = parse_console_resources(&event);
        assert!(resources.is_empty());
    }

    #[test]
    fn parse_console_resources_empty_array() {
        let event = json!({"array": []});
        assert!(parse_console_resources(&event).is_empty());
    }

    #[test]
    fn parse_console_resources_missing_array() {
        let event = json!({"type": "resources-available-array"});
        assert!(parse_console_resources(&event).is_empty());
    }

    #[test]
    fn parse_console_resources_multiple_arguments_joined() {
        let event = json!({
            "array": [["console-message", [{
                "message": {
                    "arguments": ["count:", 42, true],
                    "level": "log",
                    "filename": "test.js",
                    "lineNumber": 5,
                    "columnNumber": 1,
                    "timeStamp": 1000.0
                }
            }]]]
        });

        let resources = parse_console_resources(&event);
        assert_eq!(resources.len(), 1);
        assert_eq!(resources[0].message, "count: 42 true");
    }

    #[test]
    fn parse_console_resources_no_resource_id_is_none() {
        // Items without a resourceId produce resource_id: None (not skipped).
        let event = json!({
            "array": [["console-message", [{
                "message": {
                    "arguments": ["no id"],
                    "level": "log",
                    "filename": "test.js",
                    "lineNumber": 1,
                    "columnNumber": 1,
                    "timeStamp": 1000.0
                }
            }]]]
        });

        let resources = parse_console_resources(&event);
        assert_eq!(resources.len(), 1);
        assert_eq!(resources[0].resource_id, None);
        assert_eq!(resources[0].message, "no id");
    }

    #[test]
    fn parse_console_resources_skips_items_with_neither_format() {
        // Items that have neither "message" nor "pageError" are skipped.
        let event = json!({
            "array": [["console-message", [
                {"resourceType": "console-message"},
                {
                    "message": {
                        "arguments": ["valid"],
                        "level": "log",
                        "filename": "test.js",
                        "lineNumber": 1,
                        "columnNumber": 1,
                        "timeStamp": 1000.0
                    }
                }
            ]]]
        });

        let resources = parse_console_resources(&event);
        assert_eq!(resources.len(), 1);
        assert_eq!(resources[0].message, "valid");
    }

    #[test]
    fn parse_console_resources_defaults_level_to_log_when_missing() {
        let event = json!({
            "array": [["console-message", [{
                "message": {
                    "arguments": ["no level field"],
                    "filename": "test.js",
                    "lineNumber": 1,
                    "columnNumber": 1,
                    "timeStamp": 1000.0
                }
            }]]]
        });

        let resources = parse_console_resources(&event);
        assert_eq!(resources.len(), 1);
        assert_eq!(resources[0].level, "log");
    }
}
