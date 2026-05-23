---
name: project-daemon-architecture
description: Daemon architecture review findings — missing watchTargets, double-boundary drain bug, no Front cache, no protocol version, heavy-SPA mutex contention
metadata:
  type: project
---

ff-rdp daemon architecture review completed 2026-05-23.

**Why:** Three sessions (51/52/53) of repeated "works direct, broken in daemon mode" bugs trace to four root causes.

**How to apply:** Use these findings as the grounding for iter-61l and beyond; don't re-investigate — just fix.

## Top 4 structural problems

1. **`watchTargets("frame")` never called at daemon startup** (server.rs:176-178). The WatcherActor wiki states both `watchTargets` and `watchResources` are required. The daemon calls only `watchResources`. This may be why the daemon's own watcher buffer stays empty during idle periods.

2. **Double-boundary bug in `store_network_events` path** (daemon/client.rs:404-434 + daemon/buffer.rs:87-100). `navigate --with-network` in daemon mode records a `tabNavigated`-triggered nav boundary, then `store_network_events` records a second boundary before appending events. The subsequent `network --since -1` resolves to the second boundary whose `start_index` equals `total_inserted` at boundary time — past all events — so the drain returns nothing. This is AC-C in sessions 51/52/53.

3. **No protocol version in `DaemonInfo`** (registry.rs:29-44). Binary upgrades that change IPC message shapes are not detected; the stale daemon silently misbehaves.

4. **Heavy-SPA mutex contention causes auth-greeting timeout** (server.rs `accept_loop` + `firefox_reader_loop`). Under heavy event bursts the reader thread holds `stream_subs` lock while the accept loop needs it to send the auth greeting. CLI sees "daemon did not respond within the timeout after auth" (session-53 N2).

## Secondary structural gaps
- No `target-available-form` / `target-destroyed-form` handler → stale `target_actor` after cross-process nav.
- No per-actor request queue (DevToolsClient has one) → would break under concurrent CLI RPC clients.
- No reconnect after Firefox restart.

## Recommendation
Keep the daemon; evolve it toward Firefox's DevToolsClient model in phases:
1. Add `watchTargets("frame")` (one line, highest payoff).
2. Fix double-boundary: `handle_store_events` must not call `record_nav_boundary`; rely on the `tabNavigated`-triggered boundary.
3. Add `DaemonInfo.protocol_version`.
4. Replace direct `rpc_writer` writes from `firefox_reader_loop` with an `mpsc` channel to eliminate mutex-contention auth timeout.

Review document: `kb/research/ff-rdp-daemon-review.md`
