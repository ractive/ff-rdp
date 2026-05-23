//! Spec for the Root actor.
//!
//! Mirrors <https://searchfox.org/mozilla-central/source/devtools/shared/specs/root.js>

use serde::Deserialize;

use super::{Method, NoArgs, sealed};

// Re-export parsed types from the actors module so callers use the spec as the typed surface.
pub use crate::actors::root::ProcessInfo;
pub use crate::actors::tab::TabInfo;

// ---------------------------------------------------------------------------
// Request args
// ---------------------------------------------------------------------------

pub mod request {
    use super::NoArgs;

    /// Args for `listTabs` — no parameters needed.
    pub type ListTabs = NoArgs;

    /// Args for `getRoot` — no parameters needed.
    pub type GetRoot = NoArgs;

    /// Args for `listProcesses` — no parameters needed.
    pub type ListProcesses = NoArgs;
}

// ---------------------------------------------------------------------------
// Response types
// ---------------------------------------------------------------------------

pub mod response {
    use super::{Deserialize, TabInfo};
    use crate::types::ActorId;

    /// Reply for `listTabs`.
    #[derive(Debug, Clone, Default, Deserialize)]
    pub struct ListTabs {
        #[serde(default)]
        pub tabs: Vec<TabInfo>,
    }

    /// Reply for `getRoot` — the root actor exposes several service actor IDs.
    #[derive(Debug, Clone, Default, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct GetRoot {
        #[serde(default)]
        pub screenshot_actor: Option<ActorId>,
        #[serde(default)]
        pub preference_actor: Option<ActorId>,
        #[serde(default)]
        pub device_actor: Option<ActorId>,
    }

    /// A process descriptor entry from `listProcesses`.
    #[derive(Debug, Clone, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct ProcessEntry {
        pub actor: ActorId,
        #[serde(default)]
        pub is_parent: bool,
    }

    /// Reply for `listProcesses`.
    #[derive(Debug, Clone, Default, Deserialize)]
    pub struct ListProcesses {
        #[serde(default)]
        pub processes: Vec<ProcessEntry>,
    }
}

// ---------------------------------------------------------------------------
// Method markers
// ---------------------------------------------------------------------------

/// `listTabs` method marker.
pub struct ListTabs;
impl sealed::Sealed for ListTabs {}
impl Method for ListTabs {
    const NAME: &'static str = "listTabs";
    type Args = NoArgs;
    type Reply = response::ListTabs;
}

/// `getRoot` method marker.
pub struct GetRoot;
impl sealed::Sealed for GetRoot {}
impl Method for GetRoot {
    const NAME: &'static str = "getRoot";
    type Args = NoArgs;
    type Reply = response::GetRoot;
}

/// `listProcesses` method marker.
pub struct ListProcesses;
impl sealed::Sealed for ListProcesses {}
impl Method for ListProcesses {
    const NAME: &'static str = "listProcesses";
    type Args = NoArgs;
    type Reply = response::ListProcesses;
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn list_tabs_response_deserializes_tabs_array() {
        let v = json!({
            "from": "root",
            "tabs": [
                {"actor": "server1.conn0.tabDescriptor1", "title": "Example", "url": "https://example.com", "selected": true, "browsingContextID": 42}
            ]
        });
        let reply: response::ListTabs = serde_json::from_value(v).unwrap();
        assert_eq!(reply.tabs.len(), 1);
        assert_eq!(reply.tabs[0].title, "Example");
        assert_eq!(reply.tabs[0].url, "https://example.com");
        assert_eq!(reply.tabs[0].browsing_context_id, Some(42));
    }

    #[test]
    fn list_tabs_response_empty_tabs() {
        let v = json!({"from": "root", "tabs": []});
        let reply: response::ListTabs = serde_json::from_value(v).unwrap();
        assert!(reply.tabs.is_empty());
    }

    #[test]
    fn get_root_response_deserializes_service_actors() {
        let v = json!({
            "from": "root",
            "screenshotActor": "server1.conn0.screenshotActor7",
            "preferenceActor": "server1.conn0.preferenceActor1"
        });
        let reply: response::GetRoot = serde_json::from_value(v).unwrap();
        assert_eq!(
            reply
                .screenshot_actor
                .as_ref()
                .map(std::convert::AsRef::as_ref),
            Some("server1.conn0.screenshotActor7")
        );
        assert_eq!(
            reply
                .preference_actor
                .as_ref()
                .map(std::convert::AsRef::as_ref),
            Some("server1.conn0.preferenceActor1")
        );
    }

    #[test]
    fn list_processes_response_deserializes_processes() {
        let v = json!({
            "from": "root",
            "processes": [
                {"actor": "server1.conn0.processDescriptor1", "isParent": true},
                {"actor": "server1.conn0.processDescriptor2", "isParent": false}
            ]
        });
        let reply: response::ListProcesses = serde_json::from_value(v).unwrap();
        assert_eq!(reply.processes.len(), 2);
        assert_eq!(
            reply.processes[0].actor.as_ref(),
            "server1.conn0.processDescriptor1"
        );
        assert!(reply.processes[0].is_parent);
        assert!(!reply.processes[1].is_parent);
    }

    #[test]
    fn method_names_are_correct() {
        assert_eq!(ListTabs::NAME, "listTabs");
        assert_eq!(GetRoot::NAME, "getRoot");
        assert_eq!(ListProcesses::NAME, "listProcesses");
    }
}
