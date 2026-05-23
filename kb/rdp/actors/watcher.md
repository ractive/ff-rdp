---
type: rdp-note
tags:
  - rdp
  - firefox-server
  - actor
  - watcher
  - critical
date: 2026-05-23
firefox_files:
  - devtools/server/actors/watcher.js
  - devtools/shared/specs/watcher.js
  - devtools/server/actors/watcher/ParentProcessWatcherRegistry.sys.mjs
  - devtools/server/actors/watcher/session-context.js
  - devtools/server/actors/resources/index.js
title: WatcherActor
---

# WatcherActor (typeName `"watcher"`)

The backbone of modern devtools. **All resource streaming** (network events, console messages, document-events, css changes, …) flows through this actor.

- Source: `devtools/server/actors/watcher.js` (952 lines).
- Spec: `devtools/shared/specs/watcher.js`.

Obtained via `TabDescriptorActor.getWatcher({ isServerTargetSwitchingEnabled, isPopupDebuggingEnabled })` (or `ProcessDescriptor.getWatcher`). The descriptor calls `new WatcherActor(conn, sessionContext)` and `manage()`s it.

## Session Context

A WatcherActor is bound to a `sessionContext` (`actors/watcher/session-context.js`):

```
SESSION_TYPES = { ALL, BROWSER_ELEMENT, WEBEXTENSION, WORKER, CONTENT_PROCESS }
```

For `ff-rdp` the relevant type is `BROWSER_ELEMENT` — bound to one `<browser>` element identified by `browserId`. Stored in `this._browserElement`.

## Methods

| Method | Args | Returns | Behavior |
|---|---|---|---|
| `watchTargets` | `targetType: string` | `{}` | Start observing one target type. Spawns existing matching targets and emits `target-available-form` for each, then for any new ones. Target types: `"frame"` (WindowGlobal), `"process"` (ContentProcessTarget), `"worker"`, `"service_worker"`, `"shared_worker"`. |
| `unwatchTargets` | `targetType, options?` | oneway | Stop observing. |
| `watchResources` | `resourceTypes: array:string` | `{}` | Subscribe to one or more resource types. Causes IPC to relevant content processes via `DevToolsProcess` JSProcessActor. Existing resources are emitted, then live. |
| `unwatchResources` | `resourceTypes` | oneway | |
| `clearResources` | `resourceTypes` | oneway | Drops accumulated network-event / console-message caches. |
| `getParentBrowsingContextID` | `browsingContextID` | `nullable:number` | |
| `getNetworkParentActor` | — | `networkParent` | Throttling/blocking/persistence config (parent-process). |
| `getBlackboxingActor` | — | `blackboxing` | |
| `getBreakpointListActor` | — | `breakpoint-list` | |
| `getTargetConfigurationActor` | — | `target-configuration` | Cache disable, viewport CSS, color-scheme sim. |
| `getThreadConfigurationActor` | — | `thread-configuration` | Pause-on-exception, etc. |

## Events

The 5 events ff-rdp must handle:

- `target-available-form` — `(targetForm)` — new target. `targetForm.actor` is the target actorID, `.targetType` is `"frame" | "process" | …`. Use this to wire your console/inspector against the target.
- `target-destroyed-form` — `(targetForm, options?)`.
- `resources-available-array` — `(array)` where each entry is `[resourceType, resourcesArray]`. **Throttled by 100 ms** (see `RESOURCES_THROTTLING_DELAY`, line 65).
- `resources-updated-array` — same shape, partial deltas (e.g. network-event-update fields).
- `resources-destroyed-array` — same shape, for resources that go away (rare).

The throttle batches `available/updated/destroyed` queues into `#throttledResources`, flushed via `throttle(this.emitResources, 100)`.

## Lifecycle / IPC

- On `destroy()`: iterates `ChromeUtils.getAllDOMProcesses()` and IPCs `destroyWatcher({watcherActorID})` on every `DevToolsProcess` JSProcessActor — fan-out cleanup across all content processes.
- The Browser Toolbox session uses `BrowserToolboxDevToolsProcess` instead, to live in a distinct compartment so it can debug system code.

## Resource types (from `actors/resources/index.js` `TYPES`)

```
console-message, css-change, css-message, css-registered-properties, document-event, error-message,
last-private-context-exit, network-event, network-event-decoded-body-size, network-event-stacktrace,
platform-message, reflow, server-sent-event, session-history, source, stylesheet, thread-state,
jstracer-trace, jstracer-state, websocket, webtransport,
Cache, cookies, extension-storage, indexed-db, local-storage, session-storage,
extensions-backgroundscript-status
```

See [[rdp/resources/README|resources/]] for each.

## Gotchas for ff-rdp

- **Iframe-before-top-level race**: bfcache navigations can deliver iframe targets before the top target. `_earlyIframeTargets` caches them until the top arrives (see comment block ~L123).
- **Throttle delay** means a tiny burst of network events can be batched into one `resources-available-array` packet — your handler must iterate.
- A WatcherActor will not see anything until you `watchTargets("frame")` AND `watchResources([...])`. Resources alone get nothing.
- `getNetworkParentActor()` must be the path to set throttling — the per-event NetworkEventActor only reads, never writes.
- The registry lives in `ParentProcessWatcherRegistry.sys.mjs` (singleton, `global: "shared"`) — devtools can only have one logical view of the watcher set per process tree.
