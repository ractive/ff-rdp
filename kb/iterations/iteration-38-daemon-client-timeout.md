---
title: "Iteration 38: Fix Daemon Client Timeout + Network Timeout UX"
type: iteration
status: completed
date: 2026-04-08
branch: iter-38/daemon-client-timeout
tags:
  - iteration
  - bugfix
  - daemon
  - screenshot
  - timeout
  - network
  - ux
---

# Iteration 38: Fix Daemon Client Timeout + Network Timeout UX

## Problem

`screenshot` works with `--no-daemon` but times out via daemon:

```
error: screenshot: screenshotActor.capture failed (operation timed out)
```

**Root cause:** The daemon's `handle_client()` in `server.rs` sets a **hardcoded 500ms read timeout** on the client TCP stream (line ~567). Screenshot's two-step protocol (`prepareCapture` → `capture`) takes >500ms on Firefox 149, so the client socket times out before the daemon relays Firefox's response.

Direct mode uses the CLI's `--timeout` (default 5000ms), giving Firefox enough time.

## Context

### Current timeout chain (daemon mode)
```
CLI --timeout 5000ms   (unused — only affects direct Firefox connection)
     ↓
Client → Daemon TCP    read timeout: 500ms (hardcoded in handle_client)
     ↓
Daemon → Firefox TCP   read timeout: 1000ms (for shutdown-flag polling)
```

### Why 500ms was chosen
The 500ms timeout is used as a poll interval so the client handler can check for shutdown. It's not meant as a command timeout — it's a read-loop cadence. But when a forwarded Firefox response takes >500ms, the client sees a timeout error.

### Affected commands
- `screenshot` (two-step capture takes >500ms)
- Potentially any command that triggers a slow Firefox response

## Research Tasks [0/2]

- [x] Confirm the exact read-timeout line in `daemon/server.rs` and how `handle_client` uses it
- [x] Check if the CLI's `--timeout` value is available in the daemon client protocol (it's passed in args but may not reach the daemon handler)

## Implementation [0/4]

- [x] **Increase daemon client read timeout** from 500ms to match CLI semantics. Options (pick simplest that works):
  - **Option A (preferred):** Increase the client read timeout to a generous value (e.g. 30s) since the daemon handler already has a shutdown flag check. The 500ms was overkill for a poll cadence — 30s still allows shutdown detection on the next iteration.
  - **Option B:** Have the CLI send its `--timeout` value in the daemon greeting/first message so the daemon can set the read timeout per-client. More complex but more precise.
- [x] Add a daemon-parity e2e test for `screenshot` that uses the mock server (similar to existing `daemon_navigate_with_network_captures_requests` test)
- [x] Ensure the `USERPROFILE` env var fix (already on main) is included in any new daemon test helpers
- [ ] Verify `screenshot` works via daemon in live dogfooding after the fix

## Part B: Network Timeout UX Hints [0/5]

When `navigate --with-network` or `network` hits the timeout while events are still arriving, the user (or LLM) gets suspiciously few results with no indication that increasing the timeout would help.

### B1: Increase default `--network-timeout` [0/1]

- [x] Bump the default `--network-timeout` from 5000ms to 10000ms. Real-world pages like comparis.ch need ~8s to capture most requests. 10s is a safer default; users can always reduce it for faster results.

### B2: Add `timeout_reached` boolean to network output [0/2]

- [x] In `drain_network_events_timed()`, track whether events were still arriving when the deadline fired (i.e. the last `transport.recv()` before the deadline returned an event, not a timeout). Return this flag alongside the collected events.
- [x] Include `"timeout_reached": true/false` in the JSON output of both `network` and `navigate --with-network` (inside the `network` object). This lets LLMs and scripts programmatically detect truncated results.

### B3: Add human-readable hint when timeout was reached [0/2]

- [x] When `timeout_reached` is true, add a `"hint"` field to the network JSON output:
  ```json
  "hint": "Network collection was still receiving events when the timeout was reached. Consider increasing --network-timeout for more complete results."
  ```
- [x] Only emit the hint when `timeout_reached` is true — no noise for pages with few requests that finish before the deadline.

## Test Fixtures

If adding a screenshot daemon parity test, record the fixture from a real Firefox instance using the existing `live_record_fixtures.rs` framework.

## Acceptance Criteria

### Part A: Daemon timeout
- [ ] `ff-rdp --port 6000 screenshot --output /tmp/test.png` works via daemon (currently fails)
- [x] All existing daemon parity tests still pass
- [x] No regression in direct-mode screenshot
- [ ] Windows CI passes (daemon tests use both HOME and USERPROFILE)

### Part B: Network timeout UX
- [x] Default `--network-timeout` is 10000ms
- [x] `navigate --with-network` output includes `"timeout_reached": true` when deadline was hit while events were still flowing
- [x] `navigate --with-network` output includes `"hint"` string when `timeout_reached` is true
- [x] No hint/timeout_reached when collection finishes naturally before the deadline
