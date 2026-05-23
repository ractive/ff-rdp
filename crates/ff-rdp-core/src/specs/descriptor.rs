//! Spec for the tab Descriptor actor (plays the role of a DevTools descriptor front).
//!
//! Mirrors <https://searchfox.org/mozilla-central/source/devtools/shared/specs/descriptors/tab.js>

use serde::Deserialize;

use super::{Method, NoArgs, sealed};

// Re-export TargetInfo so callers use the spec as the typed surface.
pub use crate::actors::tab::{TabInfo, TargetInfo};

// ---------------------------------------------------------------------------
// Request args
// ---------------------------------------------------------------------------

pub mod request {
    use super::NoArgs;

    /// Args for `getTarget` — no parameters.
    pub type GetTarget = NoArgs;

    /// Args for `getWatcher` — no parameters.
    pub type GetWatcher = NoArgs;
}

// ---------------------------------------------------------------------------
// Response types
// ---------------------------------------------------------------------------

pub mod response {
    use super::Deserialize;
    use crate::types::ActorId;

    /// A target frame object returned inside a `getTarget` response.
    #[derive(Debug, Clone, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct TargetFrame {
        pub actor: ActorId,
        pub console_actor: ActorId,
        #[serde(default)]
        pub thread_actor: Option<ActorId>,
        #[serde(default)]
        pub inspector_actor: Option<ActorId>,
        #[serde(default)]
        pub screenshot_content_actor: Option<ActorId>,
        #[serde(default)]
        pub accessibility_actor: Option<ActorId>,
        #[serde(default)]
        pub responsive_actor: Option<ActorId>,
        #[serde(rename = "browsingContextID", default)]
        pub browsing_context_id: Option<u64>,
    }

    /// Reply for `getTarget` (tab descriptor — wraps target in `"frame"`).
    #[derive(Debug, Clone, Default, Deserialize)]
    pub struct GetTarget {
        pub frame: Option<TargetFrame>,
    }

    /// Reply for `getWatcher`.
    #[derive(Debug, Clone, Deserialize)]
    pub struct GetWatcher {
        pub actor: ActorId,
    }
}

// ---------------------------------------------------------------------------
// Method markers
// ---------------------------------------------------------------------------

/// `getTarget` method marker.
pub struct GetTarget;
impl sealed::Sealed for GetTarget {}
impl Method for GetTarget {
    const NAME: &'static str = "getTarget";
    type Args = NoArgs;
    type Reply = response::GetTarget;
}

/// `getWatcher` method marker.
pub struct GetWatcher;
impl sealed::Sealed for GetWatcher {}
impl Method for GetWatcher {
    const NAME: &'static str = "getWatcher";
    type Args = NoArgs;
    type Reply = response::GetWatcher;
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn get_target_response_deserializes_frame() {
        let v = json!({
            "from": "server1.conn3.tabDescriptor1",
            "frame": {
                "actor": "server1.conn3.child2/windowGlobalTarget2",
                "consoleActor": "server1.conn3.child2/consoleActor3",
                "threadActor": "server1.conn3.child2/thread1",
                "browsingContextID": 55
            }
        });
        let reply: response::GetTarget = serde_json::from_value(v).unwrap();
        let frame = reply.frame.expect("frame should be present");
        assert_eq!(
            frame.actor.as_ref(),
            "server1.conn3.child2/windowGlobalTarget2"
        );
        assert_eq!(
            frame.console_actor.as_ref(),
            "server1.conn3.child2/consoleActor3"
        );
        assert_eq!(
            frame.thread_actor.as_ref().map(std::convert::AsRef::as_ref),
            Some("server1.conn3.child2/thread1")
        );
        assert_eq!(frame.browsing_context_id, Some(55));
    }

    #[test]
    fn get_target_response_optional_fields_absent() {
        let v = json!({
            "from": "server1.conn3.tabDescriptor1",
            "frame": {
                "actor": "server1.conn3.child2/windowGlobalTarget2",
                "consoleActor": "server1.conn3.child2/consoleActor3"
            }
        });
        let reply: response::GetTarget = serde_json::from_value(v).unwrap();
        let frame = reply.frame.expect("frame should be present");
        assert!(frame.thread_actor.is_none());
        assert!(frame.inspector_actor.is_none());
        assert!(frame.browsing_context_id.is_none());
    }

    #[test]
    fn get_watcher_response_deserializes_actor() {
        let v = json!({
            "from": "server1.conn3.tabDescriptor1",
            "actor": "server1.conn3.watcher4"
        });
        let reply: response::GetWatcher = serde_json::from_value(v).unwrap();
        assert_eq!(reply.actor.as_ref(), "server1.conn3.watcher4");
    }

    #[test]
    fn method_names_are_correct() {
        assert_eq!(GetTarget::NAME, "getTarget");
        assert_eq!(GetWatcher::NAME, "getWatcher");
    }
}
