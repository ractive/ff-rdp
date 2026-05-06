---
title: "Iteration 49: Scroll & Reload Fixes"
type: iteration
date: 2026-04-23
status: completed
branch: iter-49/scroll-reload-fixes
tags:
  - iteration
  - bugfix
  - scroll
  - reload
  - daemon
---

# Iteration 49: Scroll & Reload Fixes

Fix the two remaining bugs from [[dogfooding/dogfooding-session-39]]: scroll returning Promise grips, and reload --wait-idle always reporting 0 requests in daemon mode. Also fixed 4 pre-existing Rust 1.95 clippy lints caught by CI.

## Tasks

### 1. Fix scroll commands returning Promise actor grip [2/2]

Iter-47 wrapped scroll JS in `return new Promise(resolve => { requestAnimationFrame(() => resolve(...)) })` to get post-scroll viewport position. But Firefox's `evaluateJSAsync` returns immediately without waiting for Promise resolution — so the Promise object grip was returned instead of the resolved viewport data.

- [x] Remove Promise + requestAnimationFrame wrappers from all 4 scroll functions (`run_to`, `run_by`, `run_scroll_absolute`, `run_container`). Read viewport position synchronously after scroll calls.
- [x] Verify all scroll commands (`top`, `bottom`, `by`, `to`) report correct viewport position

### 2. Fix `reload --wait-idle` in daemon mode [2/2]

The daemon intercepts watcher events and buffers them — they never reach the reload client's `recv()` loop. The reload command didn't check `ctx.via_daemon` and always used the direct `watch_resources` path, which only works without the daemon.

- [x] Add daemon-aware branching: use `start_daemon_stream("network-event")` / `stop_daemon_stream_draining` in daemon mode (same pattern as `navigate --with-network`). Keep existing `watch_resources` path for direct mode.
- [x] Extract shared drain logic into `drain_idle_events()`, `count_network_events()`, `emit_reload_result()`

### 3. Fix Rust 1.95 clippy lints [3/3]

Pre-existing issues caught by CI's newer clippy:

- [x] `launch.rs`: `Result::map().unwrap_or()` → `map_or()`
- [x] `daemon/server.rs`: `Duration::from_millis(30_000)` → `Duration::from_secs(30)`
- [x] `e2e/network.rs`: `Result::map_or(false, ...)` → `is_ok_and()`

## Acceptance Criteria

- [x] `scroll bottom` reports non-zero `viewport.y` (was Promise grip)
- [x] `scroll top` reports `viewport.y: 0` (was stale)
- [x] `reload --wait-idle` reports `requests_observed > 0` in daemon mode (was 0)
- [x] All quality gates pass: `cargo fmt`, `cargo clippy`, `cargo test`
- [x] CI passes (fmt, clippy, Linux, macOS, Windows)
