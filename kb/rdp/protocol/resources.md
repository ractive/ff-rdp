---
type: rdp-note
tags: [rdp, official-docs, protocol, watcher, resources]
date: 2026-05-23
title: Watcher resources model
sources:
  - https://firefox-source-docs.mozilla.org/devtools/backend/watcher-architecture.html
  - https://firefox-source-docs.mozilla.org/devtools/backend/actor-hierarchy.html
---

# Watcher resources model

The **WatcherActor** is the modern (Firefox ≥ ~84) replacement for the
old per-actor `startListeners` / `stopListeners` style. Instead of
attaching to each console / network / thread actor individually, a
client tells one Watcher *"notify me about these resource types across
all relevant targets"*. The Watcher does the cross-target plumbing.

You get one WatcherActor per descriptor via
`descriptor.getWatcher()` ([actor-hierarchy][ah]). See
[[rdp/overview/connection-lifecycle]] for where this fits in the boot
sequence.

[ah]: https://firefox-source-docs.mozilla.org/devtools/backend/actor-hierarchy.html

## Two parallel streams

A WatcherActor surfaces **two** independent observation streams:

1. **Targets** — debuggable contexts coming and going.
2. **Resources** — content events happening inside those targets.

From [watcher-architecture][wa]:

> *"The Watcher notifies about two primary things:
> 1. Target actors via `target-available-form` and `target-destroyed-form`
> 2. Resources via `resources-available-array`,
>    `resources-updated-array` and `resources-destroyed-array`"*

[wa]: https://firefox-source-docs.mozilla.org/devtools/backend/watcher-architecture.html

## Watching targets

```
→ { "to": "<watcherID>", "type": "watchTargets",
    "targetType": "frame" }
← { "from": "<watcherID>" }     # reply, once all existing notified
```

From [watcher-architecture][wa]:

> *"`WatcherActor.watchTarget(String targetType)` … only resolves after
> notifying all existing contexts, then emits `target-available-form`
> RDP events for new contexts and `target-destroyed-form` when they
> end."*

Recognised target types ([watcher-architecture][wa]):

| `targetType`      | Implementation                                |
| ----------------- | --------------------------------------------- |
| `"frame"`         | `WindowGlobalTargetActor` (per document/iframe) |
| `"worker"`        | `WorkerTargetActor`                           |
| `"service_worker"`| `WorkerTargetActor`                           |
| `"shared_worker"` | `WorkerTargetActor`                           |
| `"process"`       | `ProcessTargetActor` (Browser Toolbox only)   |

Event shape (illustrative — exact keys are the *target form*):

```json
{ "from": "<watcherID>", "type": "target-available-form",
  "target": { "actor": "<targetActorID>", "targetType": "frame",
              "url": "...", "title": "...",
              "consoleActor": "...", "threadActor": "...", ... } }
```

Cache the entire target form — target-scoped actor IDs live inside.

## Watching resources

```
→ { "to": "<watcherID>", "type": "watchResources",
    "resourceTypes": ["console-message","source","network-event"] }
← { "from": "<watcherID>" }   # reply, once all existing flushed
```

Per [watcher-architecture][wa]:

> *"Resources don't return from `watchResources` directly. Instead,
> three RDP event types notify clients:
> - `resources-available-array`: New or existing resources
> - `resources-updated-array`: Resource modifications (stylesheets,
>   network events)
> - `resources-destroyed-array`: Resource removal"*

Each event carries an `array` field — a batch of resource objects
(*"simple JSON objects describing a particular part of the Web"*,
[actor-hierarchy][ah]). Batching is for throughput: don't assume one
event per logical thing.

## Common resource types

Non-exhaustive; full list in `devtools/server/actors/resources/` in
mozilla-central:

- `console-message` — `console.log/warn/error` from the page
- `error-message` — uncaught JS errors / CSS warnings
- `source` — JS source registered with the debugger
- `thread-state` — pause/resume notifications
- `network-event` / `network-event-stacktrace` — HTTP requests
- `stylesheet` — stylesheets loaded / changed
- `cookie`, `local-storage`, `session-storage`, `indexed-db`,
  `cache-storage` — storage inspectors
- `platform-message` — chrome-level logs
- `document-event` — DOMContentLoaded / load / etc.
- `reflow` — layout reflow events
- `websocket`, `server-sent-event` — push channels

## Where the watcher *actually* runs

From [watcher-architecture][wa]:

> *"ResourceWatcher location varies by type:
> - **ParentProcessResources**: Instantiated in parent process
> - **FrameTargetResources**: Per-frame target on target's main thread
> - **ProcessTargetResources**: Per-process target on main thread
> - **WorkerTargetResources**: Per-worker target on worker thread"*

State the parent process wants every watcher to see lives in the
WatcherActor's **session data**: *"a JSON-serializable object that is
meant to be shared across all processes and threads"*
([watcher-architecture][wa]). Modifications happen only in the parent
process; the JS Process Actor IPC propagates them.

For a client this is transparent — all resource events still come out
of the one WatcherActor on the wire — but it explains why batching and
slight ordering surprises exist (different processes, different
threads, async IPC).

## Stopping

```
→ { "to": "<watcherID>", "type": "unwatchResources",
    "resourceTypes": ["console-message"] }
→ { "to": "<watcherID>", "type": "unwatchTargets",
    "targetType": "frame" }
```

These cancel future events for those types. In-flight events already
on the wire may still arrive — drain them.

## Mental model: async iterator, not REST

It's tempting to think of `watchResources` as a one-shot query. It
isn't — it's "open a topic subscription and also replay the existing
state". The reply just means *"replay is complete; live events
follow"*. Persist subscriptions across page navigations; the Watcher
re-binds to the new target automatically.
