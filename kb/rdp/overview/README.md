---
title: RDP Overview — Index
type: index
tags: [rdp, index, overview]
date: 2026-05-23
---

# RDP Overview

Start here if you've never touched RDP. These pages skip wire format and per-actor detail and just establish the model.

- [[architecture]] — what RDP is, who uses it (DevTools, BiDi adapter, ff-rdp), how it slots into Firefox.
- [[actor-model]] — the actor abstraction: opaque IDs (`server1.conn3.child22/consoleActor7`), lifecycle, parent/child cascade, four flavours.
- [[connection-lifecycle]] — boot sequence from TCP `connect` through Watcher subscriptions to clean shutdown (9 steps).

Once these click, move on to **[[rdp/protocol/README|protocol/]]** for wire details, **[[rdp/actors/README|actors/]]** for per-actor contracts, and **[[rdp/flows/README|flows/]]** for end-to-end walkthroughs.
