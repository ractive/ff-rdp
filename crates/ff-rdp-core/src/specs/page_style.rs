//! Spec for the PageStyle actor.
//!
//! Mirrors <https://searchfox.org/mozilla-central/source/devtools/shared/specs/page-style.js>

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{Method, sealed};

// Re-export typed CSS types so callers use this spec module as their typed surface.
pub use crate::actors::page_style::{
    AppliedRule, BoxModelLayout, BoxSides, ComputedProperty, RuleProperty,
};

// ---------------------------------------------------------------------------
// Request args
// ---------------------------------------------------------------------------

pub mod request {
    use super::Serialize;

    /// Args for `getComputed`.
    #[derive(Debug, Clone, Serialize)]
    #[serde(rename_all = "camelCase")]
    pub struct GetComputed {
        pub node: String,
        pub mark_matched: bool,
        pub filter: String,
    }

    /// Args for `getApplied`.
    #[derive(Debug, Clone, Serialize)]
    #[serde(rename_all = "camelCase")]
    pub struct GetApplied {
        pub node: String,
        pub inherited: bool,
        pub matched_selectors: bool,
        pub filter: String,
    }

    /// Args for `getLayout`.
    #[derive(Debug, Clone, Serialize)]
    #[serde(rename_all = "camelCase")]
    pub struct GetLayout {
        pub node: String,
        pub auto_margins: bool,
    }
}

// ---------------------------------------------------------------------------
// Response types
// ---------------------------------------------------------------------------

pub mod response {
    use super::{Deserialize, Value};

    /// Reply for `getComputed`.
    #[derive(Debug, Clone, Default, Deserialize)]
    pub struct GetComputed {
        #[serde(default)]
        pub computed: Value,
    }

    /// Reply for `getApplied`.
    #[derive(Debug, Clone, Default, Deserialize)]
    pub struct GetApplied {
        #[serde(default)]
        pub entries: Vec<Value>,
    }

    /// Reply for `getLayout`.
    #[derive(Debug, Clone, Default, Deserialize)]
    pub struct GetLayout(pub Value);
}

// ---------------------------------------------------------------------------
// Method markers
// ---------------------------------------------------------------------------

/// `getComputed` method marker.
pub struct GetComputed;
impl sealed::Sealed for GetComputed {}
impl Method for GetComputed {
    const NAME: &'static str = "getComputed";
    type Args = request::GetComputed;
    type Reply = response::GetComputed;
}

/// `getApplied` method marker.
pub struct GetApplied;
impl sealed::Sealed for GetApplied {}
impl Method for GetApplied {
    const NAME: &'static str = "getApplied";
    type Args = request::GetApplied;
    type Reply = response::GetApplied;
}

/// `getLayout` method marker.
pub struct GetLayout;
impl sealed::Sealed for GetLayout {}
impl Method for GetLayout {
    const NAME: &'static str = "getLayout";
    type Args = request::GetLayout;
    type Reply = response::GetLayout;
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn get_computed_request_serializes_node_and_filter() {
        let args = request::GetComputed {
            node: "server1.conn0.child1/domNode1".into(),
            mark_matched: true,
            filter: "user".into(),
        };
        let v = serde_json::to_value(&args).unwrap();
        assert_eq!(v["node"], "server1.conn0.child1/domNode1");
        assert_eq!(v["markMatched"], true);
        assert_eq!(v["filter"], "user");
    }

    #[test]
    fn get_applied_request_serializes_inherited_and_matched_selectors() {
        let args = request::GetApplied {
            node: "server1.conn0.child1/domNode1".into(),
            inherited: false,
            matched_selectors: true,
            filter: "user".into(),
        };
        let v = serde_json::to_value(&args).unwrap();
        assert_eq!(v["inherited"], false);
        assert_eq!(v["matchedSelectors"], true);
    }

    #[test]
    fn get_layout_request_serializes_auto_margins() {
        let args = request::GetLayout {
            node: "server1.conn0.child1/domNode1".into(),
            auto_margins: true,
        };
        let v = serde_json::to_value(&args).unwrap();
        assert_eq!(v["autoMargins"], true);
    }

    #[test]
    fn get_computed_response_deserializes_computed_object() {
        let v = json!({
            "from": "server1.conn0.child1/pageStyleActor1",
            "computed": {"color": {"value": "rgb(0,0,0)", "priority": ""}}
        });
        let reply: response::GetComputed = serde_json::from_value(v).unwrap();
        assert!(reply.computed.get("color").is_some());
    }

    #[test]
    fn get_applied_response_deserializes_entries() {
        let v = json!({
            "from": "server1.conn0.child1/pageStyleActor1",
            "entries": [{"rule": {"selector": "body"}, "declarations": []}]
        });
        let reply: response::GetApplied = serde_json::from_value(v).unwrap();
        assert_eq!(reply.entries.len(), 1);
    }

    #[test]
    fn method_names_are_correct() {
        assert_eq!(GetComputed::NAME, "getComputed");
        assert_eq!(GetApplied::NAME, "getApplied");
        assert_eq!(GetLayout::NAME, "getLayout");
    }
}
