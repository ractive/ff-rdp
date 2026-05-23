---
title: RDP Client (Firefox-side) — Index
type: index
tags: [rdp, index, client]
date: 2026-05-23
---

# RDP Client (Firefox's own)

How Firefox itself talks to a Firefox debugger server. `ff-rdp` reimplements this layer in Rust, but the JS reference is the source of truth for protocol shapes.

- [[rdp/client/transport|transport]] — TCP framing + WebSocket variant (mirror of [[rdp/protocol/transport|protocol/transport]] but from the client's perspective).
- [[spec-and-front]] — the `Arg`/`Option`/`RetVal` spec framework. Every server actor has a matching JS spec in `devtools/shared/specs/`. These spec files ARE the protocol contract; treat them as a documented IDL.
- [[devtools-client]] — `DevToolsClient` + `RootFront`: connection bootstrap, request lifecycle, per-actor serialization quirks.
- [[remote-agent-cdp]] — historical note: the Remote Agent used to bridge CDP→RDP. CDP support has been **removed**; only Marionette and WebDriver BiDi remain.

## Why this matters for ff-rdp

`ff-rdp` doesn't use the spec/Front framework — it speaks JSON over TCP directly. But:

- The spec files in `devtools/shared/specs/*.js` are still our canonical IDL — when a method signature is unclear from server source, the spec spells out the typed shape.
- `DevToolsClient`'s serialization and reconnect logic shows the edge cases that bite long-running clients (actor cache invalidation, in-flight request bookkeeping).
- The CDP→BiDi migration is why we should not invest in the `/remote/` code paths for inspiration — they're moving away from RDP, not into it.
