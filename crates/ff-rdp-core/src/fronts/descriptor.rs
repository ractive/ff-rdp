use crate::registry::{Front, FrontKind, Registry};
use crate::types::ActorId;

/// A typed handle to a Firefox tab descriptor actor.
///
/// Descriptor actors expose `getTarget` and `getWatcher` — they are the
/// per-tab entry points returned by the root actor's `listTabs`.
///
/// Creating a `DescriptorFront` is O(1) and does not touch the network.
pub struct DescriptorFront {
    id: ActorId,
    registry: Registry,
}

impl DescriptorFront {
    /// Wrap an actor ID as a `DescriptorFront` and register it in the registry.
    pub fn new(id: ActorId, registry: Registry) -> Self {
        registry.register(id.clone(), FrontKind::Descriptor, None);
        Self { id, registry }
    }
}

impl Front for DescriptorFront {
    fn id(&self) -> &ActorId {
        &self.id
    }

    fn registry(&self) -> &Registry {
        &self.registry
    }
}
