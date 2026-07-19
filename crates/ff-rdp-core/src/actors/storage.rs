use serde::Serialize;
use serde_json::{Value, json};

use crate::actor::actor_request;
use crate::actors::tab::TabActor;
use crate::actors::watcher::WatcherActor;
use crate::error::ProtocolError;
use crate::specs::types::resolve_long_string_slot;
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
    /// 3. Call `getStoreObjects` with `host` + `resourceId` + `options.sessionString` for each host
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

        // Capture the `resources-available-array` event that `watchResources`
        // triggers.  On Firefox 152 the cookies resource event arrives *before*
        // the `watchResources` ACK, so `actor_request`'s reply loop
        // (`recv_reply_from`) routes it to the transport's event sink and
        // returns only the plain ACK — the event is gone by the time we parse
        // the response.  Without a sink installed (the direct command path) the
        // event would be dropped entirely, `getStoreObjects` would never be
        // sent, and every cookie would silently fall back to `document.cookie`
        // (missing httpOnly cookies and all flags).  See iter-121.
        //
        // We install a temporary collector sink around the `watchResources`
        // call, restoring whatever sink was there before (e.g. a
        // daemon-installed one) afterwards.
        let (event_tx, event_rx) = std::sync::mpsc::channel::<Value>();
        let prev_sink = transport.swap_event_sink(Some(event_tx));

        let response = WatcherActor::watch_resources(transport, &watcher, &["cookies"]);

        // Restore the previous sink before doing anything that might early-return.
        transport.swap_event_sink(prev_sink);

        let response = response?;

        // The cookie store resource may arrive three ways depending on the
        // Firefox version and message ordering:
        //   1. Captured by our sink while awaiting the ACK (FF152: event first).
        //   2. Inline in the ACK response itself (older Firefox).
        //   3. As a separate event delivered *after* the ACK (older Firefox);
        //      read one more message to catch it.
        let cookie_resource = event_rx
            .try_iter()
            .find_map(|ev| parse_cookie_store_resource(&ev))
            .or_else(|| parse_cookie_store_resource(&response))
            .or_else(|| {
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
                        if let Some(cookie) = parse_cookie(transport, item)? {
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

/// Parse a single cookie entry from a `getStoreObjects` data array item,
/// resolving a `longstring` cookie `value` grip to its full content.
///
/// `name` is required — if absent the item is skipped (returns `Ok(None)`).
/// All other fields use sensible defaults when missing or when Firefox
/// sends an unexpected type, since this is a read-only inspection tool.
///
/// The cookie `value` slot is declared `longstring` in
/// `devtools/shared/specs/storage.js`: values above Firefox's long-string
/// threshold (~10 KB) arrive as `{type:"longString", …}` grips.  A bare
/// `.as_str()` returned empty for those; [`resolve_long_string_slot`] now
/// fetches the full value so large cookies are reported in full.  Returns
/// `Err` only when a long-string fetch fails.
pub(crate) fn parse_cookie(
    transport: &mut RdpTransport,
    item: &Value,
) -> Result<Option<CookieInfo>, ProtocolError> {
    let Some(name) = item.get("name").and_then(Value::as_str) else {
        return Ok(None);
    };
    let name = name.to_owned();
    let value = resolve_long_string_slot(transport, item.get("value"))?.unwrap_or_default();
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

    Ok(Some(CookieInfo {
        name,
        value,
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
    }))
}

/// A cookie entry derived from a `Set-Cookie` response header.
///
/// This is a simplified representation.  Only the name, value, domain, path,
/// and expiry are extracted — sufficient for merging with [`CookieInfo`].
///
/// Used by [`merge_storage_and_network_cookies`] to supplement the
/// StorageActor reply with cookies that Firefox has not yet flushed to the
/// storage actor (e.g. cookies set by a `Set-Cookie` header during the
/// navigation that just completed).
#[derive(Debug, Clone)]
pub struct NetworkSetCookie {
    /// Cookie name.
    pub name: String,
    /// Cookie value.
    pub value: String,
    /// Domain from the `Domain=` attribute (empty string if absent).
    pub domain: String,
    /// Path from the `Path=` attribute (defaults to `/`).
    pub path: String,
    /// Expiry epoch milliseconds derived from `Max-Age=` or `Expires=` (0 = session).
    pub expires: u64,
    /// Whether the `Secure` flag was set.
    pub is_secure: bool,
    /// Whether the `HttpOnly` flag was set.
    pub is_http_only: bool,
}

/// Parse a single `Set-Cookie` header value into a [`NetworkSetCookie`].
///
/// The format is:
/// ```text
/// name=value[; attr[=val]]*
/// ```
///
/// Returns `None` when the header value is empty or has no `name=value` pair.
///
/// **Limitation**: `Expires=` date strings are not parsed — only `Max-Age=`
/// is used to determine expiry.  Cookies with an `Expires=` attribute but no
/// `Max-Age=` attribute are treated as session cookies (`expires: 0`).
pub fn parse_set_cookie_header(header: &str) -> Option<NetworkSetCookie> {
    let header = header.trim();
    if header.is_empty() {
        return None;
    }

    let mut parts = header.split(';');
    // The first part is always `name=value`.
    let name_value = parts.next()?.trim();
    let (name, value) = name_value.split_once('=').unwrap_or((name_value, ""));
    let name = name.trim().to_owned();
    let value = value.trim().to_owned();
    if name.is_empty() {
        return None;
    }

    let mut domain = String::new();
    let mut path = "/".to_owned();
    let mut expires: u64 = 0;
    let mut is_secure = false;
    let mut is_http_only = false;

    for attr in parts {
        let attr = attr.trim();
        if attr.is_empty() {
            continue;
        }
        let (attr_name, attr_val) = if let Some((n, v)) = attr.split_once('=') {
            (n.trim(), v.trim())
        } else {
            (attr, "")
        };
        match attr_name.to_ascii_lowercase().as_str() {
            "domain" => attr_val.clone_into(&mut domain),
            "path" => attr_val.clone_into(&mut path),
            "max-age" => {
                if let Ok(seconds) = attr_val.parse::<i64>()
                    && seconds > 0
                {
                    // Convert to epoch ms: now + max-age seconds.  Uses the
                    // system clock, so tests assert only on directional
                    // properties (`expires > 0`), not on an exact value.
                    let now_ms = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map_or(0, |d| {
                            // d.as_millis() returns u128; cap at u64::MAX to
                            // avoid wrapping on implausibly large timestamps.
                            #[allow(clippy::cast_possible_truncation)]
                            let ms = d.as_millis() as u64;
                            ms
                        });
                    #[allow(clippy::cast_sign_loss)]
                    let offset_ms = (seconds as u64).saturating_mul(1000);
                    expires = now_ms.saturating_add(offset_ms);
                }
            }
            "secure" => is_secure = true,
            "httponly" => is_http_only = true,
            _ => {}
        }
    }

    Some(NetworkSetCookie {
        name,
        value,
        domain,
        path,
        expires,
        is_secure,
        is_http_only,
    })
}

/// Merge StorageActor cookies with cookies parsed from `Set-Cookie` headers.
///
/// Rules:
/// - StorageActor wins on key conflict (same `name`).
/// - `NetworkSetCookie` entries whose `name` is absent from `storage_cookies`
///   are appended to the result as minimal [`CookieInfo`] entries.
///
/// The merge key is `name` only.  If a future caller needs domain/path
/// disambiguation, add `(name, domain, path)` as a composite key.
///
/// # Arguments
/// * `storage_cookies` — cookies returned by [`StorageActor::list_cookies`].
/// * `network_cookies` — cookies from `Set-Cookie` response headers, e.g. via
///   [`parse_set_cookie_header`].
pub fn merge_storage_and_network_cookies(
    storage_cookies: Vec<CookieInfo>,
    network_cookies: Vec<NetworkSetCookie>,
) -> Vec<CookieInfo> {
    // Build a set of names already present in the storage reply.  We also
    // insert each appended network-cookie name into the same set so duplicate
    // `Set-Cookie` headers (multiple entries with the same name) collapse to
    // a single appended cookie — first-seen wins for the network-only side.
    let mut seen_names: std::collections::HashSet<String> =
        storage_cookies.iter().map(|c| c.name.clone()).collect();

    let mut result = storage_cookies;

    // Append network-only cookies that StorageActor hasn't seen yet.
    for nc in network_cookies {
        if !seen_names.contains(nc.name.as_str()) {
            seen_names.insert(nc.name.clone());
            result.push(CookieInfo {
                name: nc.name,
                value: nc.value,
                host: nc.domain,
                path: nc.path,
                expires: nc.expires,
                size: 0,
                is_http_only: nc.is_http_only,
                is_secure: nc.is_secure,
                same_site: String::new(),
                host_only: false,
                last_accessed: 0.0,
                creation_time: 0.0,
            });
        }
    }

    result
}

#[cfg(test)]
#[allow(clippy::unreadable_literal)]
mod tests {
    use super::*;
    use serde_json::json;

    /// A transport backed by a loopback TCP pair.  For inline (non-longString)
    /// cookie values `parse_cookie` never touches the socket, so the pair only
    /// satisfies the signature.
    fn dummy_transport() -> RdpTransport {
        use std::io::BufReader;
        use std::net::{TcpListener, TcpStream};
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let client = TcpStream::connect(addr).unwrap();
        let (_server, _) = listener.accept().unwrap();
        let writer = client.try_clone().unwrap();
        let reader = BufReader::new(client);
        RdpTransport::from_parts(reader, writer)
    }

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

        let cookie = parse_cookie(&mut dummy_transport(), &item)
            .expect("parse_cookie should not error")
            .expect("should parse full cookie");
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

        let cookie = parse_cookie(&mut dummy_transport(), &item)
            .expect("parse_cookie should not error")
            .expect("should parse session cookie");
        assert_eq!(cookie.name, "session_id");
        assert_eq!(cookie.expires, 0);
    }

    #[test]
    fn parse_cookie_missing_optional_fields_use_defaults() {
        // Only the required `name` field is present; all others default gracefully.
        let item = json!({ "name": "minimal" });

        let cookie = parse_cookie(&mut dummy_transport(), &item)
            .expect("parse_cookie should not error")
            .expect("should parse minimal cookie");
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
        // Exact comparison against the documented default (0.0) is intentional
        // here, not an accumulated float computation — `assert_eq!` on an
        // exact sentinel is clearer than an epsilon comparison would be.
        #[allow(clippy::float_cmp)]
        {
            assert_eq!(cookie.last_accessed, 0.0);
            assert_eq!(cookie.creation_time, 0.0);
        }
    }

    #[test]
    fn parse_cookie_missing_name_returns_none() {
        let item = json!({ "value": "something" });
        assert!(
            parse_cookie(&mut dummy_transport(), &item)
                .expect("parse_cookie should not error")
                .is_none()
        );
    }

    /// iter-102 Theme A: a cookie `value` arriving as a longString grip (a
    /// cookie value above ~10 KB) is resolved to its full content — previously
    /// `.as_str()` dropped it to an empty string.
    #[test]
    fn parse_cookie_resolves_longstring_value() {
        use std::io::{BufReader, Write};
        use std::net::TcpListener;
        use std::time::Duration;

        use crate::transport::{encode_frame, recv_from};

        let full = "x".repeat(20_000);
        let full_for_server = full.clone();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let handle = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut writer = stream.try_clone().unwrap();
            let mut reader = BufReader::new(stream);
            let greeting = json!({"from":"root","applicationType":"browser","traits":{}});
            writer
                .write_all(encode_frame(&serde_json::to_string(&greeting).unwrap()).as_bytes())
                .unwrap();
            let req = recv_from(&mut reader).unwrap();
            assert_eq!(req["type"], "substring");
            assert_eq!(req["to"], "conn0/longString9");
            let resp = json!({"from":"conn0/longString9","substring": full_for_server});
            writer
                .write_all(encode_frame(&serde_json::to_string(&resp).unwrap()).as_bytes())
                .unwrap();
        });

        let mut transport =
            RdpTransport::connect("127.0.0.1", port, Duration::from_secs(5)).unwrap();
        let item = json!({
            "name": "big",
            "value": {
                "type": "longString",
                "actor": "conn0/longString9",
                "length": 20_000,
                "initial": "x".repeat(1024),
            },
            "host": "example.com"
        });
        let cookie = parse_cookie(&mut transport, &item)
            .expect("parse_cookie should not error")
            .expect("cookie should parse");
        assert_eq!(cookie.name, "big");
        assert_eq!(cookie.value.len(), 20_000);
        assert_eq!(cookie.value, full);
        handle.join().unwrap();
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

    /// iter-121: the FF152 cookie resource event carries `browsingContextID`
    /// and `resourceKey` alongside `resourceId`/`hosts`.  The parser must
    /// extract the actor, resource ID, and hosts from this exact shape (the one
    /// captured live on Firefox 152.0.6 in the raw RDP trace).
    #[test]
    fn parse_cookie_store_resource_ff152_shape() {
        let event = json!({
            "type": "resources-available-array",
            "from": "server1.conn3.watcher11",
            "array": [[
                "cookies",
                [{
                    "actor": "server1.conn3.cookies12",
                    "hosts": {"https://httpbin.org": []},
                    "traits": {
                        "supportsAddItem": true,
                        "supportsRemoveItem": true,
                        "supportsRemoveAll": true,
                        "supportsRemoveAllSessionCookies": true
                    },
                    "resourceId": "cookies-15032385537",
                    "resourceKey": "cookies",
                    "browsingContextID": 11
                }]
            ]]
        });

        let resource =
            parse_cookie_store_resource(&event).expect("should parse FF152 cookie resource");
        assert_eq!(resource.actor.as_ref(), "server1.conn3.cookies12");
        assert_eq!(resource.resource_id, "cookies-15032385537");
        assert_eq!(resource.hosts, vec!["https://httpbin.org"]);
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
        let mut transport = dummy_transport();
        let cookies: Vec<CookieInfo> = items
            .iter()
            .filter_map(|item| parse_cookie(&mut transport, item).unwrap())
            .collect();

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
        let mut transport = dummy_transport();
        let cookies: Vec<CookieInfo> = items
            .iter()
            .filter_map(|item| parse_cookie(&mut transport, item).unwrap())
            .collect();
        assert!(cookies.is_empty());
    }

    // --- parse_set_cookie_header ---

    #[test]
    fn parse_set_cookie_header_basic_name_value() {
        let sc = parse_set_cookie_header("foo=bar").unwrap();
        assert_eq!(sc.name, "foo");
        assert_eq!(sc.value, "bar");
        assert_eq!(sc.domain, "");
        assert_eq!(sc.path, "/");
        assert_eq!(sc.expires, 0);
        assert!(!sc.is_secure);
        assert!(!sc.is_http_only);
    }

    #[test]
    fn parse_set_cookie_header_with_attributes() {
        let sc = parse_set_cookie_header(
            "session_id=abc123; Path=/api; Domain=example.com; Secure; HttpOnly",
        )
        .unwrap();
        assert_eq!(sc.name, "session_id");
        assert_eq!(sc.value, "abc123");
        assert_eq!(sc.domain, "example.com");
        assert_eq!(sc.path, "/api");
        assert!(sc.is_secure);
        assert!(sc.is_http_only);
    }

    #[test]
    fn parse_set_cookie_header_max_age_produces_nonzero_expires() {
        let sc = parse_set_cookie_header("probe=1; Max-Age=3600").unwrap();
        assert_eq!(sc.name, "probe");
        // Max-Age > 0 → expires should be in the future (> 0).
        assert!(sc.expires > 0, "expires must be > 0 when Max-Age is set");
    }

    #[test]
    fn parse_set_cookie_header_empty_returns_none() {
        assert!(parse_set_cookie_header("").is_none());
        assert!(parse_set_cookie_header("   ").is_none());
    }

    #[test]
    fn parse_set_cookie_header_no_value_uses_empty_string() {
        let sc = parse_set_cookie_header("token=").unwrap();
        assert_eq!(sc.name, "token");
        assert_eq!(sc.value, "");
    }

    // --- merge_storage_and_network_cookies ---

    fn make_cookie(name: &str, value: &str) -> CookieInfo {
        CookieInfo {
            name: name.to_owned(),
            value: value.to_owned(),
            host: String::new(),
            path: "/".to_owned(),
            expires: 0,
            size: 0,
            is_http_only: false,
            is_secure: false,
            same_site: String::new(),
            host_only: false,
            last_accessed: 0.0,
            creation_time: 0.0,
        }
    }

    fn make_network_cookie(name: &str, value: &str) -> NetworkSetCookie {
        NetworkSetCookie {
            name: name.to_owned(),
            value: value.to_owned(),
            domain: String::new(),
            path: "/".to_owned(),
            expires: 0,
            is_secure: false,
            is_http_only: false,
        }
    }

    /// AC: `unit_cookies_setcookie_merge` — storage wins on key conflict;
    /// network-only cookies are appended.
    #[test]
    fn unit_cookies_setcookie_merge() {
        // StorageActor has foo=storage_value.
        let storage = vec![make_cookie("foo", "storage_value")];

        // Network has foo=network_value (storage wins) and bar=network_only.
        let network = vec![
            make_network_cookie("foo", "network_value"),
            make_network_cookie("bar", "network_only"),
        ];

        let merged = merge_storage_and_network_cookies(storage, network);

        assert_eq!(merged.len(), 2, "merged must have 2 entries");

        let foo = merged.iter().find(|c| c.name == "foo").unwrap();
        assert_eq!(
            foo.value, "storage_value",
            "storage must win for 'foo': got '{}'",
            foo.value
        );

        let bar = merged.iter().find(|c| c.name == "bar").unwrap();
        assert_eq!(
            bar.value, "network_only",
            "network-only 'bar' must appear: got '{}'",
            bar.value
        );
    }

    #[test]
    fn merge_with_empty_network_returns_storage_unchanged() {
        let storage = vec![make_cookie("a", "1"), make_cookie("b", "2")];
        let merged = merge_storage_and_network_cookies(storage.clone(), vec![]);
        assert_eq!(merged.len(), 2);
        assert_eq!(merged[0].name, "a");
        assert_eq!(merged[1].name, "b");
    }

    #[test]
    fn merge_with_empty_storage_uses_network_cookies() {
        let network = vec![make_network_cookie("x", "1"), make_network_cookie("y", "2")];
        let merged = merge_storage_and_network_cookies(vec![], network);
        assert_eq!(merged.len(), 2);
    }
}
