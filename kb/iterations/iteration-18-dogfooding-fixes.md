---
title: "Iteration 18: Dogfooding Fixes"
status: completed
date: 2026-04-07
tags:
  - bugfix
  - dogfooding
branch: iter-18/dogfooding-fixes
---

# Iteration 18: Dogfooding Fixes

Fixes discovered during real-world dogfooding session (2026-04-07).

## Tasks

### Critical
- [x] `ff-rdp launch` fails on Firefox 149 — needs `devtools.debugger.remote-enabled` pref in user.js
  - Firefox 149 no longer honours `--start-debugger-server` without the pref
  - Add the pref to `USER_JS` in `launch.rs`
  - Also add `devtools.debugger.prompt-connection=false` and `devtools.chrome.enabled=true`

### Medium
- [x] `perf vitals` errors: "await is only valid in async functions"
  - Fix: wrapped JS in async IIFE `(async () => { ... })()`
- [x] `navigate --with-network` returns empty `network: []`
  - Root cause: `navigate_to` used `actor_request` which consumed `resources-available-array` events from the watcher while waiting for the target actor's ack
  - Fix: send `navigateTo` raw (without `actor_request`), let the drain loop collect events
  - Note: still empty when routed through daemon (daemon path uses `drain_network_from_daemon` — separate issue for a future iteration)

### Low
- [x] `eval` warning: "ownPropertyNames unrecognizedPacketType" on object results
  - Firefox 149 removed `ownPropertyNames` packet type
  - Fix: use `prototypeAndProperties` and extract keys from `own_properties`
- [x] `screenshot` error message in non-headless mode could be clearer
  - Fix: error now says `relaunch with: ff-rdp launch --headless`
