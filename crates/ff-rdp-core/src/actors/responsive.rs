use serde_json::json;

use crate::actor::actor_request;
use crate::error::ProtocolError;
use crate::transport::RdpTransport;
use crate::types::ActorId;

/// Operations on the Firefox ResponsiveActor.
///
/// The ResponsiveActor is obtained via `getTarget` (the `responsiveActor` field
/// of the returned frame).  It provides touch simulation and picker-state APIs
/// used by Firefox DevTools' Responsive Design Mode (RDM).
///
/// # Firefox version compatibility
///
/// In Firefox 149+ the actor only exposes:
/// - `toggleTouchSimulator`
/// - `setElementPickerState`
/// - `dispatchOrientationChangeEvent`
///
/// A `setViewportSize` method was **never** part of the RDP protocol for this
/// actor — viewport sizing in RDM is performed by the browser chrome layer
/// through `synchronouslyUpdateRemoteBrowserDimensions`, which is inaccessible
/// from the RDP protocol's content-process execution context.  The `responsive`
/// CLI command uses a CSS-based simulation approach instead.
pub struct ResponsiveActor;

impl ResponsiveActor {
    /// Toggle touch simulation on this browsing context.
    ///
    /// # Arguments
    ///
    /// * `transport` — open RDP transport.
    /// * `responsive_actor` — the `responsiveActor` ID from `getTarget`.
    /// * `enabled` — `true` to enable touch simulation, `false` to disable.
    pub fn toggle_touch_simulator(
        transport: &mut RdpTransport,
        responsive_actor: &ActorId,
        enabled: bool,
    ) -> Result<bool, ProtocolError> {
        let params = json!({ "options": { "enabled": enabled } });
        let response = actor_request(
            transport,
            responsive_actor.as_ref(),
            "toggleTouchSimulator",
            Some(&params),
        )?;
        let value_changed = response
            .get("valueChanged")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false);
        Ok(value_changed)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    #[test]
    fn toggle_touch_simulator_params_shape() {
        let enabled = true;
        let params = json!({ "options": { "enabled": enabled } });
        assert_eq!(params["options"]["enabled"], true);
    }

    #[test]
    fn toggle_touch_simulator_params_disabled() {
        let params = json!({ "options": { "enabled": false } });
        assert_eq!(params["options"]["enabled"], false);
    }
}
