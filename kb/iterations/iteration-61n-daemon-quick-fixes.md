---
title: "Iteration 61n: Daemon quick-fixes (watchTargets + double-boundary + mpsc isolation + protocol version)"
type: iteration
date: 2026-05-23
status: in-progress
branch: iter-61n/daemon-quick-fixes
depends_on:
  - iteration-61m-wire-tracing-and-structured-errors
tags:
  - iteration
  - daemon
  - watcher
  - network
  - stability-roadmap
---

# Iteration 61n: Daemon quick-fixes

Four small, surgical fixes to the daemon. Each is well under 20 LOC; together they close the regression clusters that drove sessions 51–53 AC-C, the heavy-SPA `daemon did not respond within the timeout after auth` (session-53 N2), and the "buffer_sizes:{}" weirdness in `daemon status`.

Diagnosis came out of [[ff-rdp-daemon-review]]. With wire tracing from iter-61m we'll see these problems on the wire as they're fixed.

## Themes

- **A — Add `watchTargets("frame")` to the daemon's WatcherActor engagement.** Currently `daemon/server.rs:177` calls only `watchResources([...])`. Per the [[watcher]] wiki page, both are required.
- **B — Fix the double-boundary drain.** `daemon/client.rs:404-434` records a second navigation boundary inside `store_network_events` *before* appending events; `network --since -1` then resolves to a `start_index` that's past the events. Fix: never record a boundary inside `store_*`; rely on the `tabNavigated` boundary that was already recorded.
- **C — mpsc isolation between Firefox reader and accept-loop auth handshake.** Reader thread (`daemon/server.rs:552-576`) shares a lock with the auth-greeting writer; heavy event bursts delay the greeting past `cli.timeout`. Fix: reader thread pushes into a `tokio::sync::mpsc` channel; a single dispatcher task forwards to the RPC writer and to per-subscriber streams.
- **D — Daemon ↔ CLI protocol version byte.** Add a `protocol_version: 1` field to the daemon-mode handshake; CLI bails clearly when versions mismatch. Future-proofs the rest of the roadmap.

## Tasks

### A. watchTargets engagement
- [x] In `crates/ff-rdp-core/src/daemon/server.rs`, after the watcher is created, call `watch_targets("frame")` before `watch_resources(...)`.
- [x] Subscribe to `target-available-form` and `target-destroyed-form` events; log them at `tracing::info!` level for now (full Front-invalidation cascade is iter-61p).
- [x] Live test in `tests/live_daemon_watch_targets.rs`: navigate same tab to two cross-origin pages; assert at least one `target-available-form` event was received and the daemon's `target_count` counter incremented.

### B. Boundary fix
- [x] In `daemon/buffer.rs` and `daemon/client.rs`, remove the implicit boundary insertion inside `store_network_events`.
- [x] Verify the navigation boundary is recorded exactly once per `tabNavigated`.
- [x] Live test in `tests/live_network_default_watcher.rs` (the same one iter-61l deferred): `ff-rdp navigate <url> --with-network` then `ff-rdp network` (no flags) returns `source: watcher` with non-null `status` and `method` for at least one entry. Without this fix it returns `source: performance-api`.

### C. Reader/dispatcher decoupling
- [x] Introduce a single `tokio::sync::mpsc::Sender<DaemonInboundEvent>` owned by the dispatcher task. Reader thread pushes; dispatcher fans out to per-subscriber `broadcast` channels and writes the rpc reply stream.
- [x] Auth handshake writes go through the dispatcher so they never contend with reader-thread writes.
- [x] Live test in `tests/live_daemon_heavy_spa.rs`: launch FF, navigate to a synthetic page that fires 200 XHRs in a tight loop, assert the daemon accepts a new CLI connection (auth completes within 2 s) during the burst.

### D. Protocol version
- [x] Add `protocol_version: u32` to the daemon greeting and the CLI's first request.
- [x] If mismatch, CLI exits with `error_type: "daemon_version_mismatch"` and a clear message.
- [x] Snapshot test for the mismatch case.

## Acceptance Criteria [0/8]

- [ ] **A.** Live test `live_daemon_watch_targets` passes: `target-available-form` arrives after a navigation.
- [ ] **B.** Live test `live_network_default_watcher` passes: `network` (no flags) returns `source: watcher` with populated status/method after `navigate --with-network`.
- [ ] **B.** `daemon status` shows non-zero `buffer_sizes.network-event` after a navigation that loaded ≥1 subresource.
- [ ] **C.** Live test `live_daemon_heavy_spa` passes: new CLI connection completes auth in ≤2 s during a 200-XHR burst.
- [x] **D.** Daemon greeting carries `protocol_version`; mismatch produces `error_type: "daemon_version_mismatch"`.
- [x] No regression in iter-61j/61k/61l ACs that are currently green.
- [x] `cargo fmt && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace -q` clean.
- [ ] PR description includes the live-test names and their asserted outputs.

## References

- [[ff-rdp-daemon-review]] — the diagnosis driving all four fixes
- [[watcher]], [[watch-resources]] — protocol requirements
- [[ff-rdp-wins]] §3 (Watcher engagement), §5 (--with-network → --headers)
- [[stability-roadmap]]
