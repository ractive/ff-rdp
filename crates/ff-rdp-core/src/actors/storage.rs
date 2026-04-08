use serde::Serialize;
use serde_json::{Value, json};

use crate::actor::actor_request;
use crate::actors::tab::TabActor;
use crate::actors::watcher::WatcherActor;
use crate::error::ProtocolError;
use crate::transport::RdpTransport;
use crate::types::ActorId;

/// Metadata about a cookie from the Firefox StorageActor.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CookieInfo {
    pub name: String,
    pub value: String,
    pub host: String,
    pub path: String,
    /// Expiry as epoch milliseconds. 0 means session cookie.
    pub expires: u64,
    pub size: u64,
    pub is_http_only: bool,
    pub is_secure: bool,
    pub same_site: String,
    pub host_only: bool,
    pub last_accessed: f64,
    pub creation_time: f64,
}

/// Information about the cookies storage resource actor returned by watchResources.
#[derive(Debug, Clone)]
pub(crate) struct CookieStoreResource {
    pub actor: ActorId,
    /// The resource ID from the watchResources response.
    ///
    /// Firefox 149+ requires this to be passed back with `getStoreObjects` when
    /// called after `getTarget` has established a child browsing context.
    pub resource_id: String,
    /// Host origins associated with this cookie store (keys of the `hosts` object).
    pub hosts: Vec<String>,
}

/// Operations on the Firefox StorageActor for reading browser storage.
pub struct StorageActor;

impl StorageActor {
    /// List all cookies for the current tab.
    ///
    /// This performs the full protocol sequence:
    /// 1. Get a watcher via `TabActor::get_watcher`
    /// 2. Watch for `"cookies"` resources to get the cookie store actor + resource ID
    /// 3. Call `getStoreObjects` with `host` + `resourceId` for each host
    /// 4. Unwatch when done (best-effort)
    ///
    /// # Firefox 149 compatibility
    ///
    /// Firefox 149 changed the `getStoreObjects` API in three ways:
    ///
    /// 1. **`host` is no longer used for routing** — older versions routed the
    ///    request via the watcher when a `host` field was present, returning an
    ///    empty ACK from the wrong actor.  Now `host` goes directly to the cookie
    ///    store actor as a filter parameter.
    ///
    /// 2. **`resourceId` is now required** when `getTarget` has been called first
    ///    (which creates a child browsing context).  Without it, Firefox fails with
    ///    "undefined passed where a value is required".  The `resourceId` is
    ///    returned as part of the `watchResources("cookies")` response.
    ///
    /// 3. **`options.sessionString` is now required** — Firefox 149 accesses
    ///    `options.sessionString` unconditionally in its internal JS before
    ///    filtering cookies.  Without it, Firefox crashes with:
    ///    `TypeError: can't access property "toLowerCase", sessionString is undefined`.
    ///    Passing `"Session"` satisfies the requirement; all cookies are returned
    ///    regardless of the value (it is used as a display/sort hint, not a filter).
    ///
    /// All three fields — `host`, `resourceId`, and `options.sessionString` — must
    /// be supplied together.
    pub fn list_cookies(
        transport: &mut RdpTransport,
        tab_actor: &ActorId,
    ) -> Result<Vec<CookieInfo>, ProtocolError> {
        let watcher = TabActor::get_watcher(transport, tab_actor)?;

        // watchResources may return the resources-available-array directly
        // (observed for "cookies") or return a plain ACK followed by the
        // resources-available-array as a separate event (as with "network-event").
        // We try parsing the immediate response first; if that yields nothing,
        // we read one more message from the watcher to catch the async case.
        let response = WatcherActor::watch_resources(transport, &watcher, &["cookies"])?;

        let cookie_resource = parse_cookie_store_resource(&response).or_else(|| {
            // The immediate response was a plain ACK — try the next message.
            let followup = transport.recv().ok()?;
            parse_cookie_store_resource(&followup)
        });

        let mut cookies = Vec::new();

        if let Some(resource) = cookie_resource {
            for host in &resource.hosts {
                // Firefox 149+: pass `host`, `resourceId`, and `options.sessionString`.
                // - `host` must be one of the keys from the `hosts` map returned by
                //   watchResources (used as a filter parameter).
                // - `resourceId` is required when getTarget has been called first
                //   (establishing a child browsing context).
                // - `options.sessionString` is required by Firefox 149+ internal JS
                //   to avoid a TypeError crash; "Session" is the expected value and
                //   all cookies are returned regardless.
                let params = json!({
                    "host": host,
                    "resourceId": resource.resource_id,
                    "options": {
                        "sessionString": "Session",
                    },
                });
                let store_response = actor_request(
                    transport,
                    resource.actor.as_ref(),
                    "getStoreObjects",
                    Some(&params),
                )?;

                if let Some(items) = store_response.get("data").and_then(Value::as_array) {
                    for item in items {
                        if let Some(cookie) = parse_cookie(item) {
                            cookies.push(cookie);
                        }
                    }
                }
            }
        }

        // Best-effort unwatch — ignore errors so we don't mask the real result.
        let _ = WatcherActor::unwatch_resources(transport, &watcher, &["cookies"]);

        Ok(cookies)
    }
}

/// Parse a `resources-available-array` response to extract the cookies store
/// resource actor, its resource ID, and the associated host origins.
///
/// Expected shape (Firefox < 149):
/// ```json
/// {
///   "type": "resources-available-array",
///   "array": [["cookies", [{"actor": "...", "hosts": {"host1": null}}]]],
///   "from": "..."
/// }
/// ```
///
/// Firefox 149+ shape (`hosts` values changed from `null` to `[]`, and a
/// `resourceId` field is present and required for subsequent `getStoreObjects`
/// calls when `getTarget` has established a child browsing context):
/// ```json
/// {
///   "type": "resources-available-array",
///   "array": [["cookies", [{"actor": "...", "hosts": {"host1": []}, "resourceId": "cookies-..."}]]],
///   "from": "..."
/// }
/// ```
fn parse_cookie_store_resource(event: &Value) -> Option<CookieStoreResource> {
    let array = event.get("array").and_then(Value::as_array)?;

    for sub in array {
        let Some(sub_arr) = sub.as_array() else {
            continue;
        };
        if sub_arr.len() != 2 {
            continue;
        }

        if sub_arr[0].as_str() != Some("cookies") {
            continue;
        }

        let Some(items) = sub_arr[1].as_array() else {
            continue;
        };

        // Take the first cookies resource. Firefox may include multiple
        // entries when iframes are present, but the CLI operates on the
        // top-level tab context, so the first entry is correct.
        let Some(item) = items.first() else {
            continue;
        };

        let Some(actor) = item.get("actor").and_then(Value::as_str) else {
            continue;
        };

        // `resourceId` is a string ID required by Firefox 149+ when calling
        // `getStoreObjects` after `getTarget` has established a child context.
        // Absent in older Firefox — default to empty string, which is harmless
        // there since the field is ignored by older cookie store actors.
        let resource_id = item
            .get("resourceId")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();

        // `hosts` is a JSON object whose keys are the host origin strings.
        // The values were `null` in older Firefox and `[]` in Firefox 149+;
        // we only need the keys.
        let hosts: Vec<String> = item
            .get("hosts")
            .and_then(Value::as_object)
            .map(|obj| obj.keys().cloned().collect())
            .unwrap_or_default();

        return Some(CookieStoreResource {
            actor: ActorId::from(actor),
            resource_id,
            hosts,
        });
    }

    None
}

/// Parse a single cookie entry from a `getStoreObjects` data array item.
///
/// `name` is required — if absent the item is skipped (returns `None`).
/// All other fields use sensible defaults when missing or when Firefox
/// sends an unexpected type, since this is a read-only inspection tool.
pub(crate) fn parse_cookie(item: &Value) -> Option<CookieInfo> {
    let name = item.get("name").and_then(Value::as_str)?;
    let value = item
        .get("value")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let host = item.get("host").and_then(Value::as_str).unwrap_or_default();
    let path = item.get("path").and_then(Value::as_str).unwrap_or_default();
    let expires = item.get("expires").and_then(Value::as_u64).unwrap_or(0);
    let size = item.get("size").and_then(Value::as_u64).unwrap_or(0);
    let is_http_only = item
        .get("isHttpOnly")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let is_secure = item
        .get("isSecure")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let same_site = item
        .get("sameSite")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let host_only = item
        .get("hostOnly")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let last_accessed = item
        .get("lastAccessed")
        .and_then(Value::as_f64)
        .unwrap_or(0.0);
    let creation_time = item
        .get("creationTime")
        .and_then(Value::as_f64)
        .unwrap_or(0.0);

    Some(CookieInfo {
        name: name.to_owned(),
        value: value.to_owned(),
        host: host.to_owned(),
        path: path.to_owned(),
        expires,
        size,
        is_http_only,
        is_secure,
        same_site: same_site.to_owned(),
        host_only,
        last_accessed,
        creation_time,
    })
}

#[cfg(test)]
#[allow(clippy::unreadable_literal)]
mod tests {
    use super::*;
    use serde_json::json;

    // --- parse_cookie ---

    #[test]
    fn parse_cookie_full_fields() {
        let item = json!({
            "name": "probecookie",
            "value": "discovery123",
            "host": "example.com",
            "path": "/",
            "expires": 1810029018311_u64,
            "size": 23,
            "isHttpOnly": false,
            "isSecure": false,
            "sameSite": "",
            "hostOnly": true,
            "lastAccessed": 1775469018311.408_f64,
            "creationTime": 1775468436089.921_f64
        });

        let cookie = parse_cookie(&item).expect("should parse full cookie");
        assert_eq!(cookie.name, "probecookie");
        assert_eq!(cookie.value, "discovery123");
        assert_eq!(cookie.host, "example.com");
        assert_eq!(cookie.path, "/");
        assert_eq!(cookie.expires, 1810029018311);
        assert_eq!(cookie.size, 23);
        assert!(!cookie.is_http_only);
        assert!(!cookie.is_secure);
        assert_eq!(cookie.same_site, "");
        assert!(cookie.host_only);
        assert!((cookie.last_accessed - 1775469018311.408).abs() < 1.0);
        assert!((cookie.creation_time - 1775468436089.921).abs() < 1.0);
    }

    #[test]
    fn parse_cookie_session_cookie_expires_zero() {
        let item = json!({
            "name": "session_id",
            "value": "abc123",
            "host": "example.com",
            "expires": 0,
            "size": 14
        });

        let cookie = parse_cookie(&item).expect("should parse session cookie");
        assert_eq!(cookie.name, "session_id");
        assert_eq!(cookie.expires, 0);
    }

    #[test]
    fn parse_cookie_missing_optional_fields_use_defaults() {
        // Only the required `name` field is present; all others default gracefully.
        let item = json!({ "name": "minimal" });

        let cookie = parse_cookie(&item).expect("should parse minimal cookie");
        assert_eq!(cookie.name, "minimal");
        assert_eq!(cookie.value, "");
        assert_eq!(cookie.host, "");
        assert_eq!(cookie.path, "");
        assert_eq!(cookie.expires, 0);
        assert_eq!(cookie.size, 0);
        assert!(!cookie.is_http_only);
        assert!(!cookie.is_secure);
        assert_eq!(cookie.same_site, "");
        assert!(!cookie.host_only);
        assert!(cookie.last_accessed == 0.0);
        assert!(cookie.creation_time == 0.0);
    }

    #[test]
    fn parse_cookie_missing_name_returns_none() {
        let item = json!({ "value": "something" });
        assert!(parse_cookie(&item).is_none());
    }

    // --- parse_cookie_store_resource ---

    #[test]
    fn parse_cookie_store_resource_extracts_actor_and_hosts() {
        // Firefox < 149: hosts values are null, no resourceId field.
        let event_old = json!({
            "type": "resources-available-array",
            "from": "server1.conn7.watcher3",
            "array": [[
                "cookies",
                [{
                    "actor": "server1.conn7.storage10",
                    "hosts": {
                        "https://example.com": null,
                        "https://other.com": null
                    },
                    "traits": {}
                }]
            ]]
        });

        let resource =
            parse_cookie_store_resource(&event_old).expect("should extract cookie resource");
        assert_eq!(resource.actor.as_ref(), "server1.conn7.storage10");
        // resource_id defaults to empty string when absent.
        assert_eq!(resource.resource_id, "");
        let mut hosts = resource.hosts.clone();
        hosts.sort();
        assert_eq!(hosts, vec!["https://example.com", "https://other.com"]);

        // Firefox 149+: hosts values are empty arrays, resourceId is present.
        let event_new = json!({
            "type": "resources-available-array",
            "from": "server1.conn7.watcher3",
            "array": [[
                "cookies",
                [{
                    "actor": "server1.conn7.storage10",
                    "hosts": {
                        "https://httpbin.org": []
                    },
                    "resourceId": "cookies-208305913857",
                    "traits": {
                        "supportsAddItem": true
                    }
                }]
            ]]
        });

        let resource2 = parse_cookie_store_resource(&event_new)
            .expect("should extract cookie resource (FF149+)");
        assert_eq!(resource2.actor.as_ref(), "server1.conn7.storage10");
        assert_eq!(resource2.resource_id, "cookies-208305913857");
        assert_eq!(resource2.hosts, vec!["https://httpbin.org"]);
    }

    #[test]
    fn parse_cookie_store_resource_ignores_non_cookies_entries() {
        let event = json!({
            "array": [[
                "network-event",
                [{"actor": "server1.conn0.netEvent1"}]
            ]]
        });

        assert!(parse_cookie_store_resource(&event).is_none());
    }

    #[test]
    fn parse_cookie_store_resource_empty_array_returns_none() {
        let event = json!({ "array": [] });
        assert!(parse_cookie_store_resource(&event).is_none());
    }

    #[test]
    fn parse_cookie_store_resource_missing_array_returns_none() {
        let event = json!({ "type": "resources-available-array" });
        assert!(parse_cookie_store_resource(&event).is_none());
    }

    // --- getStoreObjects data parsing ---

    #[test]
    fn parse_cookies_from_get_store_objects_response() {
        let response = json!({
            "data": [
                {
                    "name": "cookie_a", "value": "val_a",
                    "host": "example.com", "path": "/",
                    "expires": 1810000000000_u64, "size": 15,
                    "isHttpOnly": true, "isSecure": true,
                    "sameSite": "Strict", "hostOnly": false,
                    "lastAccessed": 1775469000000.0_f64,
                    "creationTime": 1775468000000.0_f64
                },
                {
                    "name": "cookie_b", "value": "val_b",
                    "host": "example.com", "path": "/sub",
                    "expires": 0, "size": 15,
                    "isHttpOnly": false, "isSecure": false,
                    "sameSite": "Lax", "hostOnly": true,
                    "lastAccessed": 1775469001000.0_f64,
                    "creationTime": 1775468001000.0_f64
                }
            ],
            "from": "server1.conn7.storage10",
            "offset": 0,
            "total": 2
        });

        let items = response["data"].as_array().unwrap();
        let cookies: Vec<CookieInfo> = items.iter().filter_map(parse_cookie).collect();

        assert_eq!(cookies.len(), 2);
        assert_eq!(cookies[0].name, "cookie_a");
        assert!(cookies[0].is_http_only);
        assert!(cookies[0].is_secure);
        assert_eq!(cookies[0].same_site, "Strict");
        assert!(!cookies[0].host_only);

        assert_eq!(cookies[1].name, "cookie_b");
        assert_eq!(cookies[1].expires, 0);
        assert_eq!(cookies[1].same_site, "Lax");
        assert!(cookies[1].host_only);
    }

    #[test]
    fn parse_cookies_empty_data_array() {
        let response = json!({ "data": [], "from": "server1.conn7.storage10", "total": 0 });
        let items = response["data"].as_array().unwrap();
        let cookies: Vec<CookieInfo> = items.iter().filter_map(parse_cookie).collect();
        assert!(cookies.is_empty());
    }
}
