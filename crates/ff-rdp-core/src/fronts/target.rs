use crate::registry::{Front, FrontKind, Registry};
use crate::types::ActorId;

/// A typed handle to a Firefox `WindowGlobalTarget` actor.
///
/// Target actors are scoped to a browsing context (frame).  They expose
/// `navigate`, `reload`, and are the root under which console, inspector,
/// and other per-target actors live.
///
/// Creating a `TargetFront` is O(1) and does not touch the network.
pub struct TargetFront {
    id: ActorId,
    registry: Registry,
}

impl TargetFront {
    /// Wrap an actor ID as a `TargetFront` and register it in the registry.
    ///
    /// The target itself has no owning root (it *is* the root for its subtree).
    pub fn new(id: ActorId, registry: Registry) -> Self {
        registry.register(id.clone(), FrontKind::Target, None);
        Self { id, registry }
    }
}

impl Front for TargetFront {
    fn id(&self) -> &ActorId {
        &self.id
    }

    fn registry(&self) -> &Registry {
        &self.registry
    }
}
