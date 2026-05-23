use crate::registry::{Front, FrontKind, Registry};
use crate::types::ActorId;

/// A typed handle to a Firefox `WebConsole` actor.
///
/// Console actors are scoped to a target and expose `evaluateJSAsync`,
/// `startListeners`, `getCachedMessages`, etc.
///
/// Creating a `ConsoleFront` is O(1) and does not touch the network.
pub struct ConsoleFront {
    id: ActorId,
    registry: Registry,
}

impl ConsoleFront {
    /// Wrap an actor ID as a `ConsoleFront` and register it in the registry.
    ///
    /// `target_root` should be the `WindowGlobalTarget` actor that owns this console.
    pub fn new(id: ActorId, registry: Registry, target_root: ActorId) -> Self {
        registry.register(id.clone(), FrontKind::Console, Some(target_root));
        Self { id, registry }
    }
}

impl Front for ConsoleFront {
    fn id(&self) -> &ActorId {
        &self.id
    }

    fn registry(&self) -> &Registry {
        &self.registry
    }
}
