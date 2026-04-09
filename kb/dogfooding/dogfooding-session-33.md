---
title: "Dogfooding Session 33"
type: dogfooding
date: 2026-04-08
firefox_version: 149
mode: daemon
port: 6000
site: comparis.ch
binary: ./target/release/ff-rdp
status: complete
tags: [dogfooding, testing, regression, daemon]
---

# Dogfooding Session 33

Post iterations 34-37 regression test of the 5 commands that failed in [[dogfooding/dogfooding-session-32|session 32]].

## Results

| # | Command | Session 32 | Session 33 | Status |
|---|---------|-----------|------------|--------|
| 1 | `cookies` | Actor crash (TypeError: sessionString undefined) | **3 cookies returned** | **FIXED** (iter 34) |
| 2 | `screenshot --output` (daemon) | Timeout | Still times out via daemon; works with `--no-daemon` | **NOT FIXED** |
| 3 | `console --follow` | Empty output | **3 lines captured** (log, warn, error) | **FIXED** (iter 36) |
| 4 | `network --limit 20` (daemon) | 0 results, perf-api timeout | **5 requests returned** | **FIXED** (iter 37) |
| 5 | `navigate --with-network --network-timeout 5` | Only 1 request (beacon) | 1 request with 5s timeout; **195 requests with 8s timeout** | **FIXED** (iter 37) |

## Analysis

### Screenshot via daemon (still broken)
- Works perfectly in direct mode (`--no-daemon`): 698KB PNG saved
- Fails via daemon: `screenshotActor.capture failed (operation timed out)`
- Root cause: daemon's `handle_client()` has hardcoded 500ms client read timeout; screenshot's two-step protocol takes >500ms
- Tracked in [[iterations/iteration-38-daemon-client-timeout|iteration 38]]

### Navigate --with-network timeout
- The fix in iter 37 changed from idle-timeout to wall-clock-time semantics — this works correctly
- The default `--network-timeout 5000` (5s) is just too short for heavy pages like comparis.ch (needs ~8s)
- With `--network-timeout 8000`: 195 requests captured via daemon, 215 via direct

## Summary

- **4 of 5 commands fixed** (cookies, console --follow, network --limit, navigate --with-network)
- **1 command still broken** in daemon mode only (screenshot)
- No regressions observed in previously working commands
