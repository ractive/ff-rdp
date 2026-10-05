//! The Firefox `TargetConfigurationActor`: per-target page-environment
//! settings (colour-scheme and print-media simulation, custom user agent,
//! HTTP-cache bypass).
//!
//! The configuration is tied to the watcher that owns the actor, so it lives
//! exactly as long as the RDP connection that set it: when the connection
//! closes, Firefox destroys the actor and restores every setting it changed
//! (`_restoreParentProcessConfiguration` in
//! `devtools/server/actors/target-configuration.js`). ff-rdp opens one
//! connection per command, so a setting applied here covers only the command
//! that applied it.
//!
//! Only tab (`browser-element`) watchers apply these settings to the page —
//! they are written to the top-level `BrowsingContext` in the parent process
//! (`prefersColorSchemeOverride`, `mediumOverride`, `customUserAgent`,
//! `defaultLoadFlags`).
//!
//! See `kb/rdp/actors/target-configuration.md`.

use serde_json::Value;

use crate::error::ProtocolError;
use crate::specs::target_configuration::{self as spec, request};
use crate::specs::{NoArgs, call, watcher as watcher_spec};
use crate::transport::RdpTransport;
use crate::types::ActorId;

/// The configuration patch [`TargetConfigurationActor::update_configuration`]
/// sends; unset fields are left untouched.
pub type TargetConfiguration = request::Configuration;

/// Stateless operations on a Firefox `TargetConfigurationActor`.
pub struct TargetConfigurationActor;

impl TargetConfigurationActor {
    /// Ask `watcher` for its target-configuration actor
    /// (`getTargetConfigurationActor`; the ID is nested under
    /// `configuration.actor`).
    pub fn for_watcher(
        transport: &mut RdpTransport,
        watcher: &ActorId,
    ) -> Result<ActorId, ProtocolError> {
        let reply =
            call::<watcher_spec::GetTargetConfigurationActor>(transport, watcher, &NoArgs {})?;
        Ok(reply.configuration.actor)
    }

    /// Merge `configuration` into the target's live configuration.
    ///
    /// Firefox silently drops keys it does not support, so every key sent must
    /// come back in the echoed configuration with the value sent; otherwise
    /// this returns [`ProtocolError::InvalidPacket`] naming the key, rather
    /// than letting a caller act as if the setting were in force.
    pub fn update_configuration(
        transport: &mut RdpTransport,
        actor: &ActorId,
        configuration: &TargetConfiguration,
    ) -> Result<(), ProtocolError> {
        let args = request::UpdateConfiguration {
            configuration: configuration.clone(),
        };
        let reply = call::<spec::UpdateConfiguration>(transport, actor, &args)?;
        check_echo(&args.configuration, &reply.configuration)
    }
}

/// Verify every key of `sent` appears in `echo` with the same value.
fn check_echo(sent: &TargetConfiguration, echo: &Value) -> Result<(), ProtocolError> {
    let sent = serde_json::to_value(sent)
        .map_err(|e| ProtocolError::InvalidPacket(format!("encode configuration: {e}")))?;
    let Some(sent) = sent.as_object() else {
        return Ok(());
    };
    for (key, value) in sent {
        if echo.get(key) != Some(value) {
            return Err(ProtocolError::InvalidPacket(format!(
                "updateConfiguration: Firefox did not apply `{key}` (sent {value}, \
                 echoed {})",
                echo.get(key).unwrap_or(&Value::Null)
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn dark() -> TargetConfiguration {
        TargetConfiguration {
            color_scheme_simulation: Some("dark".into()),
            ..Default::default()
        }
    }

    #[test]
    fn check_echo_accepts_matching_echo() {
        let echo = json!({"colorSchemeSimulation": "dark", "cacheDisabled": false});
        check_echo(&dark(), &echo).unwrap();
    }

    #[test]
    fn check_echo_rejects_dropped_key() {
        let err = check_echo(&dark(), &json!({})).unwrap_err();
        assert!(
            err.to_string().contains("colorSchemeSimulation"),
            "error must name the dropped key: {err}"
        );
    }

    #[test]
    fn check_echo_rejects_dropped_cache_disabled() {
        let cold = TargetConfiguration {
            cache_disabled: Some(true),
            ..Default::default()
        };
        check_echo(&cold, &json!({"cacheDisabled": true})).unwrap();
        let err = check_echo(&cold, &json!({"colorSchemeSimulation": "dark"})).unwrap_err();
        assert!(err.to_string().contains("cacheDisabled"), "{err}");
    }

    #[test]
    fn check_echo_rejects_different_value() {
        let echo = json!({"colorSchemeSimulation": "light"});
        assert!(check_echo(&dark(), &echo).is_err());
    }
}
