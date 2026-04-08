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
    #[serde(deserialize_with = "deserialize_actor_id")]
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

    let actor = frame.get("actor").and_then(Value::as_str).ok_or_else(|| {
        ProtocolError::InvalidPacket("getTarget response 'frame' missing 'actor' field".into())
    })?;

    let console_actor = frame
        .get("consoleActor")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            ProtocolError::InvalidPacket(
                "getTarget response 'frame' missing 'consoleActor' field".into(),
            )
        })?;

    let thread_actor = frame
        .get("threadActor")
        .and_then(Value::as_str)
        .map(ActorId::from);

    let inspector_actor = frame
        .get("inspectorActor")
        .and_then(Value::as_str)
        .map(ActorId::from);

    let screenshot_content_actor = frame
        .get("screenshotContentActor")
        .and_then(Value::as_str)
        .map(ActorId::from);

    let accessibility_actor = frame
        .get("accessibilityActor")
        .and_then(Value::as_str)
        .map(ActorId::from);

    let responsive_actor = frame
        .get("responsiveActor")
        .and_then(Value::as_str)
        .map(ActorId::from);

    Ok(TargetInfo {
        actor: actor.into(),
        console_actor: console_actor.into(),
        thread_actor,
        inspector_actor,
        screenshot_content_actor,
        accessibility_actor,
        responsive_actor,
    })
}

fn deserialize_actor_id<'de, D>(deserializer: D) -> Result<ActorId, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    Ok(ActorId::from(s))
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
}
