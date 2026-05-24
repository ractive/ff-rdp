---
title: "Iteration 74: Protocol correctness — oneway methods, event loss, registry lifecycle"
type: iteration
date: 2026-05-24
status: planned
branch: iter-74/protocol-correctness-oneway-events-lifecycle
depends_on:
  - iteration-72-transport-polish
  - iteration-73-spec-fidelity-gates
firefox_refs:
  - path: devtools/shared/specs/watcher.js
    lines: "23-62"
    why: "Canonical list of watcher-actor oneway methods: unwatchTargets, unwatchResources, clearResources."
  - path: devtools/shared/specs/root.js
    lines: "96-110"
    why: "Root-actor oneway methods: unwatchResources, clearResources."
  - path: devtools/shared/specs/reflow.js
    lines: "30-33"
    why: "Reflow actor's start/stop both marked oneway."
  - path: devtools/shared/specs/walker.js
    lines: "378-381"
    why: "walker.clearPicker is oneway. (releaseNode at 127-133 is response-less but NOT oneway; correctly remains an actor_request.)"
  - path: devtools/server/actors/watcher.js
    lines: "405-460"
    why: "target-destroyed-form emission sites — both the explicit emit and the WatcherActor destroy path."
  - path: devtools/shared/specs/watcher.js
    lines: "100-113"
    why: "target-destroyed-form event declaration on the watcher spec — packet shape the registry must consume."
  - path: devtools/shared/transport/transport.js
    lines: "40-56"
    why: "Confirms typed packets and bulk packets share a transport; demuxing must not drop sibling-actor packets."
  - path: devtools/server/actors/webconsole.js
    lines: "761-870"
    why: "evaluateJSAsync emits intermediate consoleAPICall packets on the same console actor before the final evaluationResult — sequence we must not drop in recv_event_from."
kb_refs:
  - kb/rdp/actors/watcher.md
  - kb/rdp/protocol/message-format.md
  - kb/rdp/from-our-codebase/open-gaps.md
first_call_sites:
  - primitive: "ff_rdp_core::transport::Transport::actor_send_oneway"
    site: "crates/ff-rdp-core/src/actors/watcher.rs (unwatchResources, unwatchTargets, clearResources call sites)"
  - primitive: "ff_rdp_core::actors::watcher::WatcherEvent::TargetDestroyed"
    site: "crates/ff-rdp-core/src/registry.rs (Registry::on_watcher_event handler)"
  - primitive: "ff_rdp_core::registry::Registry::invalidate_target"
    site: "crates/ff-rdp-core/src/actors/watcher.rs (called from the target-destroyed-form dispatch path)"
dogfood_path: |
  # 1. Oneway no-hang: unwatchResources returns immediately, not after socket timeout.
  ff-rdp --log-rdp-trace daemon start &
  ff-rdp daemon subscribe console-message
  ff-rdp daemon unsubscribe console-message
  # Expected in trace: unwatchResources sent, NO awaited reply, latency < 100ms.

  # 2. Cross-actor packet not dropped: evaluateJSAsync delivers its intermediate
  # consoleAPICall to the resource bus even though the eval is still pending.
  ff-rdp --log-rdp-trace eval --subscribe console 'console.log("ping"); 1+1'
  grep '"ping"' ~/.cache/ff-rdp/rdp-trace.log   # must appear

  # 3. Registry auto-invalidates on navigation.
  ff-rdp navigate https://example.com
  ff-rdp navigate https://example.org
  ff-rdp inspector list-actors --jq '.registry.target_count'
  # Expected: stale target actors from example.com are gone.
tags: [iteration, protocol, correctness]
---

The protocol review surfaced four correlated bugs in
`crates/ff-rdp-core/src/transport.rs` and the watcher dispatch path
(W2, C3, E2, L1). All four come from one missing invariant: the
transport should never *consume and discard* a packet just because it
wasn't the one we were waiting for. Firefox treats the channel as a
multiplexed firehose; `recv_reply_from` and `recv_event_from` treat it
as a synchronous RPC and silently drop everything else. The same root
cause shows up at the protocol layer (W2: we await replies to methods
the spec declares `oneway: true`, so we time out instead of returning)
and at the registry layer (L1: we ignore `target-destroyed-form`
events, so the registry hands out actor IDs the server has already
killed).

This iter fixes all four without rewriting the daemon's concurrency
model — that work is iter-76. Here we keep the existing
single-threaded send/recv shape and just route the dropped packets to
the event sink that already exists.

## Themes

- **A — Oneway conformance.** Add `Transport::actor_send_oneway`;
  route every spec-declared oneway call through it; gate it with a
  new xtask `check-oneway-conformance`.
- **B — Stop dropping sibling-actor packets in `recv_reply_from`.**
  Forward unmatched typed packets to the resource/event sink before
  continuing the read loop.
- **C — Stop dropping non-matching events in `recv_event_from`.**
  Same fix, but for the event-predicate path used by evaluateJSAsync.
- **D — Registry lifecycle on `target-destroyed-form`.** Dispatch the
  watcher event to `Registry::invalidate_target`.

## Tasks

### A. Oneway conformance
- [ ] Add `pub fn actor_send_oneway(&mut self, to: &ActorId, type_: &str, body: Value) -> Result<()>` to `crates/ff-rdp-core/src/transport.rs` (writes the packet, does NOT block on a reply).
- [ ] Audit every call site that currently invokes `actor_request` for a method the firefox spec marks `oneway: true`. Per `firefox_refs` above, the full list is:
  - `watcher.unwatchTargets` — `devtools/shared/specs/watcher.js:23-32`
  - `watcher.unwatchResources` — `devtools/shared/specs/watcher.js:50-55`
  - `watcher.clearResources` — `devtools/shared/specs/watcher.js:57-62`
  - root `unwatchResources` — `devtools/shared/specs/root.js:96-105`
  - root `clearResources` — `devtools/shared/specs/root.js:106-110`
  - `reflow.start`, `reflow.stop` — `devtools/shared/specs/reflow.js:30-33`
  - `walker.clearPicker` — `devtools/shared/specs/walker.js:378-381`
- [ ] Re-route each call site through `actor_send_oneway`. `walker.releaseNode` (`devtools/shared/specs/walker.js:127-133`) is response-less but NOT marked oneway — it stays an `actor_request`. Note this distinction in the relevant kb files.
- [ ] Add `tools/xtask/src/check_oneway_conformance.rs`: greps `$FF_RDP_FIREFOX_PATH/devtools/shared/specs/*.js` for `oneway: true`, builds the method-name set, then greps `crates/ff-rdp-core/src/` for `actor_request(... "<method>" ...)` calls intersecting that set. Exit 1 on any match. Wire as a CI gate.

### B. `recv_reply_from` must not drop sibling packets
- [ ] In `crates/ff-rdp-core/src/transport.rs:579-609`, when a typed packet arrives whose `from:` doesn't match the awaited actor, forward it to the existing event/resource sink (the same path the daemon already uses for unsolicited events) instead of discarding.
- [ ] Tests: `recv_reply_from_forwards_sibling_packet` — inject a packet sequence `[other_actor: event, awaited_actor: reply]`, assert the reply returns AND the event sink received the other-actor packet.

### C. `recv_event_from` must not drop non-matching events
- [ ] Same fix at `crates/ff-rdp-core/src/transport.rs:625-655`: a packet from the target actor that doesn't match the predicate must still go to the sink (e.g. `consoleAPICall` arriving between evaluateJSAsync ack and `evaluationResult`).
- [ ] Tests: `recv_event_from_forwards_non_matching` — simulate the evaluateJSAsync sequence per `devtools/server/actors/webconsole.js:761-870`; assert the intermediate `consoleAPICall` reaches the sink.

### D. Registry invalidation on target-destroyed-form
- [ ] Add `WatcherEvent::TargetDestroyed { target_actor: ActorId, options: Value }` to `crates/ff-rdp-core/src/actors/watcher.rs:113-138`.
- [ ] Parse `target-destroyed-form` packets (shape per `devtools/shared/specs/watcher.js:100-113`) in the watcher's event dispatch.
- [ ] In `crates/ff-rdp-core/src/registry.rs`, add `pub fn invalidate_target(&mut self, target_actor: &ActorId)` that removes any entries whose target actor matches (including dependent fronts: inspector, walker, console scoped to that target).
- [ ] Hook the watcher event dispatch to call `Registry::invalidate_target` when handling `WatcherEvent::TargetDestroyed`.

## Acceptance Criteria [0/8]

- [ ] `live_watcher_oneway_unwatch_no_hang`: `crates/ff-rdp-cli/tests/live_oneway.rs::live_watcher_oneway_unwatch_no_hang` — calls `unwatchResources(["console-message"])`, asserts return latency < 100ms (current code times out at the socket-read deadline ≈ 30s). Gated `FF_RDP_LIVE_TESTS=1`.
- [ ] `check_oneway_conformance_catches_regression`: `tools/xtask/tests/check_oneway_conformance.rs::check_oneway_conformance_catches_regression` — synthetic source file calling `actor_request("unwatchResources", …)` exits 1.
- [ ] `recv_reply_from_forwards_sibling_packet`: `crates/ff-rdp-core/src/transport.rs::recv_reply_from_forwards_sibling_packet` — unit test asserts the sibling-actor event hits the sink AND the awaited reply still resolves.
- [ ] `live_cross_actor_packet_not_lost`: `crates/ff-rdp-cli/tests/live_cross_actor.rs::live_cross_actor_packet_not_lost` — subscribe to `console-message`, fire `console.log("ping")` and a paused-debugger ping on a different actor concurrently, assert both deliveries arrive. Gated `FF_RDP_LIVE_TESTS=1`.
- [ ] `recv_event_from_forwards_non_matching`: `crates/ff-rdp-core/src/transport.rs::recv_event_from_forwards_non_matching` — unit test for the evaluateJSAsync-intermediate-packet scenario.
- [ ] `live_target_destroyed_invalidates_registry`: `crates/ff-rdp-cli/tests/live_target_destroyed.rs::live_target_destroyed_invalidates_registry` — navigate twice; assert no stale target actor IDs remain in the registry after the second navigation. Gated `FF_RDP_LIVE_TESTS=1`.
- [ ] `registry_invalidate_target_removes_dependents`: `crates/ff-rdp-core/src/registry.rs::registry_invalidate_target_removes_dependents` — unit test, dependent inspector + walker entries removed alongside the target.
- [ ] `cargo fmt && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace -q` clean.

## Design notes

`actor_send_oneway` is a different *function*, not a flag on
`actor_request`, deliberately: callers should not silently decide
whether to wait — the spec decides. The new xtask
`check-oneway-conformance` makes the spec the source of truth so a
future refactor that switches a `oneway:` marker on or off in Firefox
will surface here.

The sibling-packet forwarding in §B/C reuses the resource bus from
iter-61q rather than spinning up a parallel sink. The bus already
fans out typed events to subscribers; the bug was the transport
deciding which packets ever reach the bus. After this fix, the
transport's invariant is simple: every packet read off the wire is
either returned to the awaiter or forwarded to the bus. None are
discarded.

Registry invalidation only fires on `target-destroyed-form` from the
watcher. Other deletion paths (target detach without watcher
subscription, descriptor close) are out of scope; the watcher is the
sole authoritative source per `devtools/server/actors/watcher.js`.

## Out of scope

- Per-actor pipelining (iter-76 theme C).
- Bulk packet receive (iter-76 theme A).
- Long-string / object grip release (iter-76 theme B).
- Rewriting `recv_reply_from` to be async — the synchronous CLI path
  is fine; this iter only fixes the packet-dropping bug.

## References

- [[iteration-72-transport-polish]]
- [[iteration-73-spec-fidelity-gates]]
- Protocol review report (2026-05-24) §1 (W2), §2.2 (C3, E2), §2.4 (L1)
- `kb/rdp/protocol/message-format.md` §"Request / reply pairing"
- `kb/rdp/from-our-codebase/open-gaps.md` (oneway, registry-lifecycle entries)
