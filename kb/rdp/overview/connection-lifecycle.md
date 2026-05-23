---
type: rdp-note
tags: [rdp, official-docs, overview, lifecycle]
date: 2026-05-23
title: Connection lifecycle
sources:
  - https://firefox-source-docs.mozilla.org/devtools/backend/protocol.html
  - https://firefox-source-docs.mozilla.org/devtools/backend/client-api.html
  - https://firefox-source-docs.mozilla.org/devtools/backend/actor-hierarchy.html
---

# Connection lifecycle

This page describes what a client does from "TCP `connect()` succeeded"
through "I'm watching a tab's console messages" through "clean
shutdown".

## 1. Start the server (inside Firefox)

The browser must opt-in. From [client-api][ca]:

> ```
> DevToolsServer.init();
> DevToolsServer.registerAllActors();
> ```

In practice you do this by launching Firefox with
`--start-debugger-server PORT` (and `devtools.debugger.remote-enabled`
+ `devtools.chrome.enabled` prefs flipped). The server then listens on
that TCP port.

[ca]: https://firefox-source-docs.mozilla.org/devtools/backend/client-api.html

## 2. Open the transport and read the greeting

The client opens a TCP socket (or nsIPipe). As soon as the connection
is up, the server *unprompted* sends one packet from the root actor —
the **greeting**. Conceptually:

```
{ "from": "root",
  "applicationType": "browser",
  "traits": { ... }, ... }
```

The exact field set is defined by the RootActor implementation, not by
the protocol spec. Treat unknown fields as opaque. See
[[rdp/protocol/transport|transport]] for how the bytes are framed.

The framework's client-side `connect()` callback fires with
`(type, traits)` once this greeting arrives ([client-api][ca]):

> `"client.connect((type, traits) => { ... })"`

## 3. Discover what to debug

The client asks the root actor for a **descriptor**. From
[client-api][ca]:

> *"Get the list of tabs to find the one to attach to.
> `client.mainRoot.listTabs().then(tabs => { ... })`"*

Common root requests (see [[rdp/protocol/message-format]] for shape):

| Request type        | Returns                              |
| ------------------- | ------------------------------------ |
| `listTabs`          | array of tab descriptor forms        |
| `getTab`            | one tab descriptor (by `outerWindowID`, `browserId`, ...) |
| `getProcess`        | a process descriptor                 |
| `getMainProcess`    | the parent-process descriptor (chrome-debug) |
| `listAddons`        | WebExtension descriptors             |
| `listWorkers`       | service/shared worker descriptors    |
| `listProcesses`     | all process descriptors              |

Each returns one or more **descriptor forms** — JSON dicts with the
actor IDs you need next. See [[actor-model]] for the descriptor
flavours.

## 4. Get a Watcher

Modern flow (Firefox ≥ ~84) skips the legacy `attach` step on the
descriptor itself. Instead, *"Each descriptor exposes a `getWatcher()`
method returning a dedicated WatcherActor."* ([actor-hierarchy][ah]).

```
→ { "to": "<tabDescriptorID>", "type": "getWatcher" }
← { "from": "<tabDescriptorID>", "actor": "<watcherID>", "traits": {...} }
```

[ah]: https://firefox-source-docs.mozilla.org/devtools/backend/actor-hierarchy.html

## 5. Watch targets

Ask the Watcher which target types you care about. From
[watcher-architecture][wa]:

> `"WatcherActor.watchTarget(String targetType)"` — *"only resolves
> after notifying all existing contexts, then emits
> `target-available-form` RDP events for new contexts and
> `target-destroyed-form` when they end."*

Typical target types: `"frame"`, `"worker"`, `"service_worker"`,
`"shared_worker"`, `"process"` ([watcher-architecture][wa]).

Each `target-available-form` event carries a **target form** with
actor IDs for the target-scoped actors (`consoleActor`, `threadActor`,
`inspectorActor`, ...). Cache them per target.

[wa]: https://firefox-source-docs.mozilla.org/devtools/backend/watcher-architecture.html

## 6. Watch resources

Independently, ask for the *content* you actually want:

```
→ { "to": "<watcherID>",
    "type": "watchResources",
    "resourceTypes": ["console-message", "source", "network-event"] }
```

The server then streams `resources-available-array`,
`resources-updated-array`, `resources-destroyed-array` events from the
Watcher. Details in [[rdp/protocol/resources]].

## 7. (Optional) Attach the thread

For pause/resume/breakpoint debugging on a target ([client-api][ca]):

> `"const threadFront = await targetFront.attachThread()"`
> `"threadFront.on('paused', onPause)"`
> `"threadFront.on('resumed', fooListener)"`

Wire-level: `{ to: <threadActorID>, type: "attach" }` then `resume`,
`interrupt`, etc. Receive `paused` / `resumed` events.

## 8. Detach / navigate

Before debugging a different tab: *"Detach from the previous tab.
`await targetFront.detach()`. Start debugging the new tab."*
([client-api][ca]).

Closing a tab in the browser destroys its descriptor and cascades to
all children (see [[actor-model]] §lifecycles); a client should treat
`target-destroyed-form` as authoritative.

## 9. Shutdown

`client.close()` ([client-api][ca]) tears down the transport; the
server destroys the root actor and everything under it.
