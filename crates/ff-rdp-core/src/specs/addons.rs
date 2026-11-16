//! Spec for the Addons actor (temporary WebExtension installs).
//!
//! Mirrors <https://searchfox.org/mozilla-central/source/devtools/shared/specs/addon/addons.js>
//! (lines 12-31). The actor ID is the `addonsActor` field of the root actor's
//! `getRoot` reply (`devtools/server/actors/utils/actor-registry.js:129-133`
//! registers it as a global actor with prefix `addons`).
//!
//! `uninstallAddon` (spec lines 24-29, Firefox 113+) is not mirrored: nothing in
//! ff-rdp calls it yet — a temporary add-on goes away when Firefox exits.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{Method, sealed};

// ---------------------------------------------------------------------------
// Request args
// ---------------------------------------------------------------------------

pub mod request {
    use super::Serialize;

    /// Args for `installTemporaryAddon` (spec lines 16-22).
    ///
    /// `addonPath` is handed to `new FileUtils.File(addonPath)`
    /// (`devtools/server/actors/addon/addons.js:32`), so it must be an
    /// absolute native path to an extension directory or `.xpi` file.
    /// `openDevTools` (Firefox 106+, `nullable:boolean`) opens a toolbox for
    /// the add-on when `true`; it is always serialised so the request matches
    /// the spec's two positional args.
    #[derive(Debug, Clone, Serialize)]
    #[serde(rename_all = "camelCase")]
    pub struct InstallTemporaryAddon {
        pub addon_path: String,
        pub open_dev_tools: Option<bool>,
    }
}

// ---------------------------------------------------------------------------
// Response types
// ---------------------------------------------------------------------------

pub mod response {
    use super::{Deserialize, Value};

    /// The pseudo add-on object in an `installTemporaryAddon` reply.
    ///
    /// `RetVal("json")` in the spec; the server builds it as
    /// `{ id: addon.id, actor: false }` (`addons.js:75`) — `actor` has been the
    /// literal `false` since the actor existed, kept as a raw value so a future
    /// real actor ID is not a decode error.
    #[derive(Debug, Clone, Deserialize)]
    pub struct InstalledAddon {
        pub id: String,
        #[serde(default)]
        pub actor: Value,
    }

    /// Reply for `installTemporaryAddon` (spec line 21: `{ addon: RetVal("json") }`).
    #[derive(Debug, Clone, Deserialize)]
    pub struct InstallTemporaryAddon {
        pub addon: InstalledAddon,
    }
}

// ---------------------------------------------------------------------------
// Method markers
// ---------------------------------------------------------------------------

/// `installTemporaryAddon` method marker.
pub struct InstallTemporaryAddon;
impl sealed::Sealed for InstallTemporaryAddon {}
impl Method for InstallTemporaryAddon {
    const NAME: &'static str = "installTemporaryAddon";
    type Args = request::InstallTemporaryAddon;
    type Reply = response::InstallTemporaryAddon;
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn method_name_is_correct() {
        assert_eq!(InstallTemporaryAddon::NAME, "installTemporaryAddon");
    }

    #[test]
    fn install_request_serializes_both_spec_args() {
        let args = request::InstallTemporaryAddon {
            addon_path: "/tmp/ext".into(),
            open_dev_tools: Some(false),
        };
        let v = serde_json::to_value(&args).unwrap();
        assert_eq!(
            v,
            serde_json::json!({"addonPath": "/tmp/ext", "openDevTools": false})
        );
    }

    #[test]
    fn install_request_serializes_null_open_dev_tools() {
        let args = request::InstallTemporaryAddon {
            addon_path: "C:\\ext".into(),
            open_dev_tools: None,
        };
        let v = serde_json::to_value(&args).unwrap();
        assert_eq!(v["addonPath"], "C:\\ext");
        assert!(v["openDevTools"].is_null());
    }

    /// Recorded from Firefox by `live_297_record_install_temporary_addon`.
    #[test]
    fn install_reply_deserializes_recorded_success() {
        let raw = include_str!("../../tests/fixtures/install_temporary_addon_response.json");
        let v: Value = serde_json::from_str(raw).unwrap();
        let reply: response::InstallTemporaryAddon = serde_json::from_value(v).unwrap();
        assert_eq!(reply.addon.id, "temporary-addon-fixture@ff-rdp.test");
        assert_eq!(reply.addon.actor, Value::Bool(false));
    }
}
