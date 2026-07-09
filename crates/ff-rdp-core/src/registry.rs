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

use std::collections::HashSet;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use dashmap::DashMap;

use crate::error::{RdpError, RdpResult};
use crate::types::ActorId;

// ---------------------------------------------------------------------------
// FrontKind
// ---------------------------------------------------------------------------

/// Which kind of Firefox actor this front represents.
///
/// `#[non_exhaustive]` (iter-105 Theme B / DEC-019): named variants are added
/// nearly every iteration (`Manifest` in iter-104, `TargetConfiguration` in
/// iter-103, …).  The `Other(String)` catch-all already round-trips
/// unrecognised kinds by name, but the attribute additionally makes adding a
/// *named* variant non-breaking for downstream `match`es.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
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
    NetworkParent,
    Manifest,
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
    pub(crate) parent: Option<ActorId>,
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
    #[cfg(test)]
    pub(crate) fn with_parent(
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
    /// If the actor is already registered, the *new* entry is normally
    /// installed — except when the previous entry was marked dead
    /// (`alive=false`).  In that case we keep the dead state (so callers
    /// that still hold a stale handle continue to surface
    /// `RdpError::ActorDestroyed`) and emit a `tracing::warn!` recording the
    /// re-register attempt.  With iter-74's registry-invalidation lifecycle
    /// in place this branch should be effectively unreachable in
    /// production; the warn lets us spot regressions.
    ///
    /// # Atomicity (iter-101 Theme E)
    ///
    /// The check-and-install is performed under a single `DashMap::entry`
    /// lock so a concurrent [`Registry::invalidate_target`] cannot slip
    /// between the "is the existing entry dead?" test and the insert.  The
    /// previous non-atomic `get()` + `insert()` had a window in which thread A
    /// passed the alive check, thread B marked the actor dead, and thread A's
    /// insert then *revived* it with a fresh `alive=true` state — the exact
    /// race this API now closes.
    pub fn register(&self, id: ActorId, kind: FrontKind, target_root: Option<ActorId>) {
        use dashmap::mapref::entry::Entry;

        match self.inner.entry(id) {
            Entry::Occupied(mut occ) => {
                // Hold the shard lock across the alive check and the mutation so
                // `invalidate_target` (which sets `alive=false` under the same
                // shard lock via `get`) is serialized with respect to us.
                if occ.get().is_alive() {
                    // Live re-registration: install the fresh state.
                    occ.insert(FrontState::new(kind, target_root));
                } else {
                    tracing::warn!(
                        actor = %occ.key(),
                        kind = ?kind,
                        "registry: refusing to revive dead actor — keeping alive=false"
                    );
                    // Leave the dead entry untouched.
                }
            }
            Entry::Vacant(vac) => {
                vac.insert(FrontState::new(kind, target_root));
            }
        }
    }

    /// Register an actor with an explicit parent for chain-based invalidation.
    ///
    /// Use this when the actor's lifetime is bounded by another actor (the
    /// parent) rather than the top-level target — e.g. `nodeActor`s created
    /// by a walker.  When the parent is destroyed via
    /// [`Registry::invalidate_target`], every descendant reachable through
    /// `parent` links is also marked dead.
    #[cfg(test)]
    pub(crate) fn register_with_parent(
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

    /// Invalidate `actor` and every front transitively reachable from it via
    /// `target_root` or `parent` links.
    ///
    /// `actor` may be any registered actor ID — a TargetFront, a walker, or
    /// any other front that owns descendants through `target_root`/`parent`.
    ///
    /// Implementation is BFS: starting from `actor`, mark all matching
    /// entries dead, then sweep again with the newly-killed set as roots
    /// until the frontier is empty.  This catches multi-level chains such as
    /// `walker → nodeActor → nodeListActor` where intermediate actors would
    /// be missed by a single-level sweep.
    ///
    /// Subsequent calls through invalidated fronts will return
    /// [`RdpError::ActorDestroyed`].
    pub fn invalidate_target(&self, actor: &ActorId) {
        let mut frontier: HashSet<ActorId> = HashSet::new();
        frontier.insert(actor.clone());

        while !frontier.is_empty() {
            // Mark every entry in the current frontier dead (idempotent).
            for id in &frontier {
                if let Some(state) = self.inner.get(id) {
                    state.alive.store(false, Ordering::Release);
                }
            }

            // Find entries whose target_root or parent points at anything in
            // the current frontier and weren't already dead — those become
            // the next frontier.  Use a HashSet for O(1) membership and to
            // dedupe `next`.
            let mut next: HashSet<ActorId> = HashSet::new();
            for entry in self.inner.iter() {
                let state = entry.value();
                if !state.is_alive() {
                    continue;
                }
                let parent_match = state.parent.as_ref().is_some_and(|p| frontier.contains(p));
                let root_match = state
                    .target_root
                    .as_ref()
                    .is_some_and(|r| frontier.contains(r));
                if parent_match || root_match {
                    next.insert(entry.key().clone());
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

    // ── iter-74: invalidate_target_removes_dependents ────────────────────────

    /// AC: `registry_invalidate_target_removes_dependents` — calling
    /// `invalidate_target` on a target actor marks the target itself dead
    /// AND cascades to every dependent front (inspector, walker, console)
    /// that has `target_root` pointing at it.  Unrelated fronts are unaffected.
    #[test]
    fn registry_invalidate_target_removes_dependents() {
        let reg = Registry::new();
        let target = make_id("conn0/windowGlobalTarget1");
        let inspector = make_id("conn0/inspector1");
        let walker = make_id("conn0/walker1");
        let console = make_id("conn0/console1");
        let unrelated_target = make_id("conn0/windowGlobalTarget2");
        let unrelated_console = make_id("conn0/console2");

        reg.register(target.clone(), FrontKind::Target, None);
        reg.register(
            inspector.clone(),
            FrontKind::Other("inspector".into()),
            Some(target.clone()),
        );
        reg.register(walker.clone(), FrontKind::Walker, Some(target.clone()));
        reg.register(console.clone(), FrontKind::Console, Some(target.clone()));
        reg.register(unrelated_target.clone(), FrontKind::Target, None);
        reg.register(
            unrelated_console.clone(),
            FrontKind::Console,
            Some(unrelated_target.clone()),
        );

        // All alive before invalidation.
        for id in [
            &target,
            &inspector,
            &walker,
            &console,
            &unrelated_target,
            &unrelated_console,
        ] {
            assert!(
                reg.assert_alive(id).is_ok(),
                "{id:?} should be alive before invalidation"
            );
        }

        reg.invalidate_target(&target);

        // Target and its dependents are dead.
        for id in [&target, &inspector, &walker, &console] {
            assert!(
                matches!(reg.assert_alive(id), Err(RdpError::ActorDestroyed { .. })),
                "{id:?} should be dead after target invalidation"
            );
        }

        // Unrelated fronts are unaffected.
        assert!(
            reg.assert_alive(&unrelated_target).is_ok(),
            "unrelated target must still be alive"
        );
        assert!(
            reg.assert_alive(&unrelated_console).is_ok(),
            "unrelated console must still be alive"
        );
    }

    /// AC: `registry_re_register_preserves_dead` — re-registering an actor
    /// whose previous state was `alive=false` must keep the dead marker.
    /// Iter-74's invalidation lifecycle should make this branch unreachable
    /// in production, so the policy is "preserve dead + warn" — preferred
    /// over `debug_assert!` so a stray legacy code path cannot panic the
    /// CLI under `cfg(debug_assertions)`.
    #[test]
    fn registry_re_register_preserves_dead() {
        let reg = Registry::new();
        let id = make_id("conn0/target1");
        reg.register(id.clone(), FrontKind::Target, None);
        reg.invalidate_target(&id);
        assert!(matches!(
            reg.assert_alive(&id),
            Err(RdpError::ActorDestroyed { .. })
        ));

        // Re-register: must NOT revive the actor.
        reg.register(id.clone(), FrontKind::Target, None);
        assert!(
            matches!(reg.assert_alive(&id), Err(RdpError::ActorDestroyed { .. })),
            "re-register of a dead actor must keep alive=false"
        );
    }

    // ── iter-101 Theme E: atomic register vs invalidate ──────────────────────

    /// AC: `unit_registry_register_atomic_no_revive` — a concurrent
    /// `register` must never observably revive an actor that
    /// `invalidate_target` marked dead.
    ///
    /// Two threads hammer the *same* actor id: one repeatedly re-registers it,
    /// the other repeatedly invalidates it.  Because `register`'s check-and-
    /// install now runs under a single `DashMap::entry` shard lock (iter-101),
    /// once the actor is dead it can never flip back to alive.  We assert the
    /// monotonic-death invariant *during* the race (not just at the end): the
    /// instant `assert_alive` reports the actor dead, it must stay dead for the
    /// remainder of the run.
    #[test]
    fn unit_registry_register_atomic_no_revive() {
        use std::sync::Arc;
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::thread;

        let reg = Registry::new();
        let id = make_id("conn0/target-race");
        // Seed one alive registration so both threads start from a known state.
        reg.register(id.clone(), FrontKind::Target, None);

        let stop = Arc::new(AtomicBool::new(false));
        let observed_dead = Arc::new(AtomicBool::new(false));

        let iterations = 50_000;

        let reg_registrar = reg.clone();
        let id_registrar = id.clone();
        let registrar = thread::spawn(move || {
            for _ in 0..iterations {
                reg_registrar.register(id_registrar.clone(), FrontKind::Target, None);
            }
        });

        let reg_invalidator = reg.clone();
        let id_invalidator = id.clone();
        let invalidator = thread::spawn(move || {
            for _ in 0..iterations {
                reg_invalidator.invalidate_target(&id_invalidator);
            }
        });

        // Observer thread: once it sees the actor dead, any subsequent
        // observation of "alive" is a revival bug.
        let reg_observer = reg.clone();
        let id_observer = id.clone();
        let stop_observer = Arc::clone(&stop);
        let observed_dead_observer = Arc::clone(&observed_dead);
        let observer = thread::spawn(move || {
            let mut seen_dead = false;
            while !stop_observer.load(Ordering::Relaxed) {
                let dead = reg_observer.assert_alive(&id_observer).is_err();
                if dead {
                    seen_dead = true;
                    observed_dead_observer.store(true, Ordering::Relaxed);
                } else if seen_dead {
                    // Revived after being observed dead — the exact race we
                    // are guarding against.
                    return Err("actor revived after being observed dead");
                }
            }
            Ok(())
        });

        registrar.join().expect("registrar thread");
        invalidator.join().expect("invalidator thread");
        stop.store(true, Ordering::Relaxed);
        let observer_result = observer.join().expect("observer thread");

        assert!(observer_result.is_ok(), "{}", observer_result.unwrap_err());
        // The invalidator ran to completion, so the final state must be dead.
        assert!(
            matches!(reg.assert_alive(&id), Err(RdpError::ActorDestroyed { .. })),
            "after both threads finish, the actor must be dead (invalidator ran last-or-equal)"
        );
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
