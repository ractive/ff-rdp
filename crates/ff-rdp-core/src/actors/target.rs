use serde_json::json;

use crate::actor::actor_request;
use crate::error::ProtocolError;
use crate::transport::RdpTransport;
use crate::types::ActorId;

/// Operations on a WindowGlobalTarget actor (navigation, reload, etc.).
pub struct WindowGlobalTarget;

impl WindowGlobalTarget {
    /// Navigate to the given URL.
    pub fn navigate_to(
        transport: &mut RdpTransport,
        target_actor: &ActorId,
        url: &str,
    ) -> Result<(), ProtocolError> {
        let params = json!({"url": url});
        actor_request(
            transport,
            target_actor.as_ref(),
            "navigateTo",
            Some(&params),
        )?;
        Ok(())
    }

    /// Reload the current page.
    pub fn reload(
        transport: &mut RdpTransport,
        target_actor: &ActorId,
    ) -> Result<(), ProtocolError> {
        actor_request(transport, target_actor.as_ref(), "reload", None)?;
        Ok(())
    }

    /// Go back in browser history.
    pub fn go_back(
        transport: &mut RdpTransport,
        target_actor: &ActorId,
    ) -> Result<(), ProtocolError> {
        actor_request(transport, target_actor.as_ref(), "goBack", None)?;
        Ok(())
    }

    /// Go forward in browser history.
    pub fn go_forward(
        transport: &mut RdpTransport,
        target_actor: &ActorId,
    ) -> Result<(), ProtocolError> {
        actor_request(transport, target_actor.as_ref(), "goForward", None)?;
        Ok(())
    }
}
