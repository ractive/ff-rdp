---
title: RDP Flows — Index
type: index
tags: [rdp, index, flows]
date: 2026-05-23
---

# RDP Flows (end-to-end walkthroughs)

Each flow traces a real DevTools (or BiDi adapter) action all the way from the client API call to the server-side actors and back. Useful when implementing the same flow in `ff-rdp`.

- [[connect-and-list-tabs]] — TCP `connect` → root packet → `RootFront.listTabs()` → tab descriptors. Foundational.
- [[attach-target]] — descriptor → target front → (implicit attach) → child actors reachable. The modern flow removed the explicit `attach()`; many older docs still describe it.
- [[evaluate-js]] — `WebConsoleFront.evaluateJSAsync` deferred-result pattern: server returns `{resultID}` immediately, real result arrives later as an `evaluationResult` event. The `mapped: { await: true }` flag is what enables Promise-resolving on the server (and bypasses page CSP).
- [[watch-resources]] — `WatcherFront.watchTargets("frame")` + `WatcherFront.watchResources([…])`: both required. Batched `[[type, [resources…]], …]` event shape. 100ms throttling.
- [[take-screenshot]] — TWO RDP requests (not one): `screenshot-content.prepareCapture({fullpage:true})` returns the rect; `screenshot.capture({fullpage, rect, snapshotScale, browsingContextID, …})` invokes `drawSnapshot(rect, ratio, bg, fullpage)`. The 4th positional arg `fullpage` is the actual switch — not the rect alone. **This is the lookup for our long-standing `--full-page` bug.**

## How to read these

Each flow page is structured as:

1. Goal / outcome (one line).
2. Client-side trigger (file + line in Firefox checkout).
3. Wire trace — request packets, server-side handlers, events emitted, reply.
4. Edge cases and gotchas.
5. ff-rdp implementation pointers (where we currently implement this, gaps).

When something here contradicts the wiki's overview pages, the flow wins — it's grounded in actual code.
