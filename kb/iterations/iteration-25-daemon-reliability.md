---
title: "Iteration 25: Daemon Reliability"
type: iteration
status: planned
date: 2026-04-07
tags: [iteration, daemon, reliability]
branch: iter-25/daemon-reliability
---

# Iteration 25: Daemon Reliability

Fix the daemon's event forwarding so all commands work identically with and without
`--no-daemon`.

## Tasks

- [ ] Fix `navigate --with-network` returning empty through daemon: investigate
  why watcher events don't arrive in `drain_network_from_daemon` during navigation
  → [[daemon-navigate-with-network-empty]]
- [ ] Redesign daemon event forwarding: stream watcher events to the requesting
  client in real-time instead of buffering for post-hoc drain
  → [[daemon-realtime-watcher-events]]
- [ ] Ensure parity: run the full e2e test suite with and without daemon,
  both must produce identical results
