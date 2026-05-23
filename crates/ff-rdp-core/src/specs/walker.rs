//! Spec for the DOMWalker actor.
//!
//! Mirrors <https://searchfox.org/mozilla-central/source/devtools/shared/specs/walker.js>

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{Method, NoArgs, sealed};

// Re-export DOM types so callers use this spec module as their typed surface.
pub use crate::actors::dom_walker::{DomAttr, DomNode};

// ---------------------------------------------------------------------------
// Request args
// ---------------------------------------------------------------------------

pub mod request {
    use super::{NoArgs, Serialize};

    /// Args for `documentElement` — no parameters.
    pub type DocumentElement = NoArgs;

    /// Args for `querySelector`.
    #[derive(Debug, Clone, Serialize)]
    pub struct QuerySelector {
        /// The actor ID of the root node to search within.
        pub node: String,
        /// The CSS selector to match.
        pub selector: String,
    }

    /// Args for `querySelectorAll`.
    #[derive(Debug, Clone, Serialize)]
    pub struct QuerySelectorAll {
        /// The actor ID of the root node to search within.
        pub node: String,
        /// The CSS selector to match.
        pub selector: String,
    }
}

// ---------------------------------------------------------------------------
// Response types
// ---------------------------------------------------------------------------

pub mod response {
    use super::{Deserialize, Value};

    /// Reply for `documentElement`.
    #[derive(Debug, Clone, Default, Deserialize)]
    pub struct DocumentElement {
        #[serde(default)]
        pub node: Option<Value>,
    }

    /// Reply for `querySelector`.
    #[derive(Debug, Clone, Default, Deserialize)]
    pub struct QuerySelector {
        #[serde(default)]
        pub node: Option<Value>,
    }

    /// Reply for `querySelectorAll`.
    #[derive(Debug, Clone, Default, Deserialize)]
    pub struct QuerySelectorAll {
        #[serde(default)]
        pub list: Option<Value>,
    }
}

// ---------------------------------------------------------------------------
// Method markers
// ---------------------------------------------------------------------------

/// `documentElement` method marker.
pub struct DocumentElement;
impl sealed::Sealed for DocumentElement {}
impl Method for DocumentElement {
    const NAME: &'static str = "documentElement";
    type Args = NoArgs;
    type Reply = response::DocumentElement;
}

/// `querySelector` method marker.
pub struct QuerySelector;
impl sealed::Sealed for QuerySelector {}
impl Method for QuerySelector {
    const NAME: &'static str = "querySelector";
    type Args = request::QuerySelector;
    type Reply = response::QuerySelector;
}

/// `querySelectorAll` method marker.
pub struct QuerySelectorAll;
impl sealed::Sealed for QuerySelectorAll {}
impl Method for QuerySelectorAll {
    const NAME: &'static str = "querySelectorAll";
    type Args = request::QuerySelectorAll;
    type Reply = response::QuerySelectorAll;
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn query_selector_request_serializes_node_and_selector() {
        let args = request::QuerySelector {
            node: "server1.conn0.child1/domNode1".into(),
            selector: "h1".into(),
        };
        let v = serde_json::to_value(&args).unwrap();
        assert_eq!(v["node"], "server1.conn0.child1/domNode1");
        assert_eq!(v["selector"], "h1");
    }

    #[test]
    fn query_selector_all_request_serializes() {
        let args = request::QuerySelectorAll {
            node: "server1.conn0.child1/domNode1".into(),
            selector: "p".into(),
        };
        let v = serde_json::to_value(&args).unwrap();
        assert_eq!(v["selector"], "p");
    }

    #[test]
    fn document_element_response_deserializes_node() {
        let v = json!({
            "from": "server1.conn0.child1/domWalker1",
            "node": {"actor": "server1.conn0.child1/domNode1", "nodeType": 1, "nodeName": "HTML"}
        });
        let reply: response::DocumentElement = serde_json::from_value(v).unwrap();
        assert!(reply.node.is_some());
    }

    #[test]
    fn query_selector_response_deserializes_absent_node() {
        // querySelector returns {} when no match found.
        let v = json!({"from": "server1.conn0.child1/domWalker1"});
        let reply: response::QuerySelector = serde_json::from_value(v).unwrap();
        assert!(reply.node.is_none());
    }

    #[test]
    fn method_names_are_correct() {
        assert_eq!(DocumentElement::NAME, "documentElement");
        assert_eq!(QuerySelector::NAME, "querySelector");
        assert_eq!(QuerySelectorAll::NAME, "querySelectorAll");
    }
}
