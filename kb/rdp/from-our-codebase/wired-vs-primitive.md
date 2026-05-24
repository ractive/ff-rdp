---
title: Wired vs Primitive — what the 61p/q/r/t/u landings actually plug in
type: reference
date: 2026-05-23
tags: [rdp, from-codebase, architecture, stability-roadmap]
closed-in:
  - iter-61t
  - iter-61u
  - iter-61v
---

# Wired vs Primitive

Snapshot of the abstractions introduced by the iter-61p..iter-61u stability-roadmap iterations and which of them are actually **load-bearing** in production code paths (daemon / CLI command), versus which exist only as primitives ready for a future caller.

The distinction matters because the iter-61t review specifically called out the
foundation iterations as having shipped scaffolding without wiring it up
("`core::registry::Registry::new` has zero non-test call sites").  iter-61t and
iter-61u closed that gap for the highest-leverage primitives; the rest of this
page tracks what remains.

## Legend

- **Wired** — used on a hot production path (daemon dispatcher, a CLI command).
- **Primitive** — type/function exists and is unit-tested, no production call site yet.
- **Vestigial** — present but its old call sites were deleted in favour of a newer abstraction.

## Registry (iter-61p → wired in iter-61t)

**Status: wired.**

- `Registry::new` is constructed once per daemon connection
  (`crates/ff-rdp-cli/src/daemon/server.rs:253`, `:1417`) and once per
  short-lived CLI `Session` (`crates/ff-rdp-core/src/session.rs:57`).
- The daemon dispatcher subscribes to `target-available-form` /
  `target-destroyed-form` events and calls `Registry::invalidate_target` on
  each (see `daemon/server.rs:2281` "Registry integration" block).
- Walker and NetworkContent Fronts read from the registry via
  `Front::registry()` (`fronts/walker.rs:72`, `fronts/network_content.rs:57`).

Result: the `consoleActor`-staleness class of bugs (open-gaps before iter-61t)
is now invalidated automatically rather than after a manual `getTarget`.

## ResourceCommand bus (iter-61q → wired in iter-61t)

**Status: wired.**

- The daemon creates one bus per connection
  (`daemon/server.rs:205`: `ResourceCommand::new(watcher_actor.clone())`) and
  subscribes to the full daemon resource set on startup
  (`daemon/server.rs:208`).
- `commands/navigate.rs` constructs a per-command bus
  (`commands/navigate.rs:564`) and gates `dom-loading` / `dom-interactive` /
  `dom-complete` waits on `Resource` deliveries.
- `daemon/buffer.rs` was rewritten to be a single subscriber to the bus rather
  than the per-resource bucket scheme that existed before iter-61t.
- `resources-destroyed-array` events are dispatched (no longer silently
  dropped — iter-61t fix).
- Bus throttle was lowered to zero in iter-61v so a fast cross-origin navigate
  cannot race the wait setup.

## ScopedGrip + ObjectActor::release (iter-54/iter-61r → wired in iter-61t)

**Status: wired (eval); primitive (other grip call sites).**

- `commands/eval.rs:295` wraps `Object` / `LongString` grips in `ScopedGrip` on
  the eval reply, so the daemon's eval path no longer leaks server-side actors
  across long sessions.
- Other call sites that handle grips (`commands/inspector.rs`, network response
  body fetch) still receive raw `Grip` values; the wrapping pattern is
  available but not yet applied uniformly.  Soak test for bounded actor
  count is still on the iter-54 task-4 deferred list.

## Multi-actor Command abstraction (iter-61r → wired)

**Status: wired (screenshot, eval, navigate); primitive (inspector).**

- `screenshot --full-page` resolves the root-scoped `screenshot` actor +
  content-scope `screenshot-content`, calls `prepareCapture` then `capture`
  with `fullpage:true, rect, snapshotScale, browsingContextID`.  Verified by
  `live_screenshot_full_page_dpr2` (iter-61v).
- `eval` sends `mapped: { await: true }` by default and surfaces
  `meta.eval_path: "await" | "plain"`.
- `navigate` waits on the `document-event` resource (specifically
  `dom-interactive`) through the ResourceCommand bus; `tabNavigated` is
  consumed only as an abort signal inside `evaluate_js_async`, not as a
  navigate-completion signal.  Bad-DNS lands on `about:neterror` and returns
  a structured error.  (Updated iter-70 — earlier doc claimed both were
  awaited for navigate completion.)
- Inspector's command coordination (`dom-walker` traversal, shadow-DOM piercing
  — see [[open-gaps#shadow-dom-piercing]]) is still imperative and does not
  ride the multi-actor Command primitive yet.

## Typed protocol spec (iter-61s → wired in iter-61t/61u)

**Status: wired (watcher, console, screenshot, object, longstring); primitive (thread, storage, accessibility, page-style).**

- Spec modules under `crates/ff-rdp-core/src/specs/` define typed `request::*`
  and `reply::*` structs per method, and a marker type with `const NAME`.
- Watcher, WebConsole, Screenshot/ScreenshotContent, Object and LongString
  Fronts call `call::<spec::Method>(transport, &id, &args)` and rely on serde
  for shape validation.  Renames (iter-61u) brought spec names back in line
  with Firefox's IDL.
- Thread, Storage, Accessibility and PageStyle still use the older
  `send → parse Value` pair.  Migration would be mechanical but has no
  blocking business value today.

## Watcher get*Actor methods (iter-61u → primitive)

**Status: primitive across the board (except watch/unwatch/resources).**

See [[watcher#method-support-matrix]] for the per-method breakdown.
Summary: every method has a typed Front method as of iter-61u, but only
`watch*` / `unwatch*` / `Resources` (via bus) are wired into production paths.
The remaining `get*Actor` methods are ready for a future iteration that needs
network throttling, pause-on-exception toggles, viewport overrides via the
target-configuration actor, etc.

## Live-by-default test infrastructure (iter-61o → wired)

**Status: wired.**

- Mock-server tests can now simulate watcher push events
  (`crates/ff-rdp-core/tests/mock_watcher.rs`).
- Every iteration plan AC checkbox must name a live test and an asserted
  post-condition (project CLAUDE.md convention).  No iteration has been
  closed without an AC of that shape since iter-61o landed.

## Wire-level tracing + structured errors (iter-61m → wired)

**Status: wired.**

- `RUST_LOG=ff_rdp::wire=trace` prints every packet in/out.
- `RdpError` is a structured `thiserror` enum in core; CLI maps to user-facing
  JSON.  `ProtocolError` cases (`EvalNavigatedDuringEval`, `InvalidPacket`,
  `FrameTooLarge`, `BulkPacketUnsupported` after iter-61w) drive deterministic
  CLI behaviour rather than ad-hoc strings.

## Cross-references

- [[stability-roadmap]] — the iter-61m..iter-61w arc.
- [[lessons-learned]] — what we learned along the way.
- [[open-gaps]] — what remains open (and which gaps closed-in: which iter).
- [[ff-rdp-wins]] — the original "what should drive ff-rdp improvements"
  list, now annotated with which items landed.
