---
title: "navigate --with-network returns empty through daemon"
type: bug
status: open
priority: high
discovered: 2026-04-07
tags: [daemon, network, navigate, dogfooding]
---

# navigate --with-network returns empty through daemon

`navigate --with-network` works correctly with `--no-daemon` but returns an empty
`network: []` when routed through the daemon.

The daemon path uses `drain_network_from_daemon` which reads buffered events from
the daemon's event store. The watcher resource events generated during navigation
don't arrive in the daemon's buffer.

## Repro

```sh
ff-rdp navigate --with-network https://example.com
# network: []

ff-rdp navigate --with-network --no-daemon https://example.com
# network: [... correct entries ...]
```

## Root cause

The daemon buffers events by type key (`"network-event"`), but the watcher events
generated during a `navigateTo` request are not being forwarded to the daemon's
event buffer in time for the drain.
