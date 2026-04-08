---
title: "Iteration 37: Fix Network Commands in Daemon Mode"
type: iteration
status: completed
date: 2026-04-08
tags:
  - iteration
  - bugfix
  - network
  - daemon
  - performance-api
  - research
branch: iter-37/network-daemon-fix
---

# Iteration 37: Fix Network Commands in Daemon Mode

Two network-related commands are broken in daemon mode:
1. `network --limit 20` — Performance API fallback times out
2. `navigate --with-network --network-timeout 5` — captures only 1 request

Note: `network --follow` works perfectly in daemon mode (390 events captured).

## Error 1: `network --limit 20`

```
performance-api fallback eval failed: operation timed out
```

Returns 0 results. The Performance API fallback (which evaluates
`performance.getEntriesByType('resource')` via JS) times out in daemon mode.

### Current code flow (`network.rs`)

1. `drain_network_from_daemon("network-event")` → returns 0 events (buffer empty)
2. Falls back to `performance_api_fallback()` (`network_events.rs:164-215`)
3. Evaluates JS via `WebConsoleActor::evaluate_js_async()` → **TIMEOUT**

### Why it might timeout in daemon mode
The JS evaluation goes through the daemon proxy. The daemon may be intercepting
or delaying the `evaluateJSAsync` response. Or the console actor's `from` field
doesn't match what the daemon expects, causing the response to be misrouted.

## Error 2: `navigate --with-network --network-timeout 5`

Captures only 1 request (a beacon) instead of dozens when navigating to a page.
Without `--network-timeout`, it captures 161 requests (but takes 17+ seconds).

### Current code flow (`navigate.rs`)

In daemon mode:
1. `start_daemon_stream("network-event")` → clear buffer, start forwarding
2. Send `navigateTo` to browser
3. Set socket timeout to 5000ms (`set_network_timeout`)
4. `drain_network_events()` — reads events until timeout fires
5. `stop_daemon_stream_draining()` — stop stream, collect in-flight events
6. `drain_network_from_daemon()` — get any remaining buffered events

### Why only 1 request with 5s timeout
The 5-second idle timeout fires too early. The page navigation itself may take
1-2 seconds, and network requests come in bursts. A single 5-second window
might catch only the tail end. The daemon's event forwarding may also have
latency — events buffered in the daemon thread may not be forwarded before the
timeout fires.

The real issue may be that `drain_network_events()` uses a **per-read timeout**
(5s between reads) rather than a **total timeout** (5s total). If the first
few events arrive quickly but then there's a 5s gap, it times out even though
more requests will follow.

## Research Tasks

**IMPORTANT: Do thorough protocol research before attempting any code fix.**

### For `network --limit` (Performance API timeout)

- [x] **Debug the eval timeout.** Add temporary logging to trace where the
  evaluation gets stuck in daemon mode:
  1. Does `evaluateJSAsync` send the request to Firefox?
  2. Does Firefox respond?
  3. Does the daemon forward the response to the client?
  4. Is the response misrouted (daemon buffers it as a watcher event)?

- [x] **Test eval directly in daemon mode.** Run `ff-rdp eval 'JSON.stringify(
  performance.getEntriesByType("resource").length)'` — does this work? If eval
  itself works but the fallback eval doesn't, the issue is in how
  `performance_api_fallback` sends/receives the eval.

- [x] **Compare daemon vs direct eval path.** The `performance_api_fallback`
  function uses `WebConsoleActor::evaluate_js_async()`. In daemon mode, this
  goes through the proxy. Check if the daemon's `firefox_reader_loop` is
  intercepting the eval response because it looks like a watcher event.

- [x] **Check if `is_watcher_event()` misclassifies eval responses.** The
  daemon checks incoming Firefox messages with `is_watcher_event()`. If an
  eval response accidentally matches this check, it gets buffered instead of
  forwarded to the client.

### For `navigate --with-network --network-timeout 5`

- [x] **Instrument the event timing.** Add `eprintln!` timestamps to
  `drain_network_events()` to see when events arrive:
  - First event timestamp
  - Last event timestamp
  - Total events per second
  - When the timeout fires

- [x] **Test different timeout strategies.** The current approach uses the
  socket read timeout as an idle timeout. Consider:
  - Total elapsed time instead of per-read idle
  - Shorter idle timeout (1s) with a minimum total time (5s)
  - Wait for `DOMContentLoaded` or `load` event before starting the idle timer
  - Count-based: stop after N consecutive seconds with no new requests

- [x] **Check daemon event forwarding latency.** The daemon's
  `dispatch_watcher_event()` forwards events via a channel. Is there batching
  or buffering in the channel that delays delivery?

- [x] **Compare with `--follow` which works.** `network --follow` captures
  390 events. What's different about the streaming path vs the
  `navigate --with-network` drain path?

### General

- [x] **Document findings** in `kb/research/network-daemon-issues.md`

## Implementation

- [x] Fix Performance API fallback in daemon mode (network --limit)
- [x] Fix navigate --with-network --network-timeout to capture more requests
- [x] Verify `network --follow` still works after changes
- [x] Fix `total_transfer_bytes: -0.0` if not already fixed
- [x] Update test fixtures

## Test Fixtures

All e2e test fixtures must be recorded from a real Firefox instance — never hand-craft them.
Run with `FF_RDP_LIVE_TESTS_RECORD=1 cargo test -p ff-rdp-core --test live_record_fixtures -- --ignored` to record fixtures.
