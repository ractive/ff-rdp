---
name: project_ff_rdp_registry
description: Actor Registry + Front lifecycle design choices from iter-61p
metadata:
  type: project
---

# Actor Registry and Front Lifecycle (iter-61p)

Implemented in two commits on `iter-61p/actor-registry-and-front-lifecycle`.

## Key design choices

- `ActorId` upgraded from `String` to `Arc<str>` — cloning is O(1). Serialize/Deserialize added manually (serde doesn't support `Arc<str>` with `#[serde(transparent)]`).
- `Registry` uses `dashmap::DashMap<ActorId, FrontState>` wrapped in `Arc` so it can be cloned cheaply.
- `FrontState.alive` is `AtomicBool` — invalidation from watcher event doesn't need a lock.
- `invalidate_target(destroyed_target)` does a full scan of the map to cascade to owned fronts. Acceptable for small actor counts; revisit if registry grows large.
- `Front` trait is minimal: just `id()` and `registry()`. `assert_alive()` is a default method.
- `call_with_refresh` is generic over `IsActorGone` — works with both `ProtocolError` and `RdpError`.
- Live test `live_consoleactor_invalidation` AC is partially deferred — the registry side is done, but wiring the actual watcher event subscription to call `invalidate_target` is pending (iter-61q).

## Files

- `crates/ff-rdp-core/src/registry.rs` — Registry, Front trait, call_with_refresh, IsActorGone
- `crates/ff-rdp-core/src/fronts/` — 9 concrete Front types
- `crates/ff-rdp-core/src/types.rs` — upgraded ActorId
- `crates/ff-rdp-core/tests/no_string_actor_ids.rs` — CI grep check for bare String actor fields
- `crates/ff-rdp-core/tests/live_61p_registry.rs` — live tests (all gated with #[ignore])

**Why:** The plan says this is "the biggest refactor" — establishes the type-safe actor handle abstraction that all future iters (61q resource bus, 61r multi-actor commands) plug into.

**How to apply:** When working with actor IDs in ff-rdp-core, always use `ActorId` not `String`. Use `Registry::invalidate_target` when a `target-destroyed-form` event arrives. Wire `call_with_refresh` into commands that use console/walker/screenshot actors.
