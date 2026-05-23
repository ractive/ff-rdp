use crate::registry::{Front, FrontKind, Registry};
use crate::types::ActorId;

/// A typed handle to a Firefox `PageStyle` actor.
///
/// PageStyle actors provide `getApplied`, `getComputed`, `getBoxModel`, etc.
/// for CSS inspection of DOM nodes.
///
/// Creating a `PageStyleFront` is O(1) and does not touch the network.
pub struct PageStyleFront {
    id: ActorId,
    registry: Registry,
}

impl PageStyleFront {
    /// Wrap an actor ID as a `PageStyleFront` and register it in the registry.
    ///
    /// `target_root` should be the `WindowGlobalTarget` actor that owns this front.
    pub fn new(id: ActorId, registry: Registry, target_root: ActorId) -> Self {
        registry.register(id.clone(), FrontKind::PageStyle, Some(target_root));
        Self { id, registry }
    }
}

impl Front for PageStyleFront {
    fn id(&self) -> &ActorId {
        &self.id
    }

    fn registry(&self) -> &Registry {
        &self.registry
    }
}
