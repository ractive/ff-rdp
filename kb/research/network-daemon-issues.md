---
title: "Network Commands Daemon Mode Issues"
type: research
iteration: 37
date: 2026-04-08
status: resolved
---

# Network Commands Daemon Mode Issues

Research findings for [[iterations/iteration-37-network-daemon-fix]].

## Bug 1: `network --limit` Performance API Fallback Timeout

### Root Cause

Two issues combined to cause the timeout:

1. **`drain_daemon_events` was fragile to interleaved messages.** It read exactly
   ONE response after sending the drain request. In daemon mode, the
   `firefox_reader_loop` thread may forward Firefox messages (e.g.
   `consoleAPICall` push events) to the RPC client concurrently. If a forwarded
   message arrived before the daemon's drain response, `drain_daemon_events`
   consumed the wrong message and returned empty results.

2. **`evaluate_js_async` was vulnerable to console push events.** In daemon mode,
   `consoleAPICall` and `pageError` push events are forwarded to the RPC client.
   These events have `from` set to the console actor — the same actor that
   `evaluateJSAsync` targets. The `actor_request` function matches responses by
   `from` field only, so it could mistake a `consoleAPICall` for the eval ack.
   When this happened, `evaluate_js_async` either:
   - Failed with `InvalidPacket` (no `resultID` on the push event), or
   - Got the wrong ack, then timed out waiting for `evaluationResult`

### Fix

1. **`drain_daemon_events`** now loops (up to 64 iterations) discarding
   non-daemon messages until it finds the response with `from: "daemon"`. This
   matches the pattern used by `recv_daemon_ack` for `start_daemon_stream` and
   `stop_daemon_stream`.

2. **`evaluate_js_async`** no longer uses `actor_request`. Instead, it sends the
   request manually and reads messages in a loop that explicitly skips
   `consoleAPICall` and `pageError` events before matching on `from`. This
   applies to both the ack-reading phase and the `evaluationResult` loop.

### Key Insight

The daemon's `firefox_reader_loop` correctly routes messages:
- `is_watcher_event()` only matches `resources-available-array` /
  `resources-updated-array` from the daemon's own watcher → buffered/streamed
- `is_console_push_event()` matches `consoleAPICall` / `pageError` → forwarded
  to stream subscribers AND RPC client
- Everything else → forwarded to RPC client only

The `evaluationResult` is correctly classified as "everything else" and forwarded.
The issue was that OTHER messages (console push events) from the SAME actor
confused the client-side request/response matching.

## Bug 2: `navigate --with-network --network-timeout` Captures Too Few Events

### Root Cause

`--network-timeout` was used as a per-read **idle timeout** via
`set_network_timeout` (which sets the socket read timeout). During navigation:

1. Navigate command is sent to Firefox
2. Navigation takes 1-2 seconds before any network events start
3. A beacon or redirect fires very early (1 event captured)
4. 5-second idle gap while the page loads → timeout fires
5. Collection stops with only 1 event

The default (5000ms) worked for the `network` snapshot command because by the
time the user runs `network`, the page is already loaded and events are buffered.
But for `navigate --with-network`, events arrive in bursts with gaps during the
page load process.

### Fix

Replaced the idle timeout with a **total elapsed time limit** via a new
`drain_network_events_timed` function:

- Uses a short per-read poll interval (500ms) for responsive polling
- Keeps collecting events until the total wall-clock time exceeds the
  `--network-timeout` value
- Returns all events captured during the entire window

The `--network-timeout` help text was updated to reflect the new semantics:
"Total time limit for network event collection" (was: "Idle timeout").

The existing `drain_network_events` (idle-timeout based) is unchanged and still
used by the `network` snapshot command where idle-based detection is appropriate.

## Architecture Notes

### Daemon Event Routing (server.rs)

```
Firefox → firefox_reader_loop → is_watcher_event?
                                  ├─ YES → dispatch_watcher_event (buffer/stream)
                                  └─ NO  → is_console_push_event?
                                             ├─ YES → dispatch to stream subs + RPC client
                                             └─ NO  → forward_to_rpc_client only
```

### Network Event Collection Strategies

| Strategy | Function | Used By |
|----------|----------|---------|
| Idle timeout | `drain_network_events` | `network` command (snapshot) |
| Total time | `drain_network_events_timed` | `navigate --with-network` |
| Continuous | `network_follow_loop` | `network --follow` |
| Daemon buffer | `drain_network_from_daemon` | `network` command (daemon mode) |
