use crate::registry::{Front, FrontKind, Registry};
use crate::types::ActorId;

/// A typed handle to a Firefox `Screenshot` or `ScreenshotContent` actor.
///
/// Screenshot actors provide `capture` operations for taking page screenshots.
///
/// Creating a `ScreenshotFront` is O(1) and does not touch the network.
pub struct ScreenshotFront {
    id: ActorId,
    registry: Registry,
}

impl ScreenshotFront {
    /// Wrap an actor ID as a `ScreenshotFront` and register it in the registry.
    ///
    /// `target_root` should be the `WindowGlobalTarget` actor that owns this front.
    pub fn new(id: ActorId, registry: Registry, target_root: ActorId) -> Self {
        registry.register(id.clone(), FrontKind::Screenshot, Some(target_root));
        Self { id, registry }
    }
}

impl Front for ScreenshotFront {
    fn id(&self) -> &ActorId {
        &self.id
    }

    fn registry(&self) -> &Registry {
        &self.registry
    }
}
