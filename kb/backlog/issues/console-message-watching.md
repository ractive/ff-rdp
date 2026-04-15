---
title: Real-time console monitoring via watcher resource subscription
type: feature
status: resolved
priority: medium
discovered: 2026-04-07
tags:
  - console
  - watcher
  - streaming
  - protocol
---

# Real-time console monitoring via watcher resource subscription

Currently `ff-rdp console` uses `getCachedMessages` for a point-in-time snapshot.
The WatcherActor supports `watchResources(["console-message"])` for real-time
streaming of console output.

This would enable `ff-rdp console --follow` to tail console output live,
similar to `tail -f`. Could also watch `error-message` for page errors
and `stylesheet` for CSS changes.

## Supported resource types

- `console-message` — real-time console.log/warn/error
- `error-message` — page errors (JS exceptions, network errors)
- `source` — script sources as they load
- `stylesheet` — CSS changes
