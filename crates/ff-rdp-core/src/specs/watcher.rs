//! Spec for the Watcher actor.
//!
//! Mirrors <https://searchfox.org/mozilla-central/source/devtools/shared/specs/watcher.js>

use serde::{Deserialize, Serialize};

use super::{Method, NoArgs, sealed};

// ---------------------------------------------------------------------------
// Request args
// ---------------------------------------------------------------------------

pub mod request {
    use super::Serialize;

    /// Args for `watchResources` — subscribe to one or more resource types.
    #[derive(Debug, Clone, Default, Serialize)]
    pub struct WatchResources {
        #[serde(rename = "resourceTypes")]
        pub resource_types: Vec<String>,
    }

    /// Args for `unwatchResources` — unsubscribe from one or more resource types.
    #[derive(Debug, Clone, Default, Serialize)]
    pub struct UnwatchResources {
        #[serde(rename = "resourceTypes")]
        pub resource_types: Vec<String>,
    }

    /// Args for `watchTargets` — subscribe to target events of the given type.
    #[derive(Debug, Clone, Default, Serialize)]
    pub struct WatchTargets {
        #[serde(rename = "targetType")]
        pub target_type: String,
    }

    /// Args for `unwatchTargets` — unsubscribe from target events.
    ///
    /// Per `devtools/shared/specs/watcher.js:20-32`, the request takes
    /// `(targetType, options)`.  `options` is forwarded verbatim when
    /// provided; omitted entries fall back to the server's defaults.
    #[derive(Debug, Clone, Default, Serialize)]
    pub struct UnwatchTargets {
        #[serde(rename = "targetType")]
        pub target_type: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub options: Option<serde_json::Value>,
    }

    /// Args for `clearResources` — clear resources for given types (oneway).
    #[derive(Debug, Clone, Default, Serialize)]
    pub struct ClearResources {
        #[serde(rename = "resourceTypes")]
        pub resource_types: Vec<String>,
    }

    /// Args for `getParentBrowsingContextID`.
    #[derive(Debug, Clone, Default, Serialize)]
    pub struct GetParentBrowsingContextId {
        #[serde(rename = "browsingContextID")]
        pub browsing_context_id: u64,
    }
}

// ---------------------------------------------------------------------------
// Response types
// ---------------------------------------------------------------------------

pub mod response {
    use super::Deserialize;
    use crate::types::ActorId;

    /// Reply for `watchResources` — empty acknowledgement.
    #[derive(Debug, Clone, Default, Deserialize)]
    pub struct WatchResources {}

    /// Reply for `unwatchResources` — empty acknowledgement.
    #[derive(Debug, Clone, Default, Deserialize)]
    pub struct UnwatchResources {}

    /// Reply for `watchTargets` — empty acknowledgement.
    #[derive(Debug, Clone, Default, Deserialize)]
    pub struct WatchTargets {}

    /// Reply for `unwatchTargets` — empty acknowledgement.
    #[derive(Debug, Clone, Default, Deserialize)]
    pub struct UnwatchTargets {}

    /// Reply for `clearResources` — oneway, no reply expected.
    #[derive(Debug, Clone, Default, Deserialize)]
    pub struct ClearResources {}

    /// Reply for `getTargetConfigurationActor`.
    ///
    /// Firefox returns `{"configuration": {"actor": "<id>", …}, "from": …}` —
    /// the actor ID is nested inside the typed-actor `configuration` object, not
    /// at the top level. This type reads `configuration.actor` (verified against
    /// a live Firefox trace in iter-103).
    #[derive(Debug, Clone, Deserialize)]
    pub struct ConfigurationActorRef {
        pub configuration: NestedActorId,
    }

    /// Reply for `getBlackboxingActor`.
    #[derive(Debug, Clone, Deserialize)]
    pub struct BlackboxingActorRef {
        pub blackboxing: NestedActorId,
    }

    /// Reply for `getBreakpointListActor`.
    #[derive(Debug, Clone, Deserialize)]
    pub struct BreakpointListActorRef {
        #[serde(rename = "breakpointList")]
        pub breakpoint_list: NestedActorId,
    }

    /// Reply for `getThreadConfigurationActor`.
    #[derive(Debug, Clone, Deserialize)]
    pub struct ThreadConfigurationActorRef {
        pub configuration: NestedActorId,
    }

    /// Reply for `getNetworkParentActor`.
    ///
    /// Firefox returns `{"network": {"actor": "<id>", …}, "from": …}` — the
    /// actor ID is nested inside the typed-actor object keyed **`network`**, not
    /// `networkParent`. iter-109 guessed the key was `networkParent` (by analogy
    /// to the method name and `ConfigurationActorRef`'s `configuration` key) but
    /// never live-verified it; iter-110's full-suite sweep caught the mismatch
    /// (`decode getNetworkParentActor: missing field networkParent`) and the
    /// real key was captured from Firefox 152:
    /// `{"network":{"actor":"…networkParent12"},"from":"…"}`. The named key does
    /// NOT match the method name here — unlike `configuration`/`network`
    /// asymmetry, each `watcher.js` accessor picks its own key.
    #[derive(Debug, Clone, Deserialize)]
    pub struct NetworkParentActorRef {
        #[serde(rename = "network")]
        pub network_parent: NestedActorId,
    }

    /// The typed-actor payload wrapped by named-key watcher accessor responses.
    ///
    /// Only `actor` is needed by callers; the rest of the payload
    /// (`configuration`, `traits`) is ignored.
    #[derive(Debug, Clone, Deserialize)]
    pub struct NestedActorId {
        pub actor: ActorId,
    }

    /// Reply for `getParentBrowsingContextID`.
    #[derive(Debug, Clone, Default, Deserialize)]
    pub struct GetParentBrowsingContextId {
        #[serde(rename = "browsingContextID", default)]
        pub browsing_context_id: Option<u64>,
    }
}

// ---------------------------------------------------------------------------
// Method markers
// ---------------------------------------------------------------------------

/// `watchResources` method marker.
pub struct WatchResources;
impl sealed::Sealed for WatchResources {}
impl Method for WatchResources {
    const NAME: &'static str = "watchResources";
    type Args = request::WatchResources;
    type Reply = response::WatchResources;
}

/// `unwatchResources` method marker.
pub struct UnwatchResources;
impl sealed::Sealed for UnwatchResources {}
impl Method for UnwatchResources {
    const NAME: &'static str = "unwatchResources";
    type Args = request::UnwatchResources;
    type Reply = response::UnwatchResources;
    const ONEWAY: bool = true;
}

/// `watchTargets` method marker.
pub struct WatchTargets;
impl sealed::Sealed for WatchTargets {}
impl Method for WatchTargets {
    const NAME: &'static str = "watchTargets";
    type Args = request::WatchTargets;
    type Reply = response::WatchTargets;
}

/// `unwatchTargets` method marker.
///
/// This method is fire-and-forget (`oneway: true` in Firefox's spec) — Firefox
/// does not send a reply packet.  Setting `ONEWAY = true` prevents the reply
/// read that would otherwise hang on CLI shutdown.
pub struct UnwatchTargets;
impl sealed::Sealed for UnwatchTargets {}
impl Method for UnwatchTargets {
    const NAME: &'static str = "unwatchTargets";
    type Args = request::UnwatchTargets;
    type Reply = response::UnwatchTargets;
    const ONEWAY: bool = true;
}

/// `clearResources` method marker.
///
/// Oneway — clears server-side resource caches for the given types.
pub struct ClearResources;
impl sealed::Sealed for ClearResources {}
impl Method for ClearResources {
    const NAME: &'static str = "clearResources";
    type Args = request::ClearResources;
    type Reply = response::ClearResources;
    const ONEWAY: bool = true;
}

/// `getParentBrowsingContextID` method marker.
pub struct GetParentBrowsingContextId;
impl sealed::Sealed for GetParentBrowsingContextId {}
impl Method for GetParentBrowsingContextId {
    const NAME: &'static str = "getParentBrowsingContextID";
    type Args = request::GetParentBrowsingContextId;
    type Reply = response::GetParentBrowsingContextId;
}

/// `getNetworkParentActor` method marker.
pub struct GetNetworkParentActor;
impl sealed::Sealed for GetNetworkParentActor {}
impl Method for GetNetworkParentActor {
    const NAME: &'static str = "getNetworkParentActor";
    type Args = NoArgs;
    type Reply = response::NetworkParentActorRef;
}

/// `getBlackboxingActor` method marker.
pub struct GetBlackboxingActor;
impl sealed::Sealed for GetBlackboxingActor {}
impl Method for GetBlackboxingActor {
    const NAME: &'static str = "getBlackboxingActor";
    type Args = NoArgs;
    type Reply = response::BlackboxingActorRef;
}

/// `getBreakpointListActor` method marker.
pub struct GetBreakpointListActor;
impl sealed::Sealed for GetBreakpointListActor {}
impl Method for GetBreakpointListActor {
    const NAME: &'static str = "getBreakpointListActor";
    type Args = NoArgs;
    type Reply = response::BreakpointListActorRef;
}

/// `getTargetConfigurationActor` method marker.
pub struct GetTargetConfigurationActor;
impl sealed::Sealed for GetTargetConfigurationActor {}
impl Method for GetTargetConfigurationActor {
    const NAME: &'static str = "getTargetConfigurationActor";
    type Args = NoArgs;
    type Reply = response::ConfigurationActorRef;
}

/// `getThreadConfigurationActor` method marker.
pub struct GetThreadConfigurationActor;
impl sealed::Sealed for GetThreadConfigurationActor {}
impl Method for GetThreadConfigurationActor {
    const NAME: &'static str = "getThreadConfigurationActor";
    type Args = NoArgs;
    type Reply = response::ThreadConfigurationActorRef;
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn watch_resources_request_serializes_resource_types() {
        let args = request::WatchResources {
            resource_types: vec!["network-event".into(), "console-message".into()],
        };
        let v = serde_json::to_value(&args).unwrap();
        assert_eq!(
            v["resourceTypes"],
            json!(["network-event", "console-message"])
        );
    }

    #[test]
    fn unwatch_resources_request_serializes_resource_types() {
        let args = request::UnwatchResources {
            resource_types: vec!["network-event".into()],
        };
        let v = serde_json::to_value(&args).unwrap();
        assert_eq!(v["resourceTypes"], json!(["network-event"]));
    }

    #[test]
    fn watch_targets_request_serializes_target_type() {
        let args = request::WatchTargets {
            target_type: "frame".into(),
        };
        let v = serde_json::to_value(&args).unwrap();
        assert_eq!(v["targetType"], "frame");
    }

    #[test]
    fn watch_resources_response_deserializes_empty_object() {
        let v = json!({"from": "server1.conn0.watcher4"});
        let _: response::WatchResources = serde_json::from_value(v).unwrap();
    }

    #[test]
    fn method_names_are_correct() {
        assert_eq!(WatchResources::NAME, "watchResources");
        assert_eq!(UnwatchResources::NAME, "unwatchResources");
        assert_eq!(WatchTargets::NAME, "watchTargets");
        assert_eq!(UnwatchTargets::NAME, "unwatchTargets");
        assert_eq!(ClearResources::NAME, "clearResources");
        assert_eq!(
            GetParentBrowsingContextId::NAME,
            "getParentBrowsingContextID"
        );
        assert_eq!(GetNetworkParentActor::NAME, "getNetworkParentActor");
        assert_eq!(GetBlackboxingActor::NAME, "getBlackboxingActor");
        assert_eq!(GetBreakpointListActor::NAME, "getBreakpointListActor");
        assert_eq!(
            GetTargetConfigurationActor::NAME,
            "getTargetConfigurationActor"
        );
        assert_eq!(
            GetThreadConfigurationActor::NAME,
            "getThreadConfigurationActor"
        );
    }

    #[test]
    fn oneway_flags_are_correct() {
        // oneway methods must set ONEWAY = true.
        const { assert!(UnwatchTargets::ONEWAY) };
        const { assert!(UnwatchResources::ONEWAY) };
        const { assert!(ClearResources::ONEWAY) };
        // Regular methods must NOT be oneway.
        const { assert!(!WatchResources::ONEWAY) };
        const { assert!(!WatchTargets::ONEWAY) };
        const { assert!(!GetNetworkParentActor::ONEWAY) };
        const { assert!(!GetTargetConfigurationActor::ONEWAY) };
    }

    #[test]
    fn named_actor_responses_deserialize() {
        let blackboxing = json!({"blackboxing": {"actor": "server1.conn0.blackboxing5"}});
        let r: response::BlackboxingActorRef = serde_json::from_value(blackboxing).unwrap();
        assert_eq!(r.blackboxing.actor.as_ref(), "server1.conn0.blackboxing5");

        let breakpoints = json!({"breakpointList": {"actor": "server1.conn0.breakpointList6"}});
        let r: response::BreakpointListActorRef = serde_json::from_value(breakpoints).unwrap();
        assert_eq!(
            r.breakpoint_list.actor.as_ref(),
            "server1.conn0.breakpointList6"
        );

        let configuration =
            json!({"configuration": {"actor": "server1.conn0.threadConfiguration7"}});
        let r: response::ThreadConfigurationActorRef =
            serde_json::from_value(configuration).unwrap();
        assert_eq!(
            r.configuration.actor.as_ref(),
            "server1.conn0.threadConfiguration7"
        );
    }

    #[test]
    fn network_parent_actor_ref_reads_nested_actor() {
        // Real Firefox 152 shape (captured live in iter-110): the actor is
        // nested under the typed-actor object keyed `network` — NOT
        // `networkParent` as iter-109 guessed. Raw reply:
        // {"network":{"actor":"…networkParent12"},"from":"…watcher11"}
        let v = json!({
            "from": "server1.conn2.watcher11",
            "network": {
                "actor": "server1.conn2.networkParent13",
                "traits": {}
            }
        });
        let r: response::NetworkParentActorRef = serde_json::from_value(v).unwrap();
        assert_eq!(
            r.network_parent.actor.as_ref(),
            "server1.conn2.networkParent13"
        );
    }

    #[test]
    fn configuration_actor_ref_reads_nested_actor() {
        // Real Firefox shape: the actor is nested under the typed-actor
        // `configuration` object, not at the top level (iter-103 fix).
        let v = json!({
            "from": "server1.conn2.watcher11",
            "configuration": {
                "actor": "server1.conn2.target-configuration12",
                "configuration": {},
                "traits": {"supportedOptions": {"colorSchemeSimulation": true}}
            }
        });
        let r: response::ConfigurationActorRef = serde_json::from_value(v).unwrap();
        assert_eq!(
            r.configuration.actor.as_ref(),
            "server1.conn2.target-configuration12"
        );
    }

    #[test]
    fn get_parent_browsing_context_id_deserializes() {
        let v = json!({"from": "server1.conn0.watcher4", "browsingContextID": 42});
        let r: response::GetParentBrowsingContextId = serde_json::from_value(v).unwrap();
        assert_eq!(r.browsing_context_id, Some(42));
    }

    #[test]
    fn get_parent_browsing_context_id_serializes_requested_id() {
        let args = request::GetParentBrowsingContextId {
            browsing_context_id: 42,
        };
        let v = serde_json::to_value(args).unwrap();
        assert_eq!(v, json!({"browsingContextID": 42}));
    }

    #[test]
    fn clear_resources_request_serializes() {
        let args = request::ClearResources {
            resource_types: vec!["network-event".into()],
        };
        let v = serde_json::to_value(&args).unwrap();
        assert_eq!(v["resourceTypes"], json!(["network-event"]));
    }
}
