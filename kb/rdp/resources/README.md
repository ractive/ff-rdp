---
type: rdp-note
tags: [rdp, firefox-server, resources, index]
date: 2026-05-23
firefox_files:
  - devtools/server/actors/resources/
  - devtools/server/actors/resources/index.js
---

# Resource Types — Index

A **Resource** in the modern Firefox RDP is a JSON payload streamed via [[rdp/actors/watcher]] events `resources-available-array` / `resources-updated-array` / `resources-destroyed-array`.

Each resource type has a *Resource Watcher* class in `devtools/server/actors/resources/<type>.js` that the watcher instantiates **per target** (or per watcher, for root-scope resources). Each watcher exposes a `watch(targetOrWatcherActor, { onAvailable, onUpdated?, onDestroyed? })` method.

Resource type name strings (from `resources/index.js` `TYPES`):

| Constant | String | Watcher file |
|---|---|---|
| CONSOLE_MESSAGE | `console-message` | console-messages.js |
| CSS_CHANGE | `css-change` | css-changes.js |
| CSS_MESSAGE | `css-message` | css-messages.js |
| CSS_REGISTERED_PROPERTIES | `css-registered-properties` | css-registered-properties.js |
| DOCUMENT_EVENT | `document-event` | document-event.js (frame) + parent-process-document-event.js (will-navigate) |
| ERROR_MESSAGE | `error-message` | error-messages.js |
| LAST_PRIVATE_CONTEXT_EXIT | `last-private-context-exit` | last-private-context-exit.js |
| NETWORK_EVENT | `network-event` | network-events.js (parent process) |
| NETWORK_EVENT_DECODED_BODY_SIZE | `network-event-decoded-body-size` | network-events-decoded-body-size.js |
| NETWORK_EVENT_STACKTRACE | `network-event-stacktrace` | network-events-stacktraces.js |
| PLATFORM_MESSAGE | `platform-message` | platform-messages.js |
| REFLOW | `reflow` | reflow.js |
| SERVER_SENT_EVENT | `server-sent-event` | server-sent-events.js |
| SESSION_HISTORY | `session-history` | session-history.js |
| SOURCE | `source` | sources.js |
| STYLESHEET | `stylesheet` | stylesheets.js |
| THREAD_STATE | `thread-state` | thread-states.js |
| JSTRACER_TRACE | `jstracer-trace` | jstracer-trace.js |
| JSTRACER_STATE | `jstracer-state` | jstracer-state.js |
| WEBSOCKET | `websocket` | websockets.js |
| WEBTRANSPORT | `webtransport` | webtransport.js |
| CACHE_STORAGE | `Cache` | storage-cache.js |
| COOKIE | `cookies` | storage-cookie.js |
| EXTENSION_STORAGE | `extension-storage` | storage-extension.js |
| INDEXED_DB | `indexed-db` | storage-indexed-db.js |
| LOCAL_STORAGE | `local-storage` | storage-local-storage.js |
| SESSION_STORAGE | `session-storage` | storage-session-storage.js |
| EXTENSIONS_BGSCRIPT_STATUS | `extensions-backgroundscript-status` | extensions-backgroundscript-status.js |

## Scope categories (from index.js)

- **FrameTargetResources** — per BrowsingContext/WindowGlobal target. Watch class instantiated with the target actor. Most types live here.
- **ProcessTargetResources** — per content process target.
- **ParentProcessResources** — watch class instantiated with the watcher actor; observes from parent.
- **RootResources** — singletons exposed via RootActor.watchResources (e.g. `extensions-backgroundscript-status`).

`network-event` resources, despite being "per request", are watched at the **watcher level** (parent process), not per-target — that's how a single watcher can see cross-origin / cross-process requests.

## Streaming format

Each watcher pushes via `onAvailable(arrayOfResources)`. The watcher actor batches across resource types into a single `resources-available-array` event payload:

```
[
  [resourceType, [resource, resource, …]],
  [otherResourceType, [resource]],
]
```

Throttled by 100ms (see [[rdp/actors/watcher]]).

## Individual files

- [[console-message]] — `console.log/warn/error/…` plus CSS warnings.
- [[rdp/resources/network-event|network-event]] — per-request lifecycle; spawns [[rdp/actors/network-event]] actors.
- [[network-event-stacktrace]] — JS stack at request start.
- [[network-event-decoded-body-size]] — separate stream so size can update after `network-event`.
- [[document-event]] — DOM lifecycle: dom-loading, dom-interactive, dom-complete, will-navigate.
- [[css-change]] — live edits to stylesheets via devtools (track-changes).
- [[rdp/resources/css-change]] — CSS parser warnings.
- [[stylesheet]] — stylesheet add/update/destroy.
- [[reflow]] — layout reflow timing.
- [[server-sent-event]], [[websocket]], [[webtransport]] — sub-HTTP streams.
- [[source]] — JS sources for the debugger.
- [[thread-state]] — paused/resumed/breakpointHit transitions.
- [[rdp/resources/storage]], [[storage-local-storage]], [[rdp/resources/storage]], [[rdp/resources/storage]], [[rdp/resources/storage]] — storage inspector.
- [[session-history]] — browser back/forward history.
