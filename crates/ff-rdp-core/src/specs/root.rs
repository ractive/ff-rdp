//! Spec for the Root actor.
//!
//! Mirrors <https://searchfox.org/mozilla-central/source/devtools/shared/specs/root.js>

use serde::{Deserialize, Serialize};

use super::{Method, NoArgs, sealed};

// Re-export parsed types from the actors module so callers use the spec as the typed surface.
pub use crate::actors::root::ProcessInfo;
pub use crate::actors::tab::TabInfo;

// ---------------------------------------------------------------------------
// Request args
// ---------------------------------------------------------------------------

pub mod request {
    use super::{NoArgs, Serialize};

    /// Args for `listTabs` — no parameters needed.
    pub type ListTabs = NoArgs;

    /// Args for `getRoot` — no parameters needed.
    pub type GetRoot = NoArgs;

    /// Args for `listProcesses` — no parameters needed.
    pub type ListProcesses = NoArgs;

    /// Args for `listAddons` (`root.js:54-61`: `iconDataURL: Option(0, "boolean")`).
    ///
    /// `true` makes the server fetch every add-on icon into a `data:` URL
    /// before replying (`devtools/server/actors/root.js:372-374`); left out
    /// when `None`, which the server treats as `false`.
    #[derive(Debug, Clone, Default, Serialize)]
    pub struct ListAddons {
        #[serde(rename = "iconDataURL", skip_serializing_if = "Option::is_none")]
        pub icon_data_url: Option<bool>,
    }
}

// ---------------------------------------------------------------------------
// Response types
// ---------------------------------------------------------------------------

pub mod response {
    use super::{Deserialize, TabInfo};
    use crate::specs::webextension_descriptor::response::AddonDescriptor;
    use crate::types::ActorId;

    /// Reply for `listAddons` (`root.js:58-60`: `addons: array:webExtensionDescriptor`).
    ///
    /// Every add-on the AddonManager knows — system, hidden, themes and GMP
    /// plugins included (`devtools/server/actors/webbrowser.js:688-699`).
    #[derive(Debug, Clone, Deserialize)]
    pub struct ListAddons {
        pub addons: Vec<AddonDescriptor>,
    }

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

/// `listAddons` method marker (`root.js:54-61`).
pub struct ListAddons;
impl sealed::Sealed for ListAddons {}
impl Method for ListAddons {
    const NAME: &'static str = "listAddons";
    type Args = request::ListAddons;
    type Reply = response::ListAddons;
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
        assert_eq!(ListAddons::NAME, "listAddons");
    }

    #[test]
    fn list_addons_request_sends_icon_data_url_only_when_set() {
        let none = serde_json::to_value(request::ListAddons::default()).unwrap();
        assert_eq!(none, json!({}));
        let some = serde_json::to_value(request::ListAddons {
            icon_data_url: Some(false),
        })
        .unwrap();
        assert_eq!(some, json!({"iconDataURL": false}));
    }

    /// Recorded from Firefox by `live_298_record_addon_commands`: the fixture
    /// add-on is temporary, has a UUID and a `file:` source URL; GMP plugins
    /// come without the boolean flags.
    #[test]
    fn list_addons_reply_deserializes_recorded_reply() {
        let raw = include_str!("../../tests/fixtures/list_addons_response.json");
        let reply: response::ListAddons = serde_json::from_str(raw).unwrap();
        let fixture = reply
            .addons
            .iter()
            .find(|a| a.id == "temporary-addon-fixture@ff-rdp.test")
            .expect("fixture add-on listed");
        assert_eq!(fixture.temporarily_installed, Some(true));
        assert_eq!(fixture.debuggable, Some(true));
        assert_eq!(fixture.hidden, Some(false));
        assert!(fixture.uuid().is_some(), "{fixture:?}");
        assert!(
            fixture
                .url
                .as_deref()
                .is_some_and(|u| u.starts_with("file://")),
            "{fixture:?}"
        );
        assert!(
            reply.addons.iter().any(|a| a.hidden.is_none()),
            "a GMP plugin without flags is in the recording"
        );
    }
}
