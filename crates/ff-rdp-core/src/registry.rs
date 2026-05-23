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
    /// Whether the actor is still alive.  Set to `false` by [`Registry::invalidate_target`].
    pub alive: AtomicBool,
}

impl FrontState {
    pub fn new(kind: FrontKind, target_root: Option<ActorId>) -> Self {
        Self {
            kind,
            target_root,
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

    /// Invalidate all fronts whose `target_root` matches `destroyed_target`.
    ///
    /// Sets their `alive` flag to `false` atomically.  Subsequent calls
    /// through those fronts will return [`RdpError::ActorDestroyed`].
    ///
    /// Also invalidates the target front itself if it is registered.
    pub fn invalidate_target(&self, destroyed_target: &ActorId) {
        // Invalidate the target's own entry.
        if let Some(state) = self.inner.get(destroyed_target) {
            state.alive.store(false, Ordering::Release);
        }

        // Cascade to all fronts owned by this target.
        for entry in self.inner.iter() {
            if entry
                .value()
                .target_root
                .as_ref()
                .is_some_and(|root| root == destroyed_target)
            {
                entry.value().alive.store(false, Ordering::Release);
            }
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

    /// Count entries whose `target_root` matches `target` and that are alive.
    ///
    /// Used by tests to assert at-most-one TargetFront per browsing context.
    pub fn count_alive_fronts_for_target(&self, target: &ActorId) -> usize {
        self.inner
            .iter()
            .filter(|e| {
                e.value().is_alive() && e.value().target_root.as_ref().is_some_and(|r| r == target)
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
    use std::time::Instant;

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

        // Warm up.
        for _ in 0..10 {
            reg.assert_alive(&id).unwrap();
        }

        let iterations: u32 = 1_000;
        let start = Instant::now();
        for _ in 0..iterations {
            reg.assert_alive(&id).unwrap();
        }
        let elapsed = start.elapsed();
        let per_op_ns = elapsed.as_nanos() / u128::from(iterations);

        // In debug builds a DashMap lookup is well under 1 ms per op.
        assert!(
            elapsed.as_millis() < 1,
            "1000 registry lookups took {elapsed:?} — expected < 1ms total (got ~{per_op_ns}ns/op)"
        );
    }
}
