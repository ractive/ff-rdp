//! Spec for the TargetConfiguration actor (per-target page-environment
//! settings: colour-scheme and print-media simulation, custom user agent, …).
//!
//! Mirrors <https://searchfox.org/mozilla-central/source/devtools/shared/specs/target-configuration.js>
//!
//! The actor is obtained from the watcher's `getTargetConfigurationActor`
//! ([`super::watcher::GetTargetConfigurationActor`]). Every field of the
//! `target-configuration.configuration` dict is `nullable:*`, so a request
//! names only the keys it changes; this module models the subset ff-rdp sends.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{Method, sealed};

// ---------------------------------------------------------------------------
// Request args
// ---------------------------------------------------------------------------

pub mod request {
    use super::Serialize;

    /// The `target-configuration.configuration` patch ff-rdp sends. A `None`
    /// field is omitted from the wire, leaving that setting untouched.
    #[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
    #[serde(rename_all = "camelCase")]
    pub struct Configuration {
        /// `"light"`, `"dark"`, or `"none"` (no simulation).
        #[serde(skip_serializing_if = "Option::is_none")]
        pub color_scheme_simulation: Option<String>,
        /// `true` sets the browsing context's medium override to `print`.
        #[serde(skip_serializing_if = "Option::is_none")]
        pub print_simulation_enabled: Option<bool>,
        /// The UA string for `navigator.userAgent` and the HTTP `User-Agent`
        /// header; `""` restores the original.
        #[serde(skip_serializing_if = "Option::is_none")]
        pub custom_user_agent: Option<String>,
    }

    /// Args for `updateConfiguration`.
    #[derive(Debug, Clone, Serialize)]
    pub struct UpdateConfiguration {
        pub configuration: Configuration,
    }
}

// ---------------------------------------------------------------------------
// Response types
// ---------------------------------------------------------------------------

pub mod response {
    use super::{Deserialize, Value};

    /// Reply for `updateConfiguration`: the target's full configuration after
    /// the patch. Kept raw — callers check the keys they sent.
    #[derive(Debug, Clone, Default, Deserialize)]
    pub struct UpdateConfiguration {
        #[serde(default)]
        pub configuration: Value,
    }
}

// ---------------------------------------------------------------------------
// Method markers
// ---------------------------------------------------------------------------

/// `updateConfiguration` method marker.
pub struct UpdateConfiguration;
impl sealed::Sealed for UpdateConfiguration {}
impl Method for UpdateConfiguration {
    const NAME: &'static str = "updateConfiguration";
    type Args = request::UpdateConfiguration;
    type Reply = response::UpdateConfiguration;
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn configuration_omits_unset_fields() {
        let args = request::UpdateConfiguration {
            configuration: request::Configuration {
                color_scheme_simulation: Some("dark".into()),
                ..Default::default()
            },
        };
        assert_eq!(
            serde_json::to_value(&args).unwrap(),
            json!({"configuration": {"colorSchemeSimulation": "dark"}})
        );
    }

    #[test]
    fn configuration_uses_spec_wire_names() {
        let args = request::Configuration {
            color_scheme_simulation: Some("light".into()),
            print_simulation_enabled: Some(true),
            custom_user_agent: Some("UA".into()),
        };
        assert_eq!(
            serde_json::to_value(&args).unwrap(),
            json!({
                "colorSchemeSimulation": "light",
                "printSimulationEnabled": true,
                "customUserAgent": "UA",
            })
        );
    }

    #[test]
    fn update_configuration_reply_reads_configuration() {
        let reply: response::UpdateConfiguration = serde_json::from_value(json!({
            "from": "server1.conn0.target-configuration5",
            "configuration": {"printSimulationEnabled": true}
        }))
        .unwrap();
        assert_eq!(reply.configuration["printSimulationEnabled"], true);
        assert_eq!(UpdateConfiguration::NAME, "updateConfiguration");
    }
}
