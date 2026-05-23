//! Spec for the Watcher actor.
//!
//! Mirrors <https://searchfox.org/mozilla-central/source/devtools/shared/specs/watcher.js>

use serde::{Deserialize, Serialize};

use super::{Method, sealed};

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
    #[derive(Debug, Clone, Default, Serialize)]
    pub struct UnwatchTargets {
        #[serde(rename = "targetType")]
        pub target_type: String,
    }
}

// ---------------------------------------------------------------------------
// Response types
// ---------------------------------------------------------------------------

pub mod response {
    use super::Deserialize;

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
pub struct UnwatchTargets;
impl sealed::Sealed for UnwatchTargets {}
impl Method for UnwatchTargets {
    const NAME: &'static str = "unwatchTargets";
    type Args = request::UnwatchTargets;
    type Reply = response::UnwatchTargets;
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
    }
}
