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

static DAEMON_FALLBACK: OnceLock<std::sync::Mutex<Option<String>>> = OnceLock::new();

/// The `meta` key under which a silent daemon→direct fallback is reported
/// (iter-164).
pub(crate) const DAEMON_FALLBACK_KEY: &str = "daemon_fallback";

/// Record that the CLI asked for daemon mode, autostart did not produce a
/// usable daemon, and the command ran over a *direct* connection instead
/// (iter-164).
///
/// `resolve_connection_target` builds this diagnostic as
/// `ConnectionTarget::Direct::deferred_warning`, whose contract is
/// "print only if the direct fallback *also* fails". When the direct
/// connection succeeds — the common case under load — the string was simply
/// dropped, so a caller who asked for daemon mode and quietly got direct mode
/// had no signal at all. `meta.route` says `"direct"` but not *why*, and the
/// two registry-check error paths never reach
/// `daemon_status::record_autostart_failed`, so they produced no envelope
/// warning either.
///
/// Remembering it here lets [`merge_into`] surface the reason in `meta` under
/// `--verbose` instead of discarding it.
pub fn remember_daemon_fallback(reason: impl Into<String>) {
    let lock = DAEMON_FALLBACK.get_or_init(|| std::sync::Mutex::new(None));
    let mut guard = lock
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    *guard = Some(reason.into());
}

/// The daemon→direct fallback reason recorded for this process, if any.
pub fn remembered_daemon_fallback() -> Option<String> {
    let lock = DAEMON_FALLBACK.get_or_init(|| std::sync::Mutex::new(None));
    let guard = lock
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    guard.clone()
}

/// Serialization lock for tests that exercise the process-global
/// [`DAEMON_FALLBACK`] slot (iter-164).
///
/// Mirrors [`crate::daemon_status::test_lock`]: one process-wide slot means two
/// concurrently-running tests would observe each other's writes. Every test
/// that records/asserts must hold this for its whole sequence.
#[cfg(test)]
fn fallback_test_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: OnceLock<std::sync::Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| std::sync::Mutex::new(()))
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Clear the recorded fallback reason. Test-only: the slot is process-global,
/// so tests that assert on it must reset it.
#[cfg(test)]
fn clear_daemon_fallback() {
    let lock = DAEMON_FALLBACK.get_or_init(|| std::sync::Mutex::new(None));
    let mut guard = lock
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    *guard = None;
}

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

/// Merge `meta.route` — `"daemon"` or `"direct"` — into `meta` (iter-128
/// Theme D).
///
/// Unlike [`merge_into_if_verbose`]'s connection block, this is **not**
/// gated by `--verbose`: dogfooding found that an agent has no way to tell
/// how a command executed (daemon-buffered vs. one-shot direct connection)
/// without shelling out to `daemon status` and cross-referencing the
/// registry file. `via_daemon` is [`ConnectedTab::via_daemon`]
/// (`crate::commands::connect_tab::ConnectedTab`) — the RESOLVED route,
/// after any daemon-vs-direct fallback has already happened.
pub fn merge_route(meta: &mut Value, via_daemon: bool) {
    if let Some(obj) = meta.as_object_mut() {
        obj.insert(
            "route".to_string(),
            Value::String(if via_daemon { "daemon" } else { "direct" }.to_owned()),
        );
    }
}

/// Merge `meta.source` — `"native"` or `"js-fallback"` — into `meta`
/// (iter-143 Theme A).
///
/// Same unconditional treatment as [`merge_route`]: a caller scoring
/// accessibility output needs to know which tree it is looking at — the
/// native platform tree (real accessible roles like `document`/`paragraph`)
/// or the DOM-derived approximation (`generic`, …) — without a separate
/// `--verbose` round-trip. `reason` is populated only when `source` is
/// `"js-fallback"`, naming why the native path was not used (e.g.
/// `"accessibility-service-disabled"`, `"selector-mode"`).
pub fn merge_source(meta: &mut Value, source: &str, reason: Option<&str>) {
    if let Some(obj) = meta.as_object_mut() {
        obj.insert("source".to_string(), Value::String(source.to_owned()));
        if let Some(r) = reason {
            obj.insert("source_reason".to_string(), Value::String(r.to_owned()));
        }
    }
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
///
/// **Deprecated in favour of [`merge_into_if_verbose`].** This unconditional
/// variant is kept for the `doctor` command which always wants connection info.
pub fn merge_into(meta: &mut Value, host: &str, port: u16, firefox_version: Option<u32>) {
    if let Some(obj) = meta.as_object_mut() {
        obj.insert("connection".to_string(), build(host, port, firefox_version));
        // iter-164: a silent daemon→direct fallback is reported here rather
        // than thrown away (see `remember_daemon_fallback`).
        if let Some(reason) = remembered_daemon_fallback() {
            obj.insert(DAEMON_FALLBACK_KEY.to_string(), Value::String(reason));
        }
    }
}

/// Merge a connection block into `meta` **only** when `verbose` is true.
///
/// This is the standard call-site for all browser-touching commands: in
/// default (non-verbose) mode the connection block is omitted to keep
/// responses compact. Pass `--verbose` to restore it.
pub fn merge_into_if_verbose(
    meta: &mut Value,
    host: &str,
    port: u16,
    firefox_version: Option<u32>,
    verbose: bool,
) {
    if verbose {
        merge_into(meta, host, port, firefox_version);
    }
}

/// Omit `meta` from the envelope when it is empty or contains only null/empty
/// fields.  Used by [`crate::output::envelope`] variants to suppress a bare
/// `"meta": {}`.
pub fn is_meta_empty(meta: &Value) -> bool {
    match meta {
        Value::Null => true,
        Value::Object(map) => map.is_empty(),
        _ => false,
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

    #[test]
    fn merge_into_if_verbose_adds_connection_when_true() {
        let mut meta = json!({});
        merge_into_if_verbose(&mut meta, "127.0.0.1", 6000, None, true);
        assert!(
            meta.get("connection").is_some(),
            "connection must be added when verbose=true"
        );
    }

    #[test]
    fn merge_into_if_verbose_omits_connection_when_false() {
        let mut meta = json!({});
        merge_into_if_verbose(&mut meta, "127.0.0.1", 6000, None, false);
        assert!(
            meta.get("connection").is_none(),
            "connection must be absent when verbose=false"
        );
    }

    #[test]
    fn is_meta_empty_returns_true_for_empty_object() {
        assert!(is_meta_empty(&json!({})));
    }

    #[test]
    fn is_meta_empty_returns_true_for_null() {
        assert!(is_meta_empty(&serde_json::Value::Null));
    }

    #[test]
    fn is_meta_empty_returns_false_for_non_empty_object() {
        assert!(!is_meta_empty(&json!({"selector": "h1"})));
    }

    /// iter-128 Theme D: `merge_route` maps `via_daemon` to the
    /// "daemon"/"direct" route strings and inserts them unconditionally
    /// (no `--verbose` gate, unlike `merge_into_if_verbose`).
    #[test]
    fn merge_route_daemon_true_maps_to_daemon_string() {
        let mut meta = json!({});
        merge_route(&mut meta, true);
        assert_eq!(meta["route"], "daemon");
    }

    #[test]
    fn merge_route_daemon_false_maps_to_direct_string() {
        let mut meta = json!({});
        merge_route(&mut meta, false);
        assert_eq!(meta["route"], "direct");
    }

    /// iter-143 Theme A: `merge_source` sets `source` unconditionally and
    /// omits `source_reason` when none is given (the native path).
    #[test]
    fn merge_source_native_omits_reason() {
        let mut meta = json!({});
        merge_source(&mut meta, "native", None);
        assert_eq!(meta["source"], "native");
        assert!(meta.get("source_reason").is_none());
    }

    #[test]
    fn merge_source_js_fallback_includes_reason() {
        let mut meta = json!({});
        merge_source(
            &mut meta,
            "js-fallback",
            Some("accessibility-service-disabled"),
        );
        assert_eq!(meta["source"], "js-fallback");
        assert_eq!(meta["source_reason"], "accessibility-service-disabled");
    }

    #[test]
    fn merge_source_makes_meta_non_empty() {
        let mut meta = json!({});
        merge_source(&mut meta, "native", None);
        assert!(!is_meta_empty(&meta), "meta with source must not be empty");
    }

    // ── iter-164 AC5 — a silent daemon→direct fallback is reported, not
    //    discarded. These share a process-global slot, so each clears it first
    //    and again at the end.

    /// `unit_164_silent_direct_fallback_is_reported`: when autostart failed and
    /// the command ran directly, `--verbose` meta must carry the reason.
    #[test]
    fn unit_164_silent_direct_fallback_is_reported() {
        let _guard = fallback_test_lock();
        clear_daemon_fallback();
        remember_daemon_fallback(
            "warning: daemon started but did not register within 20s \
             (registry write raced or was slow)",
        );
        let mut meta = json!({});
        merge_into_if_verbose(&mut meta, "127.0.0.1", 6000, None, true);
        merge_route(&mut meta, false);
        assert_eq!(meta["route"], "direct");
        let reported = meta[DAEMON_FALLBACK_KEY]
            .as_str()
            .expect("meta must report why daemon mode degraded to direct");
        assert!(
            reported.contains("did not register"),
            "the dropped deferred_warning must be surfaced verbatim, got {reported}"
        );
        clear_daemon_fallback();
    }

    /// Without `--verbose` the key stays out of the envelope — `meta.route`
    /// already says `"direct"`; the reason is the verbose detail.
    #[test]
    fn unit_164_fallback_reason_is_verbose_only() {
        let _guard = fallback_test_lock();
        clear_daemon_fallback();
        remember_daemon_fallback("warning: could not acquire daemon spawn lock");
        let mut meta = json!({});
        merge_into_if_verbose(&mut meta, "127.0.0.1", 6000, None, false);
        assert!(
            meta.get(DAEMON_FALLBACK_KEY).is_none(),
            "non-verbose meta must stay lean: {meta}"
        );
        clear_daemon_fallback();
    }

    /// The happy path (daemon actually used) records nothing, so the key is
    /// absent even under `--verbose`.
    #[test]
    fn unit_164_no_fallback_means_no_key() {
        let _guard = fallback_test_lock();
        clear_daemon_fallback();
        let mut meta = json!({});
        merge_into_if_verbose(&mut meta, "127.0.0.1", 6000, None, true);
        assert!(
            meta.get(DAEMON_FALLBACK_KEY).is_none(),
            "no fallback recorded -> key omitted: {meta}"
        );
    }

    #[test]
    fn merge_route_makes_meta_non_empty() {
        // Route must be visible in DEFAULT (non-verbose) output — merging it
        // into an otherwise-empty meta object must flip `is_meta_empty` to
        // false so the `meta` key survives envelope construction.
        let mut meta = json!({});
        merge_route(&mut meta, true);
        assert!(!is_meta_empty(&meta), "meta with route must not be empty");
    }
}
