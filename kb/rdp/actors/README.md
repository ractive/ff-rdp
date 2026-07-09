---
type: rdp-note
tags:
  - rdp
  - firefox-server
  - actor-index
date: 2026-05-23
firefox_files:
  - devtools/server/actors/
title: Firefox DevTools Server Actors — Index
---

# Firefox DevTools Server Actors — Index

All paths are inside the firefox-source checkout at `/Users/james/devel/firefox/`.
Specs live in parallel under `devtools/shared/specs/`.

## Root / Descriptors / Targets / Watcher (the "spine")

- [[root]] — `actors/root.js` — entry point, `sayHello`, `listTabs`, `getProcess`, `listWorkers`.
- [[watcher]] — `actors/watcher.js` — backbone for `watchTargets` / `watchResources`. Lives under TabDescriptor / ProcessDescriptor.
- [[rdp/actors/descriptors/tab-descriptor]] — `actors/descriptors/tab.js` — `getTarget`, `getWatcher`, `navigateTo`, `reloadDescriptor`, `goBack/goForward`, `getFavicon`.
- [[rdp/actors/descriptors/process-descriptor]] — `actors/descriptors/process.js` — content/parent process descriptor; spawns ContentProcessTarget.
- [[rdp/actors/descriptors/worker-descriptor]] — `actors/descriptors/worker.js`.
- [[rdp/actors/targets/window-global-target]] — `actors/targets/window-global.js` — per-WindowGlobal target (top-level frames). 2055 lines. Hosts most child actors.
- [[rdp/actors/targets/content-process-target]] — `actors/targets/content-process.js`.
- [[rdp/actors/targets/parent-process-target]] — `actors/targets/parent-process.js` — Browser Toolbox.
- `actors/targets/base-target-actor.js` — shared base.
- `actors/targets/worker.js` — worker target.

## Web Console / JS Eval

- [[console]] — `actors/webconsole.js` (1683 lines) — `evaluateJSAsync`, `autocomplete`, `getCachedMessages`, `startListeners`. Emits `evaluationResult`, `consoleAPICall`, `pageError`.
- `actors/webconsole/eval-with-debugger.js` — actual eval impl.
- `actors/webconsole/commands/` — built-in console commands (`:screenshot`, `$`, etc).

## Network

- [[rdp/actors/network-event|network-event]] — `actors/network-monitor/network-event-actor.js` — per-request actor, getRequestHeaders / getResponseContent / getSecurityInfo. Emits `network-event-update:*` events.
- [[network-content]] — `actors/network-monitor/network-content.js` — per-target, `sendHTTPRequest`, `getStackTrace`.
- [[network-parent]] — `actors/network-monitor/network-parent.js` — parent-process side, throttling/blocking config (`setNetworkThrottling`, `setBlockedUrls`).

## Screenshot (CRITICAL for ff-rdp)

- [[screenshot]] — `actors/screenshot.js` (25 lines!) — parent-process actor on RootActor; single `capture(args)` method that calls `captureScreenshot()` util.
- [[screenshot-content]] — `actors/screenshot-content.js` (144 lines) — content-process actor; `prepareCapture({fullpage, selector, nodeActorID})` returns the rect to render.
- `actors/utils/capture-screenshot.js` — uses `browsingContext.currentWindowGlobal.drawSnapshot(rect, ratio, "rgb(255,255,255)", fullpage)`.

## Inspector / DOM / CSS

- [[walker]] — `actors/inspector/walker.js` (2906 lines!) — DOM walker, `querySelector`, `getRootNode`, mutation observer, picker.
- `actors/inspector/inspector.js` — entry; spawns the Walker, PageStyle, HighlighterActor.
- [[page-style]] — `actors/page-style.js` (1712 lines) — `getComputed`, `getApplied`, `getLayout`, `getAllUsedFontFaces`.
- `actors/style-rule.js`, `actors/style-sheets.js`, `actors/stylesheets/` — CSS rules/sheets.
- `actors/inspector/node.js`, `actors/inspector/node-picker.js`, `actors/inspector/event-collector.js`, `actors/inspector/document-walker.js`, `actors/inspector/custom-element-watcher.js`.
- `actors/inspector/css-logic.js`.
- `actors/changes.js` — CSS change tracking.
- `actors/layout.js` — grid/flex layouts.
- `actors/reflow.js` — reflow observer.
- `actors/highlighters.js`, `actors/highlighters/` — overlay highlights.

## Accessibility / Performance / Other Tools

- [[rdp/actors/accessibility|accessibility]] — `actors/accessibility/accessibility.js` — entry; spawns AccessibilityWalker.
- [[performance]] — `actors/perf.js` — Gecko Profiler control: `startProfiler`, `stopProfilerAndGetProfile`, `isActive`, `getSupportedFeatures`.
- `actors/memory.js` — heap-snapshot, GC.
- `actors/animation.js`, `actors/animation-type-longhand.js` — Web Animations API.
- `actors/compatibility/` — MDN browser-compat data.
- `actors/tracer.js`, `actors/tracer/` — JS execution tracer.
- `actors/thread.js` — JS debugger (Debugger API wrapper).
- `actors/source.js`, `actors/frame.js`, `actors/breakpoint.js`, `actors/breakpoint-list.js`, `actors/pause-scoped.js`, `actors/environment.js`, `actors/blackboxing.js`.
- `actors/object.js`, `actors/object/`, `actors/objects-manager.js`, `actors/array-buffer.js`, `actors/string.js` (LongStringActor).

## Configuration / Misc

- `actors/target-configuration.js` — per-target overrides (CSS color scheme, print sim, cache disabled, viewport). See [[target-configuration]] — the `ff-rdp emulate` command (iter-103).
- `actors/thread-configuration.js` — pause-on-exception etc.
- `actors/device.js` — user agent, screen size, GeckoView info.
- `actors/preference.js` — Services.prefs access.
- `actors/heap-snapshot-file.js` — file transfer for heap snapshots.
- `actors/manifest.js` — Web App Manifest.
- `actors/css-properties.js` — list known CSS properties.
- `actors/process.js` — legacy process actor.
- `actors/webbrowser.js` — legacy hooks for live tab/addon lists used by RootActor.
- `actors/emulation/` — touch/responsive emulation.
- `actors/addon/`, `actors/worker/` — addon and worker support.
- `actors/events/` — devtools internal events.

## Watcher infrastructure

- `actors/watcher/ParentProcessWatcherRegistry.sys.mjs` — singleton registry; spawns JSProcessActors `DevToolsProcess`.
- `actors/watcher/browsing-context-helpers.sys.mjs` — `getAllBrowsingContextsForContext`.
- `actors/watcher/session-context.js` — `SESSION_TYPES = { ALL, BROWSER_ELEMENT, WEBEXTENSION, WORKER, CONTENT_PROCESS }`.
- `actors/watcher/SessionDataHelpers.sys.mjs`.

## Resource Watchers (per type)

See [[rdp/resources/README|resources/]] for one-file-per-resource breakdown.
