---
type: rdp-note
tags:
  - rdp
  - firefox-server
  - actor
  - target
  - critical
date: 2026-05-23
firefox_files:
  - devtools/server/actors/targets/window-global.js
  - devtools/server/actors/targets/base-target-actor.js
  - devtools/shared/specs/targets/window-global.js
title: WindowGlobalTargetActor
---

# WindowGlobalTargetActor (typeName `"windowGlobalTarget"`)

The **per-document target** — what you talk to once you `getTarget()` on a TabDescriptor. Inherited by ParentProcessTargetActor.

- Source: `devtools/server/actors/targets/window-global.js` (2055 lines).
- Spec:   `devtools/shared/specs/targets/window-global.js`.

## form / "extra actors"

The target's `form` carries actor IDs for all the per-target child actors created by this WindowGlobal:

- `consoleActor` → [[rdp/actors/console]]
- `inspectorActor` → InspectorActor (and via it: [[rdp/actors/walker]], [[rdp/actors/page-style]])
- `threadActor` → ThreadActor (debugger)
- `storageActor`, `memoryActor`, `tracerActor`, `cssPropertiesActor`, `screenshotContentActor` ([[rdp/actors/screenshot-content]]), `networkContentActor` ([[rdp/actors/network-content]]), `manifestActor`, `accessibilityActor`, `targetConfigurationActor`, …
- `webextensionInspectedWindowActor`, `objectsManagerActor`, `reflowActor`, …

These are created via `createExtraActors` from a registry; **lazy** — first access spawns them.

## Methods (spec — many are legacy)

- `detach`, `focus`.
- `goForward`, `goBack`, `reload({force})` — **legacy**. Use [[rdp/actors/descriptors/tab-descriptor]] `goBack/Forward/reloadDescriptor` instead. Kept for third-party tools (bug 1717837).
  - iter-80 Theme B: `WindowGlobalTarget::reload(transport, target, force)` accepts a `force` flag that maps to `{options: {force: true}}` in the wire packet (the Firefox spec shape — server reads `request.options.force`). `ff-rdp reload --hard` exposes this to the CLI for cache-bypassing reloads.
- `navigateTo({url})` — legacy. Use descriptor.
- `reconfigure({cacheDisabled, colorSchemeSimulation, printSimulationEnabled, restoreFocus, serviceWorkersTestingEnabled})` — **legacy** as of v87+; use target-configuration actor instead but kept for webextensions.
- `switchToFrame({windowId})` — pick a specific iframe as the active target. Returns `{message}`.
- `listFrames()` → `{frames: [{id, parentID, url, title, destroy?}]}` — the iframe tree.
- `listWorkers()` → `{workers: array:workerDescriptor}`.
- `logInPage({text, category, flags})` — emit a synthetic page error / log.

## Events

- `tabNavigated` — `{url, title, state: "start"|"stop", isFrameSwitching}`.
- `frameUpdate` — `{frames?, selected?, destroyAll?}` when iframe list changes.
- `workerListChanged`.
- `contentScrolled` — `(deltaY)`.
- `resources-available-array` / `-destroyed-array` / `-updated-array` — for **per-target** resources (the [[rdp/actors/watcher]] also emits these from the parent process).

## Lifecycle

- Constructed by `connectToFrame` (TabDescriptor path) or by `DevToolsProcess` JSWindowActor (Watcher path).
- Tied to a `docShell` and `BrowsingContext`. Survives same-origin navigations; **destroyed and replaced** on cross-origin (process switch) navigations if `isServerTargetSwitchingEnabled`.
- `tabNavigated` fires on every load.

## Gotchas

- **Same WindowGlobal may have two target actors** during a session-switching transition. Listen to `target-destroyed-form` on the watcher to know which one is canonical.
- All the "good stuff" (eval, screenshot rect, network sending, DOM) is on **child actors of this target**, not on the target itself.
- Iframes can be either top-level targets (cross-origin, in their own process) or be reached via `switchToFrame`/`listFrames` from the parent target — depends on Fission state.
- Configuration changes (cache, color-scheme, viewport) should go through [[rdp/actors/watcher]] → `getTargetConfigurationActor()`, not the legacy `reconfigure`.
