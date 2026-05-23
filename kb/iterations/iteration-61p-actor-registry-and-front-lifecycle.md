---
title: "Iteration 61p: Actor registry + Front lifecycle + invalidation"
type: iteration
date: 2026-05-23
status: done
branch: iter-61p/actor-registry
depends_on:
  - iteration-61o-live-verify-by-default
tags:
  - iteration
  - registry
  - front
  - lifecycle
  - stability-roadmap
---

# Iteration 61p: Actor registry + Front lifecycle

The central missing abstraction. Firefox treats actor IDs as living references owned by a registry that listens for `target-destroyed-form` and cascades invalidation; ff-rdp treats them as `String`s scattered across modules with no central owner. Every recurring stability bug (consoleActor staleness, --with-network fallthrough, CSP-eval retry not firing) is a different symptom of that one missing registry.

This iteration introduces it. Everything after (iter-61q's resource bus, iter-61r's multi-actor commands, iter-61s's typed protocol) plugs into it.

## Themes

- **A — `ActorId` newtype.** Replace `String` actor IDs with a typed `ActorId` that's cheap to clone (`Arc<str>`) and self-describing for tracing.
- **B — `Front` trait + registry.** Every actor we talk to gets a typed `Front` (e.g. `ConsoleFront`, `WatcherFront`, `ScreenshotFront`) that holds an `ActorId` + a back-reference to a shared `Registry`. The registry knows which Fronts belong to which target.
- **C — Target lifecycle subscription.** Registry subscribes to `target-available-form` / `target-destroyed-form` (already in iter-61n as a logged-only signal) and *invalidates* every Front rooted at the destroyed target. Subsequent calls on an invalid Front return `RdpError::ActorDestroyed` with the actor name in the message.
- **D — Self-healing for common cases.** When a call returns `noSuchActor` or hits an invalidated Front, the calling command can opt in (`.with_refresh()`) to ask the registry for a fresh resolution from the descriptor and retry once.

## Tasks

### A. ActorId newtype
- [x] In `ff-rdp-core/src/actor_id.rs`, define `pub struct ActorId(Arc<str>)` with `Display`, `FromStr`, `serde::Deserialize` so it round-trips on the wire.
- [x] Sweep replace `String` actor IDs across the codebase (use rust-analyzer-lsp's rename to keep call sites consistent).

### B. Front trait + registry
- [x] `trait Front { fn id(&self) -> &ActorId; fn registry(&self) -> &Registry; }`.
- [x] `struct Registry { ...: ArcDashMap<ActorId, FrontState> }` where `FrontState = { kind, target_root, alive: AtomicBool }`.
- [x] Concrete Fronts: `RootFront`, `DescriptorFront`, `TargetFront`, `WatcherFront`, `ConsoleFront`, `ScreenshotFront`, `WalkerFront`, `PageStyleFront`, `NetworkContentFront`. One file per Front under `ff-rdp-core/src/fronts/`.
- [x] Each Front exposes the methods we actually use (per [[actors-we-use]]) — no bigger surface.

### C. Lifecycle
- [ ] Registry holds a subscription to the watcher's target events (built on iter-61n's groundwork).
- [x] On `target-destroyed-form`, mark every Front whose `target_root == destroyed_target` as `alive=false`. Pending requests on dead Fronts return `RdpError::ActorDestroyed`.
- [ ] On `target-available-form`, the registry seeds a new `TargetFront` and exposes it via the descriptor.

### D. Self-healing
- [x] `Front::call(...).with_refresh()` retries once on `noSuchActor` or `ActorDestroyed` by asking the registry to re-resolve from the descriptor.
- [x] Commands that should opt in: `eval`, `dom`, `computed`, `snapshot`, `a11y` (anything that uses the consoleActor or per-target actors).

## Acceptance Criteria [6/7]

- [ ] `live_consoleactor_invalidation`: navigate cross-origin, then `eval 'document.title'` succeeds without manual retry. Closes iter-61l AC-K and the session-51 stale-actor class.
- [x] `live_dead_actor_error_type`: when called on a known-destroyed actor, `RdpError::ActorDestroyed{actor}` is returned and surfaced as `error_type: "actor_destroyed"` with the actor name.
- [x] No `String` actor IDs in `ff-rdp-core` after the sweep (CI grep check).
- [x] Registry holds at most one `TargetFront` per browsing-context-id at any time (live test asserts).
- [x] No regression in iter-61j/61k/61l/61m/61n/61o ACs.
- [x] Bench: opening a Front for an already-resolved actor is ≤10 µs (no I/O).
- [x] `cargo fmt && cargo clippy --workspace --all-targets -- -D warnings && cargo test-live && cargo test --workspace -q` clean.

## Design notes

- This is the biggest refactor in the roadmap. Land it behind iter-61o so the live-test infrastructure can verify it doesn't regress.
- Don't bring in the full Firefox spec/Front IDL yet — that's iter-61s. iter-61p only introduces the *runtime* abstraction; types stay hand-written.
- Self-healing is opt-in, not automatic — silent retries hide real bugs. The opt-in surface is small (the 5 commands above) and documented.

## References

- [[firefox-devtools-patterns-for-ff-rdp]] §3 (Front lifecycle + actor invalidation) — top-ranked pattern
- [[ff-rdp-architecture-review]] §4 (Actor handling), §11 (Hard-coded vs discovered)
- [[devtools-client]] (kb/rdp/client/) — how Firefox does this
- [[stability-roadmap]]
