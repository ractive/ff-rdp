use crate::registry::{Front, FrontKind, Registry};
use crate::types::ActorId;

/// A typed handle to the Firefox RDP root actor.
///
/// The root actor is the entry point for all RDP sessions — it exposes
/// `listTabs`, `listProcesses`, and other top-level discovery methods.
///
/// Creating a `RootFront` is O(1) and does not touch the network.
pub struct RootFront {
    id: ActorId,
    registry: Registry,
}

impl RootFront {
    /// Wrap an actor ID as a `RootFront` and register it in the registry.
    pub fn new(id: ActorId, registry: Registry) -> Self {
        registry.register(id.clone(), FrontKind::Root, None);
        Self { id, registry }
    }
}

impl Front for RootFront {
    fn id(&self) -> &ActorId {
        &self.id
    }

    fn registry(&self) -> &Registry {
        &self.registry
    }
}
