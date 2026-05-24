---
title: iter-71 Theme C — Legacy startListeners vs. Watcher coexistence
date: 2026-05-24
tags: [research, console, watcher, legacy-api, iter-71]
---

# Hypothesis

`commands/console.rs` calls `WebConsoleActor::start_listeners` (legacy path) AND the
daemon path uses `ResourceCommand::subscribe` for `console-message`.  Running both paths
in the same session *may* cause Firefox to push each `consoleAPICall` event twice — once
via the legacy console actor push and once via the watcher resources stream.

# Test

`live_console_no_double_delivery` in
`crates/ff-rdp-cli/tests/live_console_no_double_delivery.rs` (iter-71 Theme C AC).

Setup:
1. Headless Firefox launched via `ff-rdp launch --headless`.
2. Watcher actor obtained via `TabActor::get_watcher`.
3. `WatcherActor::watch_targets` called for `"frame"`.
4. `ResourceCommand::subscribe` called for `ConsoleMessage` (sends `watchResources` wire call).
5. `WebConsoleActor::start_listeners` called for `["ConsoleAPI"]`.
6. `evaluateJSAsync` sent to console actor: `console.log('<sentinel>')`.
7. Transport drained for 500 ms; all packets routed through `bus.dispatch_event`.

# Finding

**No double-delivery occurs.**

On the tested Firefox version, `console.log` events triggered via `evaluateJSAsync` arrive
**only** through the legacy `consoleActor` push path:

- Packet 1: `evaluateJSAsync` reply (no `type` field) from `consoleActor`
- Packet 2: `type=consoleAPICall` from `consoleActor` (legacy push)
- Packet 3: `type=evaluationResult` from `consoleActor`

**Zero** `resources-available-array` packets arrived from the watcher actor during the 500 ms
window.  The watcher `ResourceCommand` bus received 0 `ConsoleMessage` resources.

# Interpretation

Firefox does **not** route `evaluateJSAsync`-triggered `console.log` calls through the watcher
`resources-available-array` stream in this context (headless, blank tab, no page navigation).
The watcher resource subscription for `console-message` exists on the wire but Firefox does not
push events through it for synchronous eval calls.

This means:
- **No double-delivery is possible** via the legacy `startListeners` + watcher subscription combination, at least for eval-triggered console calls.
- The legacy `startListeners` call in `commands/console.rs` is not causing watcher bus pollution.
- Removing `startListeners` would eliminate the legacy push path (packets #1–#3 above) without affecting the watcher bus.

# Implication for iter-71c

A future iteration (iter-71c) may safely remove the `startListeners` call from `console.rs`
without risk of breaking the watcher delivery path — because the watcher path for eval-triggered
console messages is not active in this scenario anyway.

Whether the watcher path activates after a real page navigation (which triggers `target-available-form`
events and possibly binds a new thread actor) is an open question for iter-71c to explore.

# Test result

```
live_console_no_double_delivery: total=0, matching sentinel=0
live_console_no_double_delivery: watcher_bus_count=0 (0=legacy-only, 1=watcher-delivered, >1=double-delivery)
live_console_no_double_delivery: PASS (no double delivery detected)
```

Status: **PASS** — no double-delivery detected.
