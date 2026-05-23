---
type: rdp-note
tags: [rdp, official-docs, overview, actors]
date: 2026-05-23
title: The actor model
sources:
  - https://firefox-source-docs.mozilla.org/devtools/backend/protocol.html
  - https://firefox-source-docs.mozilla.org/devtools/backend/actor-hierarchy.html
  - https://firefox-source-docs.mozilla.org/devtools/backend/protocol.js.html
---

# The actor model

Everything addressable on an RDP server is an **actor**. An actor is a
server-side object with:

- a unique string **ID** (e.g. `"root"`, `"server1.conn0.tabDescriptor3"`),
- a **type** (`"rootActor"`, `"tabDescriptor"`, `"watcher"`, ...),
- a set of **request types** it handles,
- optionally, **events** it spontaneously emits, and
- a **lifetime** bounded by its parent in the actor tree.

## Actor IDs

From the spec: *"Actor names are JSON strings, containing no spaces or
colons."* ([protocol][proto]). They are opaque — clients **must not**
parse them. In practice, IDs produced by Firefox look like:

```
root
server1.conn0
server1.conn0.tabDescriptor3
server1.conn0.windowGlobal42
server1.conn0.child1/consoleActor7
```

The `serverN.connM` prefix encodes which DevToolsServer instance and
which client connection a given actor belongs to, but **this is an
implementation detail** — treat IDs as opaque strings, look them up by
asking the parent actor.

[proto]: https://firefox-source-docs.mozilla.org/devtools/backend/protocol.html

## The root actor

The root actor *"is always named `"root"`"* ([protocol][proto]) and is
the only ID a client may hard-code. It hands out all **descriptor
actors** and a few **global actors** (preference, device, perf — see
[actor-hierarchy][ah]). See [[connection-lifecycle]].

[ah]: https://firefox-source-docs.mozilla.org/devtools/backend/actor-hierarchy.html

## Parent → child lifecycles

From [actor-hierarchy][ah]:

> *"once a parent is removed from the pool, its children are removed as
> well."*

Consequences for clients:

- If a tab descriptor goes away (tab closed), every actor underneath it
  (target, console, thread, inspector, ...) becomes invalid in one
  shot. You will see `noSuchActor` errors if you try to use them.
- Closing the transport disposes the root actor → cleans up everything.
- You generally **don't** explicitly destroy individual actors; you
  destroy the *thing* (stop watching, detach, close the tab) and the
  framework cascades.

This design is inherited from Mozilla IPDL but specialised for
debugging ([protocol][proto]).

## Four flavours of actor

From the docs ([actor-hierarchy][ah], [watcher-architecture][wa]):

1. **Root actor** — singleton per connection, ID `"root"`.
2. **Descriptor actors** — handles for things you can debug:
   `TabDescriptorActor`, `WorkerDescriptorActor`,
   `ParentProcessDescriptorActor`, `WebExtensionDescriptorActor`. Each
   exposes `getWatcher()`.
3. **WatcherActor** — per-descriptor observer; produces *target* and
   *resource* notifications. See [[watcher-actor]] (slice TBD) and
   [[../protocol/resources]].
4. **Target actors** — short-lived, per-context: `WindowGlobalTargetActor`
   (one per document/iframe), `WorkerTargetActor`, `ProcessTargetActor`.
   They hold *target-scoped* children: `WebConsoleActor`,
   `InspectorActor`, `ThreadActor`, etc. — *"Created lazily upon first
   request for performance optimization."* ([actor-hierarchy][ah]).

[wa]: https://firefox-source-docs.mozilla.org/devtools/backend/watcher-architecture.html

## Specs vs wire

If you're reading protocol.js source, an actor is defined by a
**spec** with typed `Arg(...)` inputs and a `RetVal(...)` output. On
the wire, that translates to:

- Request: `{ "to": "<actorID>", "type": "<methodName>", ...args }`
- Reply:   `{ "from": "<actorID>", ...returnValues }`

The framework *"will add the 'type' and 'to' request properties."*
([protocol.js][pjs]). A bare client just writes the JSON itself.

[pjs]: https://firefox-source-docs.mozilla.org/devtools/backend/protocol.js.html

## Events ("notifies")

Events flow one-way (server → client) and look like ordinary server
packets — `{ "from": "<actorID>", "type": "<eventName>", ... }` — but
are unsolicited. They are not paired with any request and clients must
demux replies vs events by *"reading the `from` field and matching
against pending requests, treating anything else as an event."*
(implied by [protocol][proto] §request/reply/notify). See
[[../protocol/message-format]].
