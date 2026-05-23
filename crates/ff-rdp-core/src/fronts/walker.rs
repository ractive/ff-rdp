use crate::registry::{Front, FrontKind, Registry};
use crate::types::ActorId;

/// A typed handle to a Firefox `DOMWalker` actor.
///
/// Walker actors traverse the DOM tree for a target and expose `getRootNode`,
/// `children`, `node`, etc.
///
/// Creating a `WalkerFront` is O(1) and does not touch the network.
pub struct WalkerFront {
    id: ActorId,
    registry: Registry,
}

impl WalkerFront {
    /// Wrap an actor ID as a `WalkerFront` and register it in the registry.
    ///
    /// `target_root` should be the `WindowGlobalTarget` actor that owns this front.
    pub fn new(id: ActorId, registry: Registry, target_root: ActorId) -> Self {
        registry.register(id.clone(), FrontKind::Walker, Some(target_root));
        Self { id, registry }
    }
}

impl Front for WalkerFront {
    fn id(&self) -> &ActorId {
        &self.id
    }

    fn registry(&self) -> &Registry {
        &self.registry
    }
}
