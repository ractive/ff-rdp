use crate::actor::actor_send;
use crate::error::ProtocolError;
use crate::transport::RdpTransport;
use crate::types::ActorId;

/// Operations on the Firefox Reflow actor.
///
/// The Reflow actor tracks layout reflow events on a target.  Both `start` and
/// `stop` are declared `oneway: true` in `devtools/shared/specs/reflow.js:30-33`
/// so Firefox never sends a reply for either.
pub struct ReflowActor;

impl ReflowActor {
    /// Begin tracking reflow events.
    ///
    /// **Oneway** — `start` is declared `oneway: true` in
    /// `devtools/shared/specs/reflow.js:30-33`. Firefox does not send a reply.
    pub fn start(
        transport: &mut RdpTransport,
        reflow_actor: &ActorId,
    ) -> Result<(), ProtocolError> {
        actor_send(transport, reflow_actor.as_ref(), "start", None)
    }

    /// Stop tracking reflow events.
    ///
    /// **Oneway** — `stop` is declared `oneway: true` in
    /// `devtools/shared/specs/reflow.js:30-33`. Firefox does not send a reply.
    pub fn stop(transport: &mut RdpTransport, reflow_actor: &ActorId) -> Result<(), ProtocolError> {
        actor_send(transport, reflow_actor.as_ref(), "stop", None)
    }
}
