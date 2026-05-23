---
title: "Iteration 61s: Typed protocol layer (spec-file → Rust types)"
type: iteration
date: 2026-05-23
status: completed
branch: iter-61s/typed-protocol-ides
depends_on:
  - iteration-61p-actor-registry-and-front-lifecycle
tags:
  - iteration
  - protocol
  - ide
  - types
  - codegen
  - stability-roadmap
---

# Iteration 61s: Typed protocol layer

The last stability win — and the one with the highest churn cost, which is why it lands last. Today every actor request/reply is hand-written `json!({...})` + `Value::as_str().ok_or(...)` extraction. Shape errors are invisible to the compiler. Firefox's `devtools/shared/specs/*.js` files declare every method with typed args/returns; we should mirror those declarations in Rust so the compiler enforces protocol shape.

This iteration introduces the typed layer and migrates the high-traffic actors. Long-tail actors stay on the Value path and migrate opportunistically.

## Themes

- **A — Spec module per actor.** `ff-rdp-core/src/specs/<actor>.rs` declares `Request` and `Response` types per method. Mirrors the Firefox `devtools/shared/specs/<actor>.js`. Hand-written, not codegen (for now).
- **B — `Front::call` typed.** `WatcherFront::watch_resources(types: &[ResourceType]) -> Result<()>` instead of `front.send_request("watchResources", json!({...}))`.
- **C — Doc-comment header per spec.** Every spec module quotes the relevant Firefox spec file's URL + commit hash. Drift is a code-review issue.
- **D — Migration order.** High-traffic first: console, watcher, screenshot, network-event, walker, page-style. Then descriptors. Then root. Other actors as needed.

## Tasks

### A. Spec module scaffold
- [x] `ff-rdp-core/src/specs/mod.rs` re-exports per-actor modules.
- [x] Each module: `pub mod request { ... }`, `pub mod response { ... }`, both `serde::Serialize`/`Deserialize`. Per-method types use the method name (e.g. `EvaluateJSAsync`, `WatchResources`).
- [x] Module header doc-comment links to the Firefox spec file (e.g. `https://searchfox.org/mozilla-central/source/devtools/shared/specs/webconsole.js#42`).

### B. Front API
- [x] `Front` trait grows `async fn call<T: Method>(&self, args: T::Args) -> Result<T::Reply>`. `Method` is a sealed trait per actor method.
- [x] Refactor `WatcherFront`, `ConsoleFront`, `ScreenshotFront`, `NetworkEventFront`, `WalkerFront`, `PageStyleFront`, `RootFront`, `DescriptorFront`, `TargetFront` to expose typed methods.
- [x] Per-method types own their own JSON serde; no Value plumbing inside the Front body.

### C. Doc-comments + drift check
- [x] Each spec module's header lists the upstream URL.
- [x] `scripts/check-spec-drift.sh` (optional, best-effort): given a known-good Firefox commit, sanity-check that our spec types match the upstream `*.js` field names. Doesn't have to be perfect — even a per-actor "smoke test" that the `json!` of our `Request` matches a fixture from the Firefox source helps.

### D. Migration of long-tail actors [1/2]
- [ ] `storage`, `accessibility`, `performance`, `thread`, `responsive`, `device`, `inspector`, `responsive` (when used), `network-content` follow the same pattern. — Only `network-content` got a typed `NetworkContentFront` (reusing the `network_event` spec module); the rest of the long-tail actors stay on the existing `Value`-based path and are deferred to a follow-up iteration.
- [x] Anything not migrated keeps using the `Value`-based `send_request`; flagged with `#[allow(dead_code)]` or removed if unused.

## Acceptance Criteria [6/7]

- [x] At least 9 actors (console, watcher, screenshot, network-event, walker, page-style, root, descriptor, target) use typed Front methods exclusively.
- [x] `cargo grep '\.send_request("'` returns 0 matches inside the migrated Fronts (use a different mechanism for unmigrated long-tail actors).
- [ ] CI grep check: no `Value::as_str()` / `Value::as_object()` inside `ff-rdp-core/src/fronts/` (the typed layer). — Strictly violated by the push-event filtering loops added to `fronts/root.rs` and `fronts/watcher.rs` in response to PR review (skip `from==<actor> && type.is_some()` packets before deserializing). These ~8 lines are transport-level routing, not payload parsing — the typed reply itself is still decoded via `serde_json::from_value`. Either relax the AC to allow filtering helpers, or hoist the filter into a shared `actor.rs` helper in a follow-up.
- [x] Each migrated spec module has a doc-comment header linking to the upstream Firefox source.
- [x] No regression in iter-61j/61k/61l/61m/61n/61o/61p/61q/61r ACs.
- [x] Adding a new actor method requires: (1) add to spec module, (2) add to Front impl. No `Value` plumbing.
- [x] `cargo fmt && cargo clippy --workspace --all-targets -- -D warnings && cargo test-live && cargo test --workspace -q` clean.

## Design notes

- **No codegen yet.** The Firefox spec DSL (`Arg`, `Option`, `RetVal`, `nullable:dom-node`, `grip`) is rich enough that a 1-shot codegen is more work than hand-writing the 30-ish methods we actually use. Revisit codegen if the typed layer doubles in size.
- **Grips stay separate.** `Grip` decoding (the LongString / Object / actor-reference type) keeps its existing helper; spec methods return `Grip` where applicable.
- **Optional fields.** Use `#[serde(skip_serializing_if = "Option::is_none")]` per spec field; many Firefox methods have evolved with new optional args.
- **One module per actor, not per method.** Otherwise the file count explodes.

## References

- [[firefox-devtools-patterns-for-ff-rdp]] §1 (Spec/Front as typed IDL), §14 (Documentation as code: spec files)
- [[spec-and-front]] (kb/rdp/client/) — how Firefox's framework looks
- [[ff-rdp-architecture-review]] §3 (RDP protocol layer) — the current `Value`-everywhere state
- [[stability-roadmap]]
