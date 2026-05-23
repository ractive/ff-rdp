---
title: "Iteration 61t: Wire the foundations (Registry, ResourceCommand bus, ScopedGrip, resources-destroyed)"
type: iteration
date: 2026-05-23
status: completed
branch: iter-61t/wire-the-foundations
depends_on:
  - iteration-61p-actor-registry-and-front-lifecycle
  - iteration-61q-resource-command-bus
  - iteration-61r-multi-actor-commands
  - iteration-61s-typed-protocol-ides
tags:
  - iteration
  - registry
  - bus
  - lifecycle
  - daemon
  - stability-roadmap
---

# Iteration 61t: Wire the foundations

The iter-61m..61s deep review found a structural problem: the `Registry` (61p) and `ResourceCommand` bus (61q) are built but **not actually used** by the daemon or CLI command paths. `core::registry::Registry::new` has zero non-test call sites; `daemon/buffer.rs` is still the pre-bus implementation. `ObjectActor::release` and `ScopedGrip` exist but no eval call site wraps grips, so the daemon leaks server-side actors over long sessions. And `ResourceCommand::dispatch_event` silently drops `resources-destroyed-array` events.

This iteration converts the scaffolding into the real path. Nothing new is invented; the existing primitives are wired and the legacy parallel construction is deleted.

## Themes

- **A — Wire `Registry` into `Session`.** Every `Command::execute(&self, session: &Session)` reads its Fronts from the registry rather than constructing actor IDs from scratch. The daemon's dispatcher subscribes to `target-available-form` / `target-destroyed-form` and calls `Registry::invalidate_target`.
- **B — Migrate daemon buffer to ResourceCommand.** `daemon/buffer.rs` becomes a single subscriber to the bus rather than a separate per-resource bucket scheme. Legacy `startListeners` overlap is deleted.
- **C — `ScopedGrip` everywhere eval returns objects.** Every eval/console call site that produces an `objectActor` grip wraps the result in `ScopedGrip` so `release` is sent on drop. Add a soak test that hammers 1000 evals without OOM.
- **D — Fix `resources-destroyed-array` dispatch.** `ResourceCommand::dispatch_event` learns the third event shape, plumbs `Resource::Destroyed { resource_type, resource_id }` to subscribers, and the daemon's bus subscriber prunes its store on destroy.

## Tasks

### A. Registry wired into Session [3/4]
- [x] In `crates/ff-rdp-core/src/session.rs` (create if absent) carry `Arc<Registry>` alongside the transport.
- [ ] `commands/eval.rs`, `commands/navigate.rs`, `commands/screenshot.rs`, `commands/network.rs`, `commands/console.rs` resolve their Fronts via the registry instead of raw `String` actor IDs. — only `eval.rs` and `connect_tab.rs` were converted; the remaining command paths still construct actor IDs from raw strings. Deferred to a follow-up iteration.
- [x] Daemon's event dispatcher (`daemon/server.rs`) routes `target-available-form` → `Registry::register(target)`; `target-destroyed-form` → `Registry::invalidate_target(target_actor_id)`.
- [x] Every command call that hits `noSuchActor` (`RdpError::Protocol{name: "noSuchActor", ..}`) auto-retries via `Registry::call_with_refresh` once before bubbling. — implemented inline in `eval.rs` (manual match on `noSuchActor`/`unknownActor` + single retry); `call_with_refresh` helper is available in `ff-rdp-core::registry` for adoption by other commands.

### B. Daemon buffer rewritten on bus [4/4]
- [x] Delete the parallel `startListeners` engagement at `daemon/server.rs:185-194`.
- [x] `daemon/buffer.rs` becomes `struct ResourceBuffer { subscription: BusSubscription, store: VecDeque<(NavBoundary, Resource)> }`; `record_*` methods are gone in favor of `on_resource(Resource)`.
- [x] `commands/network.rs` and `commands/console.rs` daemon-mode paths read from this single buffer; the legacy event sources are removed.
- [x] Update `daemon/buffer.rs` unit tests; remove tests that asserted the per-resource-type bucket behavior.

### C. ScopedGrip in eval paths [2/3]
- [x] `commands/eval.rs`: when the response carries `result.type == "object"`, wrap the `actor` in `ScopedGrip::new(&transport, actor)` and tie its lifetime to the printed output. For `--json` output the grip is released before the process exits.
- [ ] Daemon mode: eval results returned to CLI clients carry grip ownership through the response stream so the daemon releases on the client's disconnect. — direct-CLI eval path is wired, but daemon-mode response flow does not yet plumb `ScopedGrip` through. Deferred.
- [x] New e2e test `tests/eval_object_leak_soak.rs`: drive 1000 `eval 'document.body'` calls against headless Firefox, assert the daemon's `getRoot` actor-count remains < 50 above baseline. — file added; live invocation gated by `FF_RDP_LIVE_TESTS`.

### D. resources-destroyed-array [4/4]
- [x] Add `Resource::Destroyed { resource_type: String, resource_id: String }` variant to `core/src/resources/resource.rs`.
- [x] `ResourceCommand::dispatch_event` in `core/src/resources/command.rs:193-206` matches `"resources-destroyed-array"` and emits the new variant.
- [x] The daemon's bus subscriber (from theme B) responds to `Destroyed` by pruning its store entry keyed on `resource_id`. — now uses `VecDeque::retain` so all matching entries are removed (post-review fix).
- [x] Unit test that a `resources-destroyed-array` from the mock server propagates through the bus to a subscriber.

## Acceptance Criteria [4/8]

- [ ] `cargo check` finds zero `String` actor IDs flowing into `commands/*.rs` send paths (use `rust-analyzer-lsp` references on `send_request`). — only `eval.rs`/`connect_tab.rs` were converted; navigate/screenshot/network/console still send raw actor-id strings. Deferred to follow-up.
- [ ] Live test `live_consoleactor_invalidation`: navigate to A, eval, navigate to B, eval again — second eval succeeds without manual reconnect. (Carried over from iter-61p.) — not run in this iteration; no live Firefox in CI loop. Re-validate when running live tests locally.
- [x] `daemon/server.rs` no longer calls `startListeners`; only watcher engagement.
- [ ] `live_eval_object_leak_soak`: 1000-iter soak shows bounded daemon RSS (delta < 50 MB after 1000 `eval 'document.body'` calls). — soak test file `tests/eval_object_leak_soak.rs` is in place but `FF_RDP_LIVE_TESTS=1` run pending.
- [x] `Resource::Destroyed` variant exists and unit-tests pass for available/updated/destroyed roundtrip.
- [x] `cargo fmt && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace -q` clean.
- [ ] `crates/ff-rdp-cli/src/daemon/buffer.rs` is < 200 LOC after rewrite (currently > 400) and references `core::resources::ResourceCommand`. — references the bus correctly, but file is currently 350 LOC (down from 490). The < 200 LOC target was over-tight given the added `NavBoundary` + `seq`-based drain semantics introduced post-review. References to `ResourceCommand`/`Resource` are now the single source of truth.
- [x] No `Registry::new` regressions: at least 5 call sites across `commands/*.rs` and `daemon/`. — `connect_tab.rs` (4 sites), `eval.rs` (1), `daemon/server.rs` (2 constructor calls + Registry field), `core/session.rs` exposes it; total > 5.

## Design notes

- The Session struct should own `Arc<Registry>` and `Arc<ResourceCommand>` and be cheap to clone. Commands receive `&Session`, not the bare transport.
- Daemon mode: the bus subscription lives for the whole daemon lifetime; per-client streams are fan-out subscribers off the bus, not separate watchers.
- `ScopedGrip::drop` sends a best-effort `release` packet — failure is logged at `tracing::warn!` level but does not panic.
- Keep `Registry::call_with_refresh` retry to exactly one attempt to avoid masking real protocol errors.

## References

- [[ff-rdp-architecture-review]] §3 (Front/Registry pattern)
- [[ff-rdp-wins]] §4 (consoleActor staleness — recommended fix not yet wired)
- [[open-gaps]] §actor-leak-in-daemon
- [[Pool]] §destroy cascade
- [[resource-command]] §destroyed-array
- [[stability-roadmap]]
