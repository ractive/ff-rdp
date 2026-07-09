use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::actor::actor_request;
use crate::error::ProtocolError;
use crate::transport::RdpTransport;
use crate::types::ActorId;

/// Metadata for a browser tab as returned by the root actor's `listTabs`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TabInfo {
    /// The tab descriptor actor ID.
    pub actor: ActorId,
    /// Page title.
    #[serde(default)]
    pub title: String,
    /// Current URL.
    #[serde(default)]
    pub url: String,
    /// Whether this tab is currently selected/active.
    #[serde(default)]
    pub selected: bool,
    /// Browsing context identifier (may be absent in older Firefox versions).
    ///
    /// Firefox sends this as `browsingContextID` (uppercase D), which does not
    /// match the `camelCase` rename of this field name, so we override it.
    #[serde(default, rename = "browsingContextID")]
    pub browsing_context_id: Option<u64>,
}

/// Actor IDs returned by `getTarget` on a tab descriptor.
#[derive(Debug, Clone)]
pub struct TargetInfo {
    /// The WindowGlobalTarget actor ID (used for navigation, reload, etc.).
    pub actor: ActorId,
    /// The WebConsole actor ID (used for JS evaluation).
    pub console_actor: ActorId,
    /// The thread actor ID.
    pub thread_actor: Option<ActorId>,
    /// The inspector actor ID.
    pub inspector_actor: Option<ActorId>,
    /// The screenshot content actor ID (for screenshots without drawWindow).
    pub screenshot_content_actor: Option<ActorId>,
    /// The accessibility actor ID (for accessibility tree inspection).
    pub accessibility_actor: Option<ActorId>,
    /// The responsive design actor ID (for viewport size emulation).
    ///
    /// Present on Firefox ≥ 68 with RDM support.  Used by the `responsive`
    /// command to call `setViewportSize` instead of the browser-blocked
    /// `window.resizeTo()`.
    pub responsive_actor: Option<ActorId>,
    /// The manifest actor ID (for `fetchCanonicalManifest`).
    ///
    /// Present on the target frame as `manifestActor`; created lazily by
    /// Firefox on first access.  Used by the `manifest` command to fetch the
    /// parsed Web App Manifest plus its conformance errors.
    pub manifest_actor: Option<ActorId>,
    /// The browsing context ID for this target.
    ///
    /// Required by the Firefox 149+ two-step screenshot protocol:
    /// `screenshotContentActor.prepareCapture` + `screenshotActor.capture`.
    pub browsing_context_id: Option<u64>,
}

/// Inspect a `tabNavigated` push packet and emit a `tracing::warn!` when the
/// scheme of the new URL differs from the scheme of `previous_url`.
///
/// This is observability, not enforcement: Firefox already blocks dangerous
/// transitions like `http→file` and `https→javascript`.  The warning exists
/// so that a user driving ff-rdp from a script notices that a redirect
/// crossed a scheme boundary — their automation may not expect a
/// `https://example.com/foo` request to land on `about:neterror` or `file:`.
///
/// Returns `true` when a scheme change was detected and the warning was
/// emitted; `false` otherwise (no URL in the packet, no previous URL, or
/// schemes match).  The boolean lets unit tests assert the behaviour without
/// installing a `tracing` subscriber.
pub fn note_tab_navigated_scheme_change(packet: &Value, previous_url: &str) -> bool {
    let Some(new_url) = packet.get("url").and_then(Value::as_str) else {
        return false;
    };
    let new_scheme = scheme_of(new_url);
    let old_scheme = scheme_of(previous_url);
    if new_scheme.is_empty() || old_scheme.is_empty() || new_scheme == old_scheme {
        return false;
    }
    tracing::warn!(
        target: "ff_rdp_core::actors::tab",
        event = "tabNavigated.scheme_changed",
        from_scheme = %old_scheme,
        to_scheme = %new_scheme,
        from_url = %previous_url,
        to_url = %new_url,
        "tabNavigated: scheme changed across redirect — script automation may not expect this transition",
    );
    true
}

/// Extract the lower-cased scheme of a URL by taking everything before the
/// first `':'`.  Returns an empty slice when no colon is present.
fn scheme_of(url: &str) -> String {
    match url.find(':') {
        Some(i) => url[..i].to_ascii_lowercase(),
        None => String::new(),
    }
}

/// Operations on a tab descriptor actor.
pub struct TabActor;

impl TabActor {
    /// Call `getTarget` on a tab descriptor to obtain the WindowGlobalTarget
    /// and associated actor IDs (console, thread, inspector).
    pub fn get_target(
        transport: &mut RdpTransport,
        tab_actor: &ActorId,
    ) -> Result<TargetInfo, ProtocolError> {
        let response = actor_request(transport, tab_actor.as_ref(), "getTarget", None)?;
        parse_target_response(&response)
    }

    /// Call `getTarget` on a **process descriptor** actor.
    ///
    /// Process descriptors (from `listProcesses`) wrap their target inside a
    /// `"process"` key instead of the `"frame"` key used by tab descriptors.
    ///
    /// ```json
    /// { "process": { "actor": "...", "consoleActor": "...", ... }, "from": "..." }
    /// ```
    pub fn get_process_target(
        transport: &mut RdpTransport,
        process_actor: &ActorId,
    ) -> Result<TargetInfo, ProtocolError> {
        let response = actor_request(transport, process_actor.as_ref(), "getTarget", None)?;
        parse_process_target_response(&response)
    }

    /// Call `getWatcher` on a tab descriptor to obtain the watcher actor ID.
    pub fn get_watcher(
        transport: &mut RdpTransport,
        tab_actor: &ActorId,
    ) -> Result<ActorId, ProtocolError> {
        let response = actor_request(transport, tab_actor.as_ref(), "getWatcher", None)?;

        let watcher_actor = response
            .get("actor")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                ProtocolError::InvalidPacket("getWatcher response missing 'actor' field".into())
            })?;

        Ok(watcher_actor.into())
    }
}

/// Extract [`TargetInfo`] from a raw `getTarget` RDP response.
///
/// Firefox wraps all target fields inside a `"frame"` object:
/// ```json
/// { "frame": { "actor": "...", "consoleActor": "...", ... }, "from": "..." }
/// ```
fn parse_target_response(response: &Value) -> Result<TargetInfo, ProtocolError> {
    let frame = response.get("frame").ok_or_else(|| {
        ProtocolError::InvalidPacket("getTarget response missing 'frame' object".into())
    })?;
    parse_target_response_inner(frame, "frame")
}

/// Extract [`TargetInfo`] from a `getTarget` response issued to a process descriptor.
///
/// Process descriptor responses wrap the target fields in a `"process"` key:
/// ```json
/// { "process": { "actor": "...", "consoleActor": "...", ... }, "from": "..." }
/// ```
fn parse_process_target_response(response: &Value) -> Result<TargetInfo, ProtocolError> {
    let process = response.get("process").ok_or_else(|| {
        ProtocolError::InvalidPacket("process getTarget response missing 'process' object".into())
    })?;
    parse_target_response_inner(process, "process")
}

/// Shared field extraction logic for both `parse_target_response` and
/// `parse_process_target_response`.
///
/// `inner` is the wrapper object (`frame` or `process`), and `wrapper_key` is
/// its name (used only in error messages).
fn parse_target_response_inner(
    inner: &Value,
    wrapper_key: &str,
) -> Result<TargetInfo, ProtocolError> {
    let actor = inner.get("actor").and_then(Value::as_str).ok_or_else(|| {
        ProtocolError::InvalidPacket(format!(
            "getTarget response '{wrapper_key}' missing 'actor' field"
        ))
    })?;

    let console_actor = inner
        .get("consoleActor")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            ProtocolError::InvalidPacket(format!(
                "getTarget response '{wrapper_key}' missing 'consoleActor' field"
            ))
        })?;

    let thread_actor = inner
        .get("threadActor")
        .and_then(Value::as_str)
        .map(ActorId::from);

    let inspector_actor = inner
        .get("inspectorActor")
        .and_then(Value::as_str)
        .map(ActorId::from);

    let screenshot_content_actor = inner
        .get("screenshotContentActor")
        .and_then(Value::as_str)
        .map(ActorId::from);

    let accessibility_actor = inner
        .get("accessibilityActor")
        .and_then(Value::as_str)
        .map(ActorId::from);

    let responsive_actor = inner
        .get("responsiveActor")
        .and_then(Value::as_str)
        .map(ActorId::from);

    let manifest_actor = inner
        .get("manifestActor")
        .and_then(Value::as_str)
        .map(ActorId::from);

    let browsing_context_id = inner.get("browsingContextID").and_then(Value::as_u64);

    Ok(TargetInfo {
        actor: actor.into(),
        console_actor: console_actor.into(),
        thread_actor,
        inspector_actor,
        screenshot_content_actor,
        accessibility_actor,
        responsive_actor,
        manifest_actor,
        browsing_context_id,
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn tab_info_deserializes_from_firefox_response() {
        let v = json!({
            "actor": "server1.conn0.tabDescriptor1",
            "title": "Example",
            "url": "https://example.com",
            "selected": true,
            "browsingContextID": 42
        });
        let tab: TabInfo = serde_json::from_value(v).unwrap();
        assert_eq!(tab.actor.as_ref(), "server1.conn0.tabDescriptor1");
        assert_eq!(tab.title, "Example");
        assert_eq!(tab.url, "https://example.com");
        assert!(tab.selected);
        assert_eq!(tab.browsing_context_id, Some(42));
    }

    #[test]
    fn tab_info_handles_missing_optional_fields() {
        let v = json!({
            "actor": "server1.conn0.tabDescriptor1"
        });
        let tab: TabInfo = serde_json::from_value(v).unwrap();
        assert_eq!(tab.title, "");
        assert_eq!(tab.url, "");
        assert!(!tab.selected);
        assert_eq!(tab.browsing_context_id, None);
    }

    #[test]
    fn tab_info_serializes_to_json() {
        let tab = TabInfo {
            actor: ActorId::from("tab1"),
            title: "Test".into(),
            url: "https://test.com".into(),
            selected: false,
            browsing_context_id: Some(1),
        };
        let v = serde_json::to_value(&tab).unwrap();
        assert_eq!(v["actor"], "tab1");
        assert_eq!(v["browsingContextID"], 1);
    }

    // --- parse_target_response ---

    #[test]
    fn parse_target_response_extracts_frame_fields() {
        let response = json!({
            "frame": {
                "actor": "server1.conn3.child2/windowGlobalTarget2",
                "consoleActor": "server1.conn3.child2/consoleActor3",
                "threadActor": "server1.conn3.child2/thread1",
                "inspectorActor": "server1.conn3.child2/inspectorActor4",
                "screenshotContentActor": "server1.conn3.child2/screenshotContentActor5",
                "accessibilityActor": "server1.conn3.child2/accessibilityActor6"
            },
            "from": "server1.conn3.tabDescriptor1"
        });
        let info = parse_target_response(&response).unwrap();
        assert_eq!(
            info.actor.as_ref(),
            "server1.conn3.child2/windowGlobalTarget2"
        );
        assert_eq!(
            info.console_actor.as_ref(),
            "server1.conn3.child2/consoleActor3"
        );
        assert_eq!(
            info.thread_actor.as_ref().map(ActorId::as_ref),
            Some("server1.conn3.child2/thread1")
        );
        assert_eq!(
            info.inspector_actor.as_ref().map(ActorId::as_ref),
            Some("server1.conn3.child2/inspectorActor4")
        );
        assert_eq!(
            info.screenshot_content_actor.as_ref().map(ActorId::as_ref),
            Some("server1.conn3.child2/screenshotContentActor5")
        );
        assert_eq!(
            info.accessibility_actor.as_ref().map(ActorId::as_ref),
            Some("server1.conn3.child2/accessibilityActor6")
        );
    }

    #[test]
    fn parse_target_response_optional_actors_absent() {
        let response = json!({
            "frame": {
                "actor": "server1.conn3.child2/windowGlobalTarget2",
                "consoleActor": "server1.conn3.child2/consoleActor3"
            },
            "from": "server1.conn3.tabDescriptor1"
        });
        let info = parse_target_response(&response).unwrap();
        assert!(info.thread_actor.is_none());
        assert!(info.inspector_actor.is_none());
        assert!(info.screenshot_content_actor.is_none());
        assert!(info.accessibility_actor.is_none());
    }

    #[test]
    fn parse_target_response_missing_frame_returns_error() {
        let response = json!({
            "actor": "server1.conn3.child2/windowGlobalTarget2",
            "from": "server1.conn3.tabDescriptor1"
        });
        let err = parse_target_response(&response).unwrap_err();
        assert!(
            err.to_string().contains("'frame'"),
            "error should mention 'frame': {err}"
        );
    }

    #[test]
    fn parse_target_response_missing_actor_in_frame_returns_error() {
        let response = json!({
            "frame": {
                "consoleActor": "server1.conn3.child2/consoleActor3"
            },
            "from": "server1.conn3.tabDescriptor1"
        });
        let err = parse_target_response(&response).unwrap_err();
        assert!(
            err.to_string().contains("'actor'"),
            "error should mention 'actor': {err}"
        );
    }

    #[test]
    fn parse_target_response_missing_console_actor_returns_error() {
        let response = json!({
            "frame": {
                "actor": "server1.conn3.child2/windowGlobalTarget2"
            },
            "from": "server1.conn3.tabDescriptor1"
        });
        let err = parse_target_response(&response).unwrap_err();
        assert!(
            err.to_string().contains("'consoleActor'"),
            "error should mention 'consoleActor': {err}"
        );
    }

    // --- parse_process_target_response ---

    #[test]
    fn parse_process_target_response_happy_path() {
        let response = json!({
            "process": {
                "actor": "server1.conn0.processDescriptor1/windowGlobalTarget1",
                "consoleActor": "server1.conn0.processDescriptor1/consoleActor2",
                "threadActor": "server1.conn0.processDescriptor1/thread1",
                "inspectorActor": "server1.conn0.processDescriptor1/inspectorActor3",
                "screenshotContentActor": "server1.conn0.processDescriptor1/screenshotContentActor4",
                "accessibilityActor": "server1.conn0.processDescriptor1/accessibilityActor5",
                "browsingContextID": 99
            },
            "from": "server1.conn0.processDescriptor1"
        });
        let info = parse_process_target_response(&response).unwrap();
        assert_eq!(
            info.actor.as_ref(),
            "server1.conn0.processDescriptor1/windowGlobalTarget1"
        );
        assert_eq!(
            info.console_actor.as_ref(),
            "server1.conn0.processDescriptor1/consoleActor2"
        );
        assert_eq!(
            info.thread_actor.as_ref().map(ActorId::as_ref),
            Some("server1.conn0.processDescriptor1/thread1")
        );
        assert_eq!(
            info.inspector_actor.as_ref().map(ActorId::as_ref),
            Some("server1.conn0.processDescriptor1/inspectorActor3")
        );
        assert_eq!(
            info.screenshot_content_actor.as_ref().map(ActorId::as_ref),
            Some("server1.conn0.processDescriptor1/screenshotContentActor4")
        );
        assert_eq!(
            info.accessibility_actor.as_ref().map(ActorId::as_ref),
            Some("server1.conn0.processDescriptor1/accessibilityActor5")
        );
        assert_eq!(info.browsing_context_id, Some(99));
    }

    #[test]
    fn parse_process_target_response_optional_fields_absent() {
        let response = json!({
            "process": {
                "actor": "server1.conn0.processDescriptor1/windowGlobalTarget1",
                "consoleActor": "server1.conn0.processDescriptor1/consoleActor2"
            },
            "from": "server1.conn0.processDescriptor1"
        });
        let info = parse_process_target_response(&response).unwrap();
        assert!(info.thread_actor.is_none());
        assert!(info.inspector_actor.is_none());
        assert!(info.screenshot_content_actor.is_none());
        assert!(info.accessibility_actor.is_none());
        assert!(info.browsing_context_id.is_none());
    }

    #[test]
    fn parse_process_target_response_missing_process_wrapper_returns_error() {
        let response = json!({
            "actor": "server1.conn0.processDescriptor1/windowGlobalTarget1",
            "from": "server1.conn0.processDescriptor1"
        });
        let err = parse_process_target_response(&response).unwrap_err();
        assert!(
            err.to_string().contains("'process'"),
            "error should mention 'process': {err}"
        );
    }

    // --- note_tab_navigated_scheme_change (iter-75 E) ---

    /// AC: `tab_navigated_scheme_change_warns` — synthetic `tabNavigated`
    /// crossing a scheme boundary (`https→file`) must fire the helper's
    /// scheme-change branch.  Unit-level: we don't install a subscriber, we
    /// only assert the return value tracks the detection logic.
    #[test]
    fn tab_navigated_scheme_change_warns() {
        let pkt = json!({
            "from": "server1.conn0.child0/tab0",
            "type": "tabNavigated",
            "url": "file:///tmp/leak.txt"
        });
        assert!(
            note_tab_navigated_scheme_change(&pkt, "https://example.com/redirect"),
            "https→file scheme change must be flagged"
        );
        assert!(
            note_tab_navigated_scheme_change(&pkt, "http://example.com/r"),
            "http→file scheme change must be flagged"
        );

        // Same scheme — no warning expected.
        let same_scheme = json!({
            "from": "server1.conn0.child0/tab0",
            "type": "tabNavigated",
            "url": "https://example.com/landing"
        });
        assert!(
            !note_tab_navigated_scheme_change(&same_scheme, "https://example.com/start"),
            "same scheme must not warn"
        );

        // No url field — silent no-op (Firefox sometimes emits the event
        // without a URL during early initialization).
        let no_url = json!({"from": "tab0", "type": "tabNavigated"});
        assert!(!note_tab_navigated_scheme_change(
            &no_url,
            "https://example.com/"
        ));
    }

    #[test]
    fn parse_process_target_response_missing_console_actor_returns_error() {
        let response = json!({
            "process": {
                "actor": "server1.conn0.processDescriptor1/windowGlobalTarget1"
            },
            "from": "server1.conn0.processDescriptor1"
        });
        let err = parse_process_target_response(&response).unwrap_err();
        assert!(
            err.to_string().contains("'consoleActor'"),
            "error should mention 'consoleActor': {err}"
        );
    }
}
