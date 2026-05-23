---
title: "Iteration 61q: ResourceCommand-style watcher bus + full WatcherActor engagement"
type: iteration
date: 2026-05-23
status: planned
branch: iter-61q/resource-command-bus
depends_on:
  - iteration-61p-actor-registry-and-front-lifecycle
tags: [iteration, watcher, resources, bus, network, console, stability-roadmap]
---

# Iteration 61q: ResourceCommand-style watcher bus

iter-61n added `watchTargets("frame")` and fixed the double-boundary drain. This iteration goes further: every consumer of watcher data (the daemon's per-target buffers, the CLI's `network` / `console` / `storage` commands, future agents) talks to a single in-process **ResourceCommand bus** that handles subscription deduplication, ordering, throttling reconciliation, and subscriber fan-out. The pattern is lifted from Firefox's `devtools/shared/resources/ResourceCommand.js`.

Closes `--with-network` for good: `network` (no flags), `network --detail --headers`, and the future N1 regression class all read from the same data path.

## Themes

- **A — `ResourceCommand` central bus.** `subscribe(types: &[ResourceType], filter, sink)`. Internally maintains a single watcher subscription per type, throttle reconciliation per the wiki's 100ms batching, and a fan-out to all subscribers.
- **B — Typed `Resource` enum.** `enum Resource { NetworkEvent(NetworkEvent), ConsoleMessage(ConsoleMessage), DocumentEvent(DocumentEvent), CssChange(CssChange), Storage(StorageResource), ... }`. Each variant matches the [[resources/README]] catalogue.
- **C — Daemon buffer rewritten on top of the bus.** Instead of the daemon hand-rolling per-resource buckets, it's just a subscriber that retains the last N events per type with per-navigation indexing.
- **D — `network` / `console` / `storage` commands use the bus directly.** No more separate "performance-api fallback" logic that conflicts with the watcher path; fallback is a `Source::PerfApi` variant the user can request explicitly (`--source perf-api`).

## Tasks

### A. ResourceCommand
- [ ] `ff-rdp-core/src/resources/command.rs`: `pub struct ResourceCommand { watcher: WatcherFront, subscriptions: ... }` with `subscribe`/`unsubscribe`.
- [ ] Internally reconciles: many subscribers asking for `NetworkEvent` produce exactly one `watchResources(["networkEvent"])` request. Reference-counted; last unsubscribe sends `unwatchResources`.
- [ ] Throttling: events come in batched (`[[type, [r1, r2]], ...]`) — the bus unpacks once and fans out per-resource to subscribers.

### B. Typed Resource enum
- [ ] One variant per resource type listed in [[resources/README]]. Each variant carries a typed payload (e.g. `NetworkEvent { request: HttpRequest, response: HttpResponse, ... }`).
- [ ] `From<serde_json::Value>` impls per variant; mock-server uses these for `inject_watcher_resource`.

### C. Daemon buffer on the bus
- [ ] Rewrite `daemon/buffer.rs` as `struct ResourceBuffer { subscriptions: Vec<Subscription>, store: VecDeque<(NavBoundary, Resource)> }`.
- [ ] Eviction policy: last N=10000 events per type (configurable).
- [ ] Per-navigation indexing: each event tagged with the navigation boundary it belongs to, so `--since -1` is a simple range filter.

### D. Commands migrated
- [ ] `network`: reads from the bus by default; `--source perf-api` to opt into the fallback. No silent downgrade. `meta.source` reflects the actual source per the wiki.
- [ ] `console`: same shape — reads from the bus, default tail behaves like `tail -f`, `--since` honored.
- [ ] `storage`: reads from the bus for the resource types the watcher delivers; performance-API has no storage data anyway.

## Acceptance Criteria [0/8]

- [ ] `live_network_default_watcher`: `ff-rdp navigate <url> --with-network` then `ff-rdp network` returns `source: watcher` with populated `status`, `method`, `transfer_size`. (Re-greens iter-61l C.)
- [ ] `live_network_detail_headers`: `ff-rdp network --detail --headers` after `--with-network` returns real response headers per entry, `meta.source` stays `watcher`. (Closes iter-61l N1 regression.)
- [ ] `live_resource_dedupe`: two CLI invocations subscribing to `network-event` simultaneously result in exactly one `watchResources` call on the wire (assert via iter-61m's tracing).
- [ ] `live_console_tail`: `ff-rdp console --follow` streams new messages as they arrive; closing the consumer correctly unsubscribes.
- [ ] Mock-server-driven unit test: bus correctly dedupes subscribers, fans out events, and unsubscribes on last drop.
- [ ] Eviction respects configured cap; old events are evicted in arrival order.
- [ ] No regression in iter-61n's daemon ACs.
- [ ] `cargo fmt && cargo clippy --workspace --all-targets -- -D warnings && cargo test-live && cargo test --workspace -q` clean.

## Design notes

- The bus lives in `ff-rdp-core` so library consumers (future TUI, future LLM agent) can subscribe without going through the daemon IPC.
- The daemon mode is a thin shell: it owns the bus, exposes a subscribe-via-RPC surface, but the bus itself is a normal in-process abstraction usable without daemon mode.
- Don't over-model: the typed `Resource` enum doesn't need every Firefox-side field on day one. Add fields as commands need them.

## References

- [[firefox-devtools-patterns-for-ff-rdp]] §4 (Resource subscription as shared bus) — top-3 pattern
- [[watcher]], [[watch-resources]], [[resources/README]] (kb/rdp/)
- [[ff-rdp-daemon-review]] §4 (Watcher buffer architecture)
- [[ff-rdp-wins]] §3 (Watcher engagement), §5 (--headers data path)
- [[stability-roadmap]]
