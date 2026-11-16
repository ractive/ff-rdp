//! Spec for the WebExtension descriptor actor (one per installed add-on).
//!
//! Mirrors <https://searchfox.org/mozilla-central/source/devtools/shared/specs/descriptors/webextension.js>
//! (lines 12-45). Descriptors are handed out by the root actor's `listAddons`
//! (`devtools/shared/specs/root.js:54-61`); their form is built by
//! `WebExtensionDescriptorActor.form()`
//! (`devtools/server/actors/descriptors/webextension.js:89-124`).
//!
//! Only `reload` is mirrored — about:debugging's Reload button
//! (`devtools/client/aboutdebugging/src/actions/debug-targets.js:161-175`).
//! `reloadDescriptor` is the same call under the generic descriptor name
//! (server lines 213-215); `terminateBackgroundScript` and `getWatcher` have no
//! ff-rdp caller.

use serde::Deserialize;
use serde_json::Value;

use super::{Method, NoArgs, sealed};
use crate::types::ActorId;

// ---------------------------------------------------------------------------
// Response types
// ---------------------------------------------------------------------------

pub mod response {
    use super::{ActorId, Deserialize, Value};

    /// A `webExtensionDescriptor` form, one per entry of `listAddons`.
    ///
    /// Built in `webextension.js:89-124`. Every field but `actor` and `id` is
    /// optional: a field whose value is `undefined` is left out of the packet,
    /// and Firefox 160 sends GMP plugins (`gmp-widevinecdm`) with no
    /// `debuggable`, `hidden`, `isSystem` or `temporarilyInstalled` at all.
    /// The form carries no version.
    #[derive(Debug, Clone, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct AddonDescriptor {
        /// The descriptor actor — valid only on the connection that listed it.
        pub actor: ActorId,
        pub id: String,
        #[serde(default)]
        pub name: Option<String>,
        /// `policy?.active && addon.isDebuggable` (line 102); about:debugging
        /// lists only add-ons where this is true.
        #[serde(default)]
        pub debuggable: Option<bool>,
        /// Hidden from about:addons and, by default, about:debugging.
        #[serde(default)]
        pub hidden: Option<bool>,
        #[serde(default)]
        pub is_system: Option<bool>,
        #[serde(default)]
        pub temporarily_installed: Option<bool>,
        /// `moz-extension://<UUID>/manifest.json` while the extension runs,
        /// `null` for a theme or plugin without an active policy (line 110).
        #[serde(default, rename = "manifestURL")]
        pub manifest_url: Option<String>,
        /// The add-on's `sourceURI` (line 119): for a temporary install the
        /// `file:` URL of the directory or `.xpi` it was installed from.
        #[serde(default)]
        pub url: Option<String>,
        /// Manifest warnings Firefox collected while loading the add-on.
        #[serde(default)]
        pub warnings: Vec<Value>,
    }

    impl AddonDescriptor {
        /// The internal UUID from [`Self::manifest_url`] — the host part of
        /// the add-on's `moz-extension://` origin.
        pub fn uuid(&self) -> Option<&str> {
            self.manifest_url
                .as_deref()?
                .strip_prefix("moz-extension://")?
                .split('/')
                .next()
                .filter(|uuid| !uuid.is_empty())
        }
    }

    /// Reply for `reload`.
    ///
    /// The spec declares `{ addon: RetVal("json") }` (line 18) but the server
    /// returns `{}` (line 219), so Firefox 160 answers `{"addon": {}}`:
    /// nothing to read.
    #[derive(Debug, Clone, Deserialize)]
    pub struct Reload {}
}

// ---------------------------------------------------------------------------
// Method markers
// ---------------------------------------------------------------------------

/// `reload` method marker (spec lines 16-19).
///
/// The server awaits `AddonWrapper.reload()` (`webextension.js:217-220`): for
/// a temporary add-on that re-runs `AddonManager.installTemporaryAddon` on the
/// add-on's own source (`XPIDatabase.sys.mjs:1505-1519`); for any other add-on
/// it disables and re-enables it.
pub struct Reload;
impl sealed::Sealed for Reload {}
impl Method for Reload {
    const NAME: &'static str = "reload";
    type Args = NoArgs;
    type Reply = response::Reload;
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> Value {
        let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    #[test]
    fn method_name_is_correct() {
        assert_eq!(Reload::NAME, "reload");
    }

    #[test]
    fn reload_request_has_no_args() {
        assert_eq!(
            serde_json::to_value(NoArgs::default()).unwrap(),
            serde_json::json!({})
        );
    }

    /// Recorded from Firefox by `live_298_record_addon_commands`.
    #[test]
    fn reload_reply_deserializes_recorded_reply() {
        let reply = fixture("webextension_reload_response.json");
        assert_eq!(reply["addon"], serde_json::json!({}), "{reply}");
        let _: response::Reload = serde_json::from_value(reply).unwrap();
    }

    #[test]
    fn uuid_comes_from_the_manifest_url() {
        let form: response::AddonDescriptor = serde_json::from_value(serde_json::json!({
            "actor": "server1.conn0.webExtensionDescriptor15",
            "id": "a@b",
            "manifestURL": "moz-extension://cb03b7bf-f042-4747-b81e-4cb2e5c866cd/manifest.json",
        }))
        .unwrap();
        assert_eq!(form.uuid(), Some("cb03b7bf-f042-4747-b81e-4cb2e5c866cd"));
        let no_policy: response::AddonDescriptor = serde_json::from_value(serde_json::json!({
            "actor": "server1.conn0.webExtensionDescriptor16",
            "id": "theme@b",
            "manifestURL": null,
        }))
        .unwrap();
        assert_eq!(no_policy.uuid(), None);
    }
}
