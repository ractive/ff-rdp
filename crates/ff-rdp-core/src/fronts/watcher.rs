use crate::registry::{Front, FrontKind, Registry};
use crate::types::ActorId;

/// A typed handle to a Firefox Watcher actor.
///
/// Watcher actors manage resource subscriptions (`watchResources`, `watchTargets`)
/// and deliver push events for network, console, and target lifecycle events.
///
/// Creating a `WatcherFront` is O(1) and does not touch the network.
pub struct WatcherFront {
    id: ActorId,
    registry: Registry,
    /// The owning target root (if this watcher is scoped to a tab).
    target_root: Option<ActorId>,
}

impl WatcherFront {
    /// Wrap an actor ID as a `WatcherFront` and register it in the registry.
    pub fn new(id: ActorId, registry: Registry, target_root: Option<ActorId>) -> Self {
        registry.register(id.clone(), FrontKind::Watcher, target_root.clone());
        Self {
            id,
            registry,
            target_root,
        }
    }

    /// The owning target root, if any.
    pub fn target_root(&self) -> Option<&ActorId> {
        self.target_root.as_ref()
    }
}

impl Front for WatcherFront {
    fn id(&self) -> &ActorId {
        &self.id
    }

    fn registry(&self) -> &Registry {
        &self.registry
    }
}
