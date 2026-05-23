---
title: Actors ff-rdp Talks To
type: rdp-note
tags: [rdp, from-codebase]
date: 2026-05-23
---

# Actors `ff-rdp` Talks To

Inventory of every Firefox RDP actor surfaced by `ff-rdp-core` as of 2026-05-23. Source files are under `crates/ff-rdp-core/src/actors/`. The kb deep-dive on protocol shapes is in [[firefox-rdp-protocol]] and [[rdp-protocol-deep-dive]]; this file is the *map* of what we wire up vs. what we ignore.

## Discovery / root tier

### `root` (fixed ID `"root"`)
- Code: `crates/ff-rdp-core/src/actors/root.rs`
- Methods used: `listTabs`, `getRoot`, `listProcesses`.
- `listTabs` filtering: Firefox interleaves `tabListChanged` push events (have a `type` field) with the actual reply (no `type` field). `RootActor::list_tabs` loops until it finds a packet `from == "root"` with no `type` (root.rs:32-73). Earlier iter-54 replaced an old retry hack with this canonical filter.
- `getRoot` is used to grab the `screenshotActor` ID (Firefox 87+; see `screenshot.rs:24-37`).
- `listProcesses` parses each `{actor, isParent}` strictly (root.rs:100-128). PR #73 / CodeRabbit feedback now rejects malformed entries rather than silently dropping them. See [[lessons-learned#strict-parsing]].

### Tab descriptor (`server*.conn*.tabDescriptor*`)
- Code: `crates/ff-rdp-core/src/actors/tab.rs`
- Methods used: `getTarget`, `getWatcher` (`TabActor::get_target` / `get_watcher`).
- `getTarget` wraps everything in a `"frame"` object on tab descriptors and in a `"process"` object on **process** descriptors (`parse_target_response` vs `parse_process_target_response`, tab.rs:115-120, 83-89). This split surprised us — see [[lessons-learned#descriptor-wrappers]].
- Returned IDs we capture into `TargetInfo`: `actor` (WindowGlobalTarget), `consoleActor`, `threadActor`, `inspectorActor`, `screenshotContentActor`, `accessibilityActor`, `responsiveActor`, plus `browsingContextID` (required for the Firefox 149+ two-step screenshot protocol).

## Target / navigation tier

### `WindowGlobalTargetActor`
- Code: `crates/ff-rdp-core/src/actors/target.rs` (whole file ~54 lines).
- Methods used: `navigateTo`, `reload`, `goBack`, `goForward`.
- *Not used*: `detach`, `listFrames`, `focus`. iframe enumeration is currently done via the walker / DOM eval instead.
- Edge case: `navigate` to the *current* URL used to hang for 5 s until iter-61i added a same-URL short-circuit. See [[dogfooding-session-49]] / [[dogfooding-session-51]].

### `WatcherActor`
- Code: `crates/ff-rdp-core/src/actors/watcher.rs` (983 lines incl. parsers + tests).
- Methods used: `watchResources`, `unwatchResources`, `watchTargets`, `unwatchTargets`.
- Resource types we subscribe to: `"network-event"`, `"console-message"`, `"error-message"`, `"cookies"` (cookies via storage flow, see below).
- Event types we parse:
  - `resources-available-array` → `parse_network_resources`, `parse_console_resources` (note: the wire shape is `array: [["network-event", [items...]]]` — a *list of [type, items] pairs*, not a flat object).
  - `resources-updated-array` → `parse_network_resource_updates` (status, mimeType, totalTime, contentSize, transferredSize, fromCache, remoteAddress, securityState).
  - `target-available-form` / `target-destroyed-form` → `parse_target_event` (extracts `targetType`, `isTopLevelTarget`).
- *Status*: works end-to-end *inside* `navigate --with-network`, but a subsequent standalone `network` call still falls back to the performance-api path. See [[open-gaps#with-network-fallthrough]].
- *Methods we don't use yet*: `getNetworkParentActor`, `getTargetConfigurationActor`, `getThreadConfigurationActor`.

## Per-target inspection tier

### `WebConsoleActor`
- Code: `crates/ff-rdp-core/src/actors/console.rs` (969 lines, biggest actor file).
- Methods used: `startListeners` (`PageError`, `ConsoleAPI`), `getCachedMessages`, `evaluateJSAsync` (both default and `chromeContext: true` variants).
- Reply correlation: `evaluateJSAsync` returns an *immediate* ack (no `type`, contains `resultID`) and then an asynchronous `evaluationResult` push event with the matching `resultID`. We use the *no-type-field-means-reply* invariant to skip pushes (console.rs:129-151, 224-243). See [[lessons-learned#reply-vs-event]].
- Navigation-during-eval: if a `tabNavigated`/`willNavigate` event from the *same* console actor arrives while we wait for the result, we return `ProtocolError::EvalNavigatedDuringEval` instead of hanging until the socket timeout (console.rs:175-179, iter-54 task 3).
- Chrome-context eval: same wire path with `"chromeContext": true` to bypass page CSP (`evaluate_js_async_chrome`, console.rs:210-275). Used as a fallback when an eval hits `EvalError`/`CSP`. **Currently broken in practice** — the retry doesn't fire in the daemon path on HN/lit.dev. See [[open-gaps#csp-eval-fallback]] and [[dogfooding-session-53]] AC-H.
- Push-event parsing: `parse_console_notification` handles direct `consoleAPICall` / `pageError` pushes that arrive *outside* the watcher resources channel after `startListeners` (Firefox 149+ behaviour).
- Legacy `startListeners` is still wired alongside the modern Watcher path (iter-54 task 6 deferred).

### `InspectorActor` / `WalkerActor`
- Code: `crates/ff-rdp-core/src/actors/inspector.rs` (131 lines), `dom_walker.rs` (553 lines).
- Methods used: `getWalker`, `documentElement`, `querySelector`, `querySelectorAll`, walker tree traversal for `snapshot`.
- Quirk: Firefox sends node `attrs` as a *flat alternating string array* `["name", "value", ...]`, not `Vec<{name,value}>`. We custom-parse via `parse_dom_node` (dom_walker.rs:20-47).
- iter-61k added `hasShadowRoot` / `shadowMode` flagging on host nodes (verified in [[dogfooding-session-53]] AC-I). Shadow-DOM *piercing* is not yet implemented.

### `ResponsiveActor`
- Code: `crates/ff-rdp-core/src/actors/responsive.rs` (72 lines).
- Methods used: `toggleTouchSimulator` only.
- *Cannot use*: `setViewportSize` — see the `project_viewport_protocol` memory note. The actor never exposed it; viewport sizing in DevTools RDM uses `synchronouslyUpdateRemoteBrowserDimensions` on the browser chrome layer, which is unreachable from RDP. We simulate viewport via CSS width constraints instead.

### `ScreenshotActor` / `ScreenshotContentActor`
- Code: `screenshot.rs` (340 lines), `screenshot_content.rs` (361 lines).
- Two-step protocol (Firefox 149+):
  1. `screenshotContentActor.prepareCapture` → returns `{ window_dpr, window_zoom, rect }`.
  2. `screenshotActor.capture` with `{ browsingContextID, fullpage, dpr, snapshotScale, rect }` → returns `data:image/png;base64,...`.
- The actor ID comes from `getRoot().screenshotActor` not from the target.
- *Broken*: `--full-page` returns viewport-sized PNG (800×600 on a 22 491-px tall page) — five dogfooding sessions running. See [[open-gaps#full-page-screenshot]] and [[dogfooding-session-48]]/[[dogfooding-session-53]].

### `StorageActor`
- Code: `crates/ff-rdp-core/src/actors/storage.rs` (491 lines).
- Flow: `tab.getWatcher` → `watcher.watchResources(["cookies"])` → capture `cookies` resource (actor ID + `resourceId` + `hosts`) → call `getStoreObjects` with `{host, resourceId, options.sessionString}` per host → `unwatchResources` at end.
- Firefox 149 changes (documented inline storage.rs:43-60): `host` field is no longer for routing; `resourceId` from the watch response is mandatory.

### `AccessibilityActor`
- Code: `accessibility.rs` (450 lines). Backs the `a11y` and `a11y contrast` commands.

### `ThreadActor`
- Code: `thread.rs` (241 lines). Used minimally — `attach`/`detach` and source listing.
- Caveat from iter-54 task 2: ThreadActor's `attach` *legitimately* returns a packet with `type: "paused"`, which is why a blanket "skip-all-packets-with-type" filter was rejected as the universal correlation strategy. Per-actor filters instead.

### `PageStyleActor`
- Code: `page_style.rs` (515 lines). Powers the `computed` command. iter-61k made the output shape consistent (always `[{computed, index, selector}]`) — see [[dogfooding-session-53]] AC-E.

### `NetworkEventActor`
- Code: `network.rs` (300 lines).
- Per-request fetch methods used: `getRequestHeaders`, `getResponseHeaders`, `getResponseContent`.
- iter-54 task 5: response bodies arrive as either plain string OR `{type:"longString", actor, initial, length}` grips for large bodies. We fetch via `LongStringActor::full_string` and cap at `MAX_FRAME_BYTES` (64 MiB), surfacing truncation via `ResponseContent.truncated`.

### `LongStringActor`
- Code: `string.rs` (101 lines). Chunked `substring` reads for `longString` grip resolution.

### `ObjectActor`
- Code: `object.rs` (485 lines). Provides `release` for grip cleanup (iter-54 task 4). `ScopedGrip` wrapper exists but is not yet wired into daemon-mode call sites — actor leaks in long-running daemons remain a real risk.

### `DeviceActor`
- Code: `device.rs` (326 lines). Used by `doctor` for Firefox version detection.

## Actors we *don't* use

- `PreferenceActor` — `intl.locale.*` and similar prefs are currently set via `user.js` at launch, not via RDP. (Why locale pin failed in iter-61k — see [[lessons-learned#locale]].)
- `Source` actor — we use `eval`-based source enumeration as a fallback for `sources` command (iter-61g).
- `Worker` target actors — listed but never attached.
- `WebExtensionDescriptor`, `ParentProcessDescriptor` (except via `listProcesses` enumeration).
- `PerfActor` (root-level performance profiling) — vitals are computed via `Performance` API eval, not RDP perf.
