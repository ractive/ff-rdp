---
title: "Iteration 25: Daemon Reliability"
type: iteration
status: completed
date: 2026-04-07
tags:
  - iteration
  - daemon
  - reliability
branch: iter-25/daemon-reliability
---

# Iteration 25: Daemon Reliability

Fix the daemon's event forwarding so all commands work identically with and without
`--no-daemon`.

## Tasks

- [x] Fix `navigate --with-network` returning empty through daemon: investigate
  why watcher events don't arrive in `drain_network_from_daemon` during navigation
  → [[backlog/issues/daemon-navigate-with-network-empty]]
- [x] Redesign daemon event forwarding: stream watcher events to the requesting
  client in real-time instead of buffering for post-hoc drain
  → [[backlog/issues/daemon-realtime-watcher-events]]
- [x] Ensure parity: run the full e2e test suite with and without daemon,
  both must produce identical results

## Test Fixtures

All e2e test fixtures must be recorded from a real Firefox instance — never hand-craft them.
Run with `FF_RDP_LIVE_TESTS_RECORD=1 cargo test -p ff-rdp-core --test live_record_fixtures -- --ignored` to record fixtures.
