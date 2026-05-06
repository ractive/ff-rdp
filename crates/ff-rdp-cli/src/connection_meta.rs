//! Build the `meta.connection` block embedded in every browser-touching
//! command's JSON envelope.
//!
//! The block surfaces who we are talking to so that confused users (and AI
//! agents) can see at a glance whether they have a fresh launch, a stale
//! daily-driver Firefox, or no connection at all. Fields that cannot be
//! determined are omitted rather than emitted as `null`.
//!
//! Looking up the listener PID is OS-specific and may be slow on Windows,
//! so the result is cached per process via [`OnceLock`].

use std::sync::OnceLock;

use serde_json::{Value, json};

use crate::port_owner::{self, PortOwner};

type OwnerCacheEntry = ((String, u16), Option<PortOwner>);

static OWNER_CACHE: OnceLock<std::sync::Mutex<Vec<OwnerCacheEntry>>> = OnceLock::new();

static REMEMBERED_VERSION: OnceLock<std::sync::Mutex<Option<u32>>> = OnceLock::new();

/// Cache the Firefox version observed at handshake so later commands can
/// surface it in `meta.connection` without re-reading the greeting.
pub fn remember_version(version: Option<u32>) {
    if version.is_none() {
        return;
    }
    let lock = REMEMBERED_VERSION.get_or_init(|| std::sync::Mutex::new(None));
    let mut guard = lock
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    *guard = version;
}

/// Return the Firefox major version observed at the most recent handshake,
/// if any.  Used by error-path code that needs to mention the version in a
/// user-facing message (e.g. screenshot version-mismatch hint).
pub fn remembered_version() -> Option<u32> {
    let lock = REMEMBERED_VERSION.get_or_init(|| std::sync::Mutex::new(None));
    let guard = lock
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    *guard
}

fn cached_owner(host: &str, port: u16) -> Option<PortOwner> {
    // Only cache for loopback hosts. A remote port would require a different
    // lookup strategy entirely; we just skip the cache for those.
    let lock = OWNER_CACHE.get_or_init(|| std::sync::Mutex::new(Vec::new()));
    let mut guard = lock
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let key = (host.to_owned(), port);
    if let Some((_, owner)) = guard.iter().find(|(k, _)| k == &key) {
        return owner.clone();
    }
    let owner = port_owner::find_listener(port).ok().flatten();
    guard.push((key, owner.clone()));
    owner
}

/// Build the `meta.connection` JSON object.
///
/// `firefox_version` comes from the RDP greeting (parsed via
/// [`RdpConnection::firefox_version`]). `host` and `port` are the values the
/// CLI used to reach Firefox. PID and uptime are looked up from the OS port
/// table on a best-effort basis; missing fields are simply omitted.
pub fn build(host: &str, port: u16, firefox_version: Option<u32>) -> Value {
    let mut obj = serde_json::Map::new();
    obj.insert("host".to_string(), Value::String(host.to_owned()));
    obj.insert("port".to_string(), json!(port));
    let version = firefox_version.or_else(remembered_version);
    if let Some(v) = version {
        obj.insert("firefox_version".to_string(), json!(v));
    }
    if is_loopback(host)
        && let Some(owner) = cached_owner(host, port)
    {
        obj.insert("connected_pid".to_string(), json!(owner.pid));
        if !owner.process_name.is_empty() {
            obj.insert(
                "connected_process".to_string(),
                Value::String(owner.process_name),
            );
        }
        if let Some(uptime) = owner.uptime_s {
            obj.insert("uptime_s".to_string(), json!(uptime));
        }
    }
    Value::Object(obj)
}

pub(crate) fn is_loopback(host: &str) -> bool {
    matches!(host, "localhost" | "127.0.0.1" | "::1")
}

/// Merge a connection block into an existing `meta` JSON value.
///
/// Used by commands that build a custom meta object: they call this once
/// before constructing the envelope so the resulting `meta` carries
/// `host`, `port`, and `connection`.
pub fn merge_into(meta: &mut Value, host: &str, port: u16, firefox_version: Option<u32>) {
    if let Some(obj) = meta.as_object_mut() {
        obj.insert("connection".to_string(), build(host, port, firefox_version));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_includes_host_and_port() {
        let meta = build("127.0.0.1", 6000, None);
        assert_eq!(meta["host"], "127.0.0.1");
        assert_eq!(meta["port"], 6000);
    }

    #[test]
    fn build_includes_firefox_version_when_known() {
        let meta = build("127.0.0.1", 6000, Some(149));
        assert_eq!(meta["firefox_version"], 149);
    }

    #[test]
    fn build_omits_firefox_version_when_unknown() {
        let meta = build("127.0.0.1", 6000, None);
        assert!(meta.get("firefox_version").is_none());
    }

    #[test]
    fn merge_into_adds_connection_field() {
        let mut meta = json!({"host": "127.0.0.1", "port": 6000});
        merge_into(&mut meta, "127.0.0.1", 6000, Some(149));
        assert!(meta["connection"].is_object());
        assert_eq!(meta["connection"]["firefox_version"], 149);
    }
}
