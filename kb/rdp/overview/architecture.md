---
type: rdp-note
tags: [rdp, official-docs, overview]
date: 2026-05-23
title: RDP architecture overview
sources:
  - https://firefox-source-docs.mozilla.org/devtools/backend/protocol.html
  - https://firefox-source-docs.mozilla.org/devtools/backend/actor-hierarchy.html
  - https://firefox-source-docs.mozilla.org/devtools/backend/watcher-architecture.html
  - https://firefox-source-docs.mozilla.org/devtools/backend/client-api.html
---

# RDP architecture overview

The **Remote Debugging Protocol (RDP)** is Mozilla's JSON-over-stream
protocol that DevTools (and other tools) use to talk to a Firefox
instance — locally or over the network. It is described by Mozilla as
*"The foundation for DevTools communication with Firefox"*
([devtools index][idx]).

[idx]: https://firefox-source-docs.mozilla.org/devtools/index.html

## Where it lives

- The **server** lives inside Firefox (`DevToolsServer`). A consumer
  inside the browser turns it on with
  `DevToolsServer.init(); DevToolsServer.registerAllActors();`
  ([client-api][client-api]).
- The **transport** is a stream — either an in-process `nsIPipe` or a
  TCP socket — typically started with the `--start-debugger-server PORT`
  command-line flag.
- The **client** opens a stream to that port, exchanges framed JSON
  packets, and drives the server through *actors*.

[client-api]: https://firefox-source-docs.mozilla.org/devtools/backend/client-api.html

## Who consumes RDP

- **Firefox DevTools** — the in-browser debugger UI is just an RDP
  client talking to its own browser ([actor-hierarchy][ah]).
- **about:debugging** and **WebIDE** — remote-debug other Firefox
  instances (desktop ↔ Android, etc.).
- **Third-party tools** — anything speaking the wire protocol (this
  project, `ff-rdp`, is one such tool).
- Note: Firefox's **Remote Agent** (CDP / WebDriver BiDi) is a
  *separate* protocol surface, not RDP — don't confuse the two.

[ah]: https://firefox-source-docs.mozilla.org/devtools/backend/actor-hierarchy.html

## Core ideas at a glance

1. **Actors** — addressable server objects identified by string IDs.
   The fixed `"root"` actor is the entry point. See [[actor-model]].
2. **Hierarchy** — actors form a tree; *"once a parent is removed from
   the pool, its children are removed as well"* ([actor-hierarchy][ah]).
3. **Descriptors → Watcher → Targets → Resources** — a four-level model
   for finding debuggable contexts and observing what happens inside
   them. See [[rdp/protocol/resources]] and [[rdp/actors/watcher]].
4. **Two interaction styles** — request/reply, and request/reply +
   spontaneous notifications ([protocol][proto]).
5. **Two packet kinds** — JSON packets (`length:json`) and bulk packets
   (`bulk actor type length:bytes`). See [[../protocol/transport]].

[proto]: https://firefox-source-docs.mozilla.org/devtools/backend/protocol.html

## Layered view

```
+-------------------------------------------+
| DevTools UI / about:debugging / ff-rdp    |  client
+-------------------------------------------+
| protocol.js fronts (auto-generated stubs) |  client framework (optional)
+-------------------------------------------+
| Transport: length:JSON + bulk packets     |  wire
+-------------------------------------------+
| protocol.js actors                        |  server framework
+-------------------------------------------+
| DevToolsServer + RootActor + descriptors  |  server
+-------------------------------------------+
| Debugger API, DOM, network, console, ...  |  Firefox internals
+-------------------------------------------+
```

`protocol.js` is the *framework* that most actors are written in — it
generates request/reply wiring from declarative specs
([protocol.js][pjs]). A raw client (like `ff-rdp`) does not need to use
protocol.js; it only needs to speak the wire protocol.

[pjs]: https://firefox-source-docs.mozilla.org/devtools/backend/protocol.js.html

## Compatibility model

Backward compatibility is one-directional: *"Nightly desktop client
**MUST** maintain existing compatibility back to release channel
servers."* ([backward-compatibility][bc]). Clients detect features via
**traits** (boolean flags on actor forms / root) and
`target.hasActor("actorTypeName")`. Missing trait → assume `false`.
See [[rdp/protocol/error-handling]] for how a server signals "I don't
know that".

[bc]: https://firefox-source-docs.mozilla.org/devtools/backend/backward-compatibility.html

## Staleness warning

Older MDN pages (e.g. `developer.mozilla.org/.../Tools/Remote_Debugging`)
predate the Watcher / resources model and the
`target-available-form` event flow. When in doubt, prefer
`firefox-source-docs.mozilla.org` and `searchfox.org`.
