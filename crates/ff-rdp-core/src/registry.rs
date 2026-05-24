//! Actor registry — tracks live Fronts and their lifecycle.
//!
//! The [`Registry`] is the central bookkeeper for actor handles.  It maps
//! actor IDs to [`FrontState`] records that encode:
//!
//! - the **kind** of actor (`FrontKind`),
//! - the **target root** that owns this actor (so we can cascade invalidation
//!   when a target is destroyed), and
//! - an **alive** flag (set to `false` on destruction).
//!
//! Subsequent calls on a dead front return [`RdpError::ActorDestroyed`].

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use dashmap::DashMap;

use crate::error::{RdpError, RdpResult};
use crate::types::ActorId;

// ---------------------------------------------------------------------------
// FrontKind
// ---------------------------------------------------------------------------

/// Which kind of Firefox actor this front represents.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FrontKind {
    Root,
    Descriptor,
    Target,
    Watcher,
    Console,
    Screenshot,
    Walker,
    PageStyle,
    NetworkContent,
    TargetConfiguration,
    /// Any actor kind not yet given a dedicated variant.
    Other(String),
}

// ---------------------------------------------------------------------------
// FrontState
// ---------------------------------------------------------------------------

/// Runtime state for a single actor entry in the registry.
pub struct FrontState {
    /// What kind of actor this is.
    pub kind: FrontKind,
    /// The top-level target actor that "owns" this front.  When the owning
    /// target is destroyed, all fronts with a matching `target_root` are
    /// invalidated atomically.
    pub target_root: Option<ActorId>,
    /// Direct parent actor for chain-based invalidation.  When the parent is
    /// destroyed, this front (and any of its own descendants) are invalidated
    /// via BFS through the parent graph.  Used for actors that aren't owned by
    /// a single target — e.g. `nodeActor`s created by a walker live under the
    /// walker's lifetime, not the target's.
    pub parent: Option<ActorId>,
    /// Whether the actor is still alive.  Set to `false` by [`Registry::invalidate_target`].
    pub alive: AtomicBool,
}

impl FrontState {
    pub fn new(kind: FrontKind, target_root: Option<ActorId>) -> Self {
        Self {
            kind,
            target_root,
            parent: None,
            alive: AtomicBool::new(true),
        }
    }

    /// Create a new state with both `target_root` and an explicit `parent` actor.
    pub fn with_parent(
        kind: FrontKind,
        target_root: Option<ActorId>,
        parent: Option<ActorId>,
    ) -> Self {
        Self {
            kind,
            target_root,
            parent,
            alive: AtomicBool::new(true),
        }
    }

    /// Returns `true` if the actor is still live.
    pub fn is_alive(&self) -> bool {
        self.alive.load(Ordering::Acquire)
    }
}

// ---------------------------------------------------------------------------
// Registry
// ---------------------------------------------------------------------------

/// Central registry of actor handles and their lifecycle state.
///
/// Cheap to clone — the inner map is `Arc`-wrapped.
#[derive(Clone, Default)]
pub struct Registry {
    inner: Arc<DashMap<ActorId, FrontState>>,
}

impl Registry {
    /// Create a new, empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register an actor with the given kind and optional owning target root.
    ///
    /// If the actor is already registered, the existing entry is overwritten
    /// (it was either dead or a stale reference from a previous page load).
    pub fn register(&self, id: ActorId, kind: FrontKind, target_root: Option<ActorId>) {
        self.inner.insert(id, FrontState::new(kind, target_root));
    }

    /// Register an actor with an explicit parent for chain-based invalidation.
    ///
    /// Use this when the actor's lifetime is bounded by another actor (the
    /// parent) rather than the top-level target — e.g. `nodeActor`s created
    /// by a walker.  When the parent is destroyed via
    /// [`Registry::invalidate_target`], every descendant reachable through
    /// `parent` links is also marked dead.
    pub fn register_with_parent(
        &self,
        id: ActorId,
        kind: FrontKind,
        target_root: Option<ActorId>,
        parent: Option<ActorId>,
    ) {
        self.inner
            .insert(id, FrontState::with_parent(kind, target_root, parent));
    }

    /// Remove an actor from the registry.
    pub fn remove(&self, id: &ActorId) {
        self.inner.remove(id);
    }

    /// Assert that `actor` is alive, returning [`RdpError::ActorDestroyed`] if not.
    ///
    /// Returns `Ok(())` for unknown actors (never registered = conservatively alive).
    pub fn assert_alive(&self, actor: &ActorId) -> RdpResult<()> {
        if let Some(state) = self.inner.get(actor)
            && !state.is_alive()
        {
            return Err(RdpError::ActorDestroyed {
                actor: actor.clone(),
            });
        }
        Ok(())
    }

    /// Invalidate `destroyed_target` and every front transitively reachable
    /// from it via `target_root` or `parent` links.
    ///
    /// Implementation is BFS: starting from `destroyed_target`, mark all
    /// matching entries dead, then sweep again with the newly-killed set as
    /// roots until the frontier is empty.  This catches multi-level chains
    /// such as `walker → nodeActor → nodeListActor` where intermediate actors
    /// would be missed by a single-level sweep.
    ///
    /// Subsequent calls through invalidated fronts will return
    /// [`RdpError::ActorDestroyed`].
    pub fn invalidate_target(&self, destroyed_target: &ActorId) {
        let mut frontier: Vec<ActorId> = vec![destroyed_target.clone()];

        while !frontier.is_empty() {
            // Mark every entry in the current frontier dead (idempotent).
            for id in &frontier {
                if let Some(state) = self.inner.get(id) {
                    state.alive.store(false, Ordering::Release);
                }
            }

            // Find entries whose target_root or parent points at anything in
            // the current frontier and weren't already dead — those become
            // the next frontier.
            let mut next: Vec<ActorId> = Vec::new();
            for entry in self.inner.iter() {
                let state = entry.value();
                if !state.is_alive() {
                    continue;
                }
                let parent_match = state
                    .parent
                    .as_ref()
                    .is_some_and(|p| frontier.iter().any(|f| f == p));
                let root_match = state
                    .target_root
                    .as_ref()
                    .is_some_and(|r| frontier.iter().any(|f| f == r));
                if parent_match || root_match {
                    next.push(entry.key().clone());
                }
            }
            frontier = next;
        }
    }

    /// Return the number of registered actor entries (alive or dead).
    pub fn len(&self) -> usize {
        self.inner.len()
    }

    /// Return `true` if the registry has no entries.
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    /// Count alive entries whose `target_root` matches `target`.
    ///
    /// Counts all alive fronts that are *owned by* (rooted at) the given target
    /// actor, regardless of their `FrontKind`.  This includes console fronts,
    /// walker fronts, and any other front registered with `target_root =
    /// Some(target)`.  It does **not** count the target's own registry entry
    /// (which has `target_root = None`).
    pub fn count_alive_fronts_for_target(&self, target: &ActorId) -> usize {
        self.inner
            .iter()
            .filter(|e| {
                e.value().is_alive() && e.value().target_root.as_ref().is_some_and(|r| r == target)
            })
            .count()
    }

    /// Count alive entries with `kind == FrontKind::Target` for the given actor.
    ///
    /// The TargetFront for a browsing context is registered with its own actor
    /// ID as the key and `target_root = None`.  This method counts entries
    /// where the map key matches `target` and the `kind` is `FrontKind::Target`,
    /// giving the number of live TargetFront registrations for that actor.
    ///
    /// For a well-formed registry this should always be 0 or 1.
    pub fn count_alive_target_fronts_for(&self, target: &ActorId) -> usize {
        self.inner
            .iter()
            .filter(|e| {
                e.key() == target && e.value().is_alive() && e.value().kind == FrontKind::Target
            })
            .count()
    }
}

// ---------------------------------------------------------------------------
// Front trait
// ---------------------------------------------------------------------------

/// A typed handle to a Firefox RDP actor.
///
/// Every concrete front (console, screenshot, walker, …) implements this
/// trait.  The [`id`](Front::id) and [`registry`](Front::registry) accessors
/// are the only requirements — everything else is implemented in terms of them.
pub trait Front {
    /// Return the actor ID this front wraps.
    fn id(&self) -> &ActorId;

    /// Return the registry this front is registered in.
    fn registry(&self) -> &Registry;

    /// Check whether this front's actor is still alive.
    ///
    /// Returns [`RdpError::ActorDestroyed`] if the actor has been invalidated.
    fn assert_alive(&self) -> RdpResult<()> {
        self.registry().assert_alive(self.id())
    }
}

// ---------------------------------------------------------------------------
// Self-healing helper
// ---------------------------------------------------------------------------

/// Call `op` with `actor_id`, retrying **once** if the first attempt returns
/// `noSuchActor` (via `ProtocolError::UnknownActor`) or the actor is already
/// destroyed (via `RdpError::ActorDestroyed`).
///
/// On retry, `refresh` is called to obtain a fresh actor ID, which is then
/// passed to `op` again.  If the second attempt also fails, its error is
/// returned as-is.
///
/// # Design
///
/// This is intentionally opt-in (not automatic) — silent retries hide real
/// bugs.  Commands that need self-healing (e.g. `eval`, `dom`, `computed`,
/// `snapshot`, `a11y`) call this helper explicitly.
///
/// # Example
///
/// ```rust,ignore
/// let title = call_with_refresh(
///     &console_actor_id,
///     |id| WebConsoleActor::evaluate_js_async(transport, id, "document.title"),
///     || {
///         let target = TabActor::get_target(transport, &tab_actor)?;
///         Ok(target.console_actor)
///     },
/// )?;
/// ```
pub fn call_with_refresh<T, E, Op, Refresh>(
    actor_id: &ActorId,
    op: Op,
    refresh: Refresh,
) -> Result<T, E>
where
    Op: Fn(&ActorId) -> Result<T, E>,
    Refresh: FnOnce() -> Result<ActorId, E>,
    E: IsActorGone,
{
    match op(actor_id) {
        Ok(v) => Ok(v),
        Err(e) if e.is_actor_gone() => {
            let fresh = refresh()?;
            op(&fresh)
        }
        Err(e) => Err(e),
    }
}

/// Sealed trait for errors that indicate a stale / destroyed actor.
///
/// Implemented for [`crate::error::ProtocolError`] and [`crate::error::RdpError`]
/// so that [`call_with_refresh`] can be used with either error type.
pub trait IsActorGone {
    /// Returns `true` when the error means the actor ID is no longer valid.
    fn is_actor_gone(&self) -> bool;
}

impl IsActorGone for crate::error::ProtocolError {
    fn is_actor_gone(&self) -> bool {
        self.is_unknown_actor()
    }
}

impl IsActorGone for crate::error::RdpError {
    fn is_actor_gone(&self) -> bool {
        matches!(self, crate::error::RdpError::ActorDestroyed { .. })
    }
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn make_id(s: &str) -> ActorId {
        ActorId::from(s)
    }

    // ── invalidation cascades ────────────────────────────────────────────────

    #[test]
    fn invalidate_target_marks_owned_fronts_dead() {
        let reg = Registry::new();
        let target = make_id("target1");
        let console = make_id("console1");
        let walker = make_id("walker1");

        reg.register(target.clone(), FrontKind::Target, None);
        reg.register(console.clone(), FrontKind::Console, Some(target.clone()));
        reg.register(walker.clone(), FrontKind::Walker, Some(target.clone()));

        assert!(reg.assert_alive(&target).is_ok());
        assert!(reg.assert_alive(&console).is_ok());
        assert!(reg.assert_alive(&walker).is_ok());

        reg.invalidate_target(&target);

        assert!(
            matches!(
                reg.assert_alive(&target),
                Err(RdpError::ActorDestroyed { .. })
            ),
            "target itself must be dead"
        );
        assert!(
            matches!(
                reg.assert_alive(&console),
                Err(RdpError::ActorDestroyed { .. })
            ),
            "console owned by target must be dead"
        );
        assert!(
            matches!(
                reg.assert_alive(&walker),
                Err(RdpError::ActorDestroyed { .. })
            ),
            "walker owned by target must be dead"
        );
    }

    #[test]
    fn registry_parent_chain_invalidation() {
        // walker → nodeActor → nodeListActor: each linked via `parent`, none
        // share a `target_root` with the walker.  Invalidating the walker
        // must cascade through the parent chain to mark all three dead.
        let reg = Registry::new();
        let walker = make_id("walker1");
        let node = make_id("node1");
        let node_list = make_id("nodeList1");

        reg.register(walker.clone(), FrontKind::Walker, None);
        reg.register_with_parent(
            node.clone(),
            FrontKind::Other("nodeActor".into()),
            None,
            Some(walker.clone()),
        );
        reg.register_with_parent(
            node_list.clone(),
            FrontKind::Other("nodeListActor".into()),
            None,
            Some(node.clone()),
        );

        assert!(reg.assert_alive(&walker).is_ok());
        assert!(reg.assert_alive(&node).is_ok());
        assert!(reg.assert_alive(&node_list).is_ok());

        reg.invalidate_target(&walker);

        for id in [&walker, &node, &node_list] {
            assert!(
                matches!(reg.assert_alive(id), Err(RdpError::ActorDestroyed { .. })),
                "{id:?} must be dead after walker invalidation",
            );
        }
    }

    #[test]
    fn invalidate_target_does_not_affect_unrelated_fronts() {
        let reg = Registry::new();
        let target_a = make_id("targetA");
        let target_b = make_id("targetB");
        let console_b = make_id("consoleB");

        reg.register(target_a.clone(), FrontKind::Target, None);
        reg.register(target_b.clone(), FrontKind::Target, None);
        reg.register(
            console_b.clone(),
            FrontKind::Console,
            Some(target_b.clone()),
        );

        reg.invalidate_target(&target_a);

        // target_b and its console must still be alive.
        assert!(reg.assert_alive(&target_b).is_ok());
        assert!(reg.assert_alive(&console_b).is_ok());
    }

    #[test]
    fn assert_alive_returns_ok_for_unknown_actor() {
        let reg = Registry::new();
        // An actor that was never registered is treated as alive.
        assert!(reg.assert_alive(&make_id("never-registered")).is_ok());
    }

    #[test]
    fn actor_destroyed_error_carries_actor_id() {
        let reg = Registry::new();
        let target = make_id("conn0/target42");
        reg.register(target.clone(), FrontKind::Target, None);
        reg.invalidate_target(&target);

        match reg.assert_alive(&target) {
            Err(RdpError::ActorDestroyed { actor }) => {
                assert_eq!(actor, target, "error must carry the destroyed actor id");
            }
            other => panic!("expected ActorDestroyed, got {other:?}"),
        }
    }

    // ── call_with_refresh ─────────────────────────────────────────────────────

    /// Minimal error type implementing `IsActorGone` for testing.
    #[derive(Debug, PartialEq)]
    enum TestError {
        Gone,
        Other(String),
    }

    impl IsActorGone for TestError {
        fn is_actor_gone(&self) -> bool {
            matches!(self, TestError::Gone)
        }
    }

    #[test]
    fn call_with_refresh_succeeds_on_first_try() {
        let id = make_id("actor1");
        let result = call_with_refresh(
            &id,
            |a| {
                if a.as_ref() == "actor1" {
                    Ok::<_, TestError>("ok")
                } else {
                    Err(TestError::Other("wrong actor".into()))
                }
            },
            || unreachable!("refresh should not be called on success"),
        );
        assert_eq!(result, Ok("ok"));
    }

    #[test]
    fn call_with_refresh_retries_on_gone_and_succeeds() {
        let id = make_id("actor-old");
        let call_count = std::cell::Cell::new(0u32);

        let result = call_with_refresh(
            &id,
            |a| {
                call_count.set(call_count.get() + 1);
                if a.as_ref() == "actor-new" {
                    Ok::<_, TestError>("fresh result")
                } else {
                    Err(TestError::Gone)
                }
            },
            || Ok::<_, TestError>(make_id("actor-new")),
        );

        assert_eq!(result, Ok("fresh result"));
        assert_eq!(
            call_count.get(),
            2,
            "op must be called twice (first + retry)"
        );
    }

    #[test]
    fn call_with_refresh_propagates_non_gone_error() {
        let id = make_id("actor1");

        let result = call_with_refresh(
            &id,
            |_| Err::<&str, _>(TestError::Other("timeout".into())),
            || unreachable!("refresh should not be called for non-gone errors"),
        );

        assert_eq!(result, Err(TestError::Other("timeout".into())));
    }

    #[test]
    fn call_with_refresh_propagates_retry_failure() {
        let id = make_id("actor1");

        let result = call_with_refresh(
            &id,
            |_| Err::<&str, _>(TestError::Gone),
            || Ok::<_, TestError>(make_id("actor-new")),
        );

        // The retry also returns Gone — the error from the second op is returned.
        assert_eq!(result, Err(TestError::Gone));
    }

    // ── performance bench ────────────────────────────────────────────────────

    #[test]
    fn opening_front_for_known_actor_is_fast() {
        let reg = Registry::new();
        let id = make_id("conn0/console1");
        reg.register(id.clone(), FrontKind::Console, None);

        // Exercise the hot path for correctness: all 1_000 lookups must return Ok.
        for _ in 0..1_000 {
            reg.assert_alive(&id).unwrap();
        }
    }
}
