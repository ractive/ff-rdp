use crate::registry::{Front, FrontKind, Registry};
use crate::types::ActorId;

/// A typed handle to a Firefox `NetworkEvent` (network content) actor.
///
/// NetworkContent actors provide `getRequestHeaders`, `getResponseContent`,
/// etc. for individual network requests captured by the watcher.
///
/// Creating a `NetworkContentFront` is O(1) and does not touch the network.
pub struct NetworkContentFront {
    id: ActorId,
    registry: Registry,
}

impl NetworkContentFront {
    /// Wrap an actor ID as a `NetworkContentFront` and register it in the registry.
    ///
    /// `target_root` should be the `WindowGlobalTarget` actor that owns this front.
    pub fn new(id: ActorId, registry: Registry, target_root: ActorId) -> Self {
        registry.register(id.clone(), FrontKind::NetworkContent, Some(target_root));
        Self { id, registry }
    }
}

impl Front for NetworkContentFront {
    fn id(&self) -> &ActorId {
        &self.id
    }

    fn registry(&self) -> &Registry {
        &self.registry
    }
}
