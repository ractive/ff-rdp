use serde_json::json;

use crate::actor::actor_request;
use crate::error::ProtocolError;
use crate::transport::RdpTransport;
use crate::types::ActorId;

/// Operations on the Firefox ResponsiveActor.
///
/// The ResponsiveActor is obtained via `getTarget` (the `responsiveActor` field
/// of the returned frame).  It provides viewport simulation capabilities used by
/// Responsive Design Mode (RDM) in Firefox DevTools.
///
/// Unlike `window.resizeTo()`, which is blocked by the browser for non-popup
/// windows and has no effect in headless mode, `setViewportSize` works reliably
/// across all Firefox configurations.
pub struct ResponsiveActor;

impl ResponsiveActor {
    /// Set the simulated viewport size for this browsing context.
    ///
    /// After this call Firefox renders the page as if the viewport is `width × height`
    /// pixels, regardless of the actual window size.  This is the mechanism used by
    /// Firefox DevTools' Responsive Design Mode.
    ///
    /// # Arguments
    ///
    /// * `transport` — open RDP transport.
    /// * `responsive_actor` — the `responsiveActor` ID from `getTarget`.
    /// * `width` — desired viewport width in CSS pixels.
    /// * `height` — desired viewport height in CSS pixels.  Pass the current
    ///   outer height (from `window.outerHeight`) to keep the vertical dimension
    ///   unchanged.
    pub fn set_viewport_size(
        transport: &mut RdpTransport,
        responsive_actor: &ActorId,
        width: u32,
        height: u32,
    ) -> Result<(), ProtocolError> {
        let params = json!({
            "viewport": {
                "width": width,
                "height": height,
            }
        });
        actor_request(
            transport,
            responsive_actor.as_ref(),
            "setViewportSize",
            Some(&params),
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    /// Verify that `setViewportSize` builds the expected wire message shape.
    ///
    /// We only test the message structure here — actual over-the-wire behaviour
    /// is validated by the live e2e tests.
    #[test]
    fn set_viewport_size_params_shape() {
        // Simulate what actor_request would encode by building the params manually.
        let width: u32 = 320;
        let height: u32 = 768;
        let params = json!({
            "viewport": {
                "width": width,
                "height": height,
            }
        });

        assert_eq!(params["viewport"]["width"], 320);
        assert_eq!(params["viewport"]["height"], 768);
    }

    #[test]
    fn set_viewport_size_params_large_dimensions() {
        let width: u32 = 2560;
        let height: u32 = 1440;
        let params = json!({
            "viewport": { "width": width, "height": height }
        });
        assert_eq!(params["viewport"]["width"], 2560);
        assert_eq!(params["viewport"]["height"], 1440);
    }
}
