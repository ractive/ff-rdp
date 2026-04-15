---
title: Daemon should forward watcher events in real-time
type: improvement
status: resolved
priority: medium
discovered: 2026-04-07
tags:
  - daemon
  - watcher
  - architecture
---

# Daemon should forward watcher events in real-time

Currently the daemon buffers watcher resource events and clients drain them after
the fact. This causes timing issues where events generated during a command
(e.g. `navigate --with-network`) are missed because they arrive between the command
send and the drain.

## Desired behavior

The daemon should stream watcher events to the requesting client in real-time,
so that events generated during a navigation are captured in the same response.

## Related

- [[backlog/issues/daemon-navigate-with-network-empty]]
