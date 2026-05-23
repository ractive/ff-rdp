---
type: rdp-note
tags:
  - rdp
  - firefox-client
  - flow
  - watcher
date: 2026-05-23
firefox_files:
  - devtools/shared/specs/watcher.js
  - devtools/shared/specs/root.js
  - devtools/server/actors/watcher.js
  - devtools/client/fronts/watcher.js
title: "Flow: Watch resources"
---

# Flow: Watch resources (network events, console messages, ...)

The watcher API is the modern replacement for the per-actor
`startListeners`/`stopListeners` pattern that the WebConsole exposed. One
subscription, cross-target, cross-frame, with batched events.

## Two flavours of watcher

Resources can be watched in two scopes:

- **Per-target** via `WatcherFront`
  (got from `descriptor.getWatcher()`). Scoped to one browsing context tree.
- **Root-scoped** via `RootFront.watchResources` (spec `specs/root.js:89-94`).
  Used for things that are inherently process-wide (e.g. workers).

Both expose the same `watchResources`/`unwatchResources`/`clearResources`
methods (spec `specs/watcher.js:40-59`) and emit the same three events
(`resources-available-array`, `resources-destroyed-array`,
`resources-updated-array`).

## Resource types

Common string constants the server understands:

- `"network-event"` / `"network-event-stacktrace"`
- `"console-message"`
- `"platform-message"` (browser-internal log messages)
- `"error-message"` (page errors)
- `"document-event"` (navigation lifecycle: `will-navigate`, `dom-loading`,
  `dom-interactive`, `dom-complete`)
- `"css-message"`, `"css-change"`, `"css-registered-properties"`
- `"stylesheet"`
- `"source"`, `"thread-state"`
- `"server-sent-event"`, `"websocket"`
- `"cookies"`, `"local-storage"`, `"session-storage"`, `"cache-storage"`,
  `"indexed-db"`, `"extension-storage"`
- `"reflow"`, `"jstracer-state"`, `"jstracer-trace"`, `"last-private-context-exit"`

`ff-rdp`-relevant: `network-event`, `console-message`, `document-event`,
`error-message`.

## Step-by-step

1. `const watcher = await descriptor.getWatcher();`
2. Attach event listeners *before* calling `watchResources` — there is no
   replay of events that arrive between request send and listener attach:

   ```js
   watcher.on("resources-available-array", batch => {
     for (const [resourceType, resources] of batch) {
       // resources is Array<resourceForm>
     }
   });
   ```

3. `await watcher.watchResources(["network-event", "console-message"]);`
4. Resources start streaming as `resources-available-array` events.
5. Stop with `watcher.unwatchResources([...])` (oneway — no reply).

## Event shape

The `-array` variants batch resources for throughput. Wire packet:

```json
{"from":"server1.conn0.watcher2","type":"resources-available-array",
 "array":[
   ["network-event", [ {actor,url,method,...}, {...}, ... ]],
   ["document-event", [ {name:"dom-loading", time: ...}, ... ]]
 ]}
```

The spec declares `array: Arg(0, "array:json")` so the Front re-emits it as a
JS array of `[resourceType, resources[]]` tuples.

## Ordering & timing

- Events for a single resource type arrive in causal order.
- Events across types are *not* strictly ordered — `network-event` and
  `console-message` for the same activity may interleave either way.
- The server may emit `resources-available-array` for a given resource
  *before* the `evaluateJSAsync` reply that triggered it. Race-aware clients
  buffer events keyed by resourceID and reconcile.
- `document-event` is the only reliable signal for "page navigation
  complete" — listen for `name: "dom-complete"`. This is what ff-rdp's
  `navigate` command waits on.

## Surprising bit

The same-named events `resources-available-array` exist on **both** RootFront
*and* WatcherFront. The root one fires only for root-scoped resources (worker
discovery); the watcher one fires for everything you subscribed to via that
watcher. ff-rdp must keep the two channels separate (subscribe on the right
actor) or events get attributed to the wrong scope.

## ff-rdp implementation pointers

The daemon-mode work (iter 37-38) added per-connection event broadcasting on
top of this — a single `WatcherFront` subscription fans out to multiple
CLI clients via a unix socket. The streaming pattern is straightforward
because RDP itself is already an event stream over TCP — we just need
clients to share the underlying socket.

See also: [[evaluate-js]] for the related deferred-response pattern.
