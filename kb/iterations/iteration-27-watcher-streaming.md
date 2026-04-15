---
title: "Iteration 27: Watcher & Streaming"
type: iteration
status: completed
date: 2026-04-07
tags:
  - iteration
  - watcher
  - streaming
  - console
branch: iter-27/watcher-streaming
---

# Iteration 27: Watcher & Streaming

Enable real-time event streaming via watcher resource subscriptions and target watching.

## Notes

Target watching protocol research requires a live Firefox instance. Launch headless Firefox for
discovery: `firefox -no-remote -profile /tmp/ff-rdp-test-profile --start-debugger-server 6000 --headless`

## Tasks

- [x] Implement `watchResources(["console-message"])` for real-time console output
  → [[backlog/issues/console-message-watching]]
- [x] Add `ff-rdp console --follow` to tail console messages live
- [x] Implement `watchTargets("frame")` for seamless navigation target tracking
  → [[backlog/issues/target-watching-navigation]]
- [x] Add `--follow` flag to `ff-rdp network` for live network event streaming
- [x] Improve RDP error protocol handling: distinguish unknownActor, wrongState,
  threadWouldRun and provide actionable error messages
  → [[backlog/issues/structured-error-protocol]]
- [x] Daemon compatibility: decide whether `--follow` commands bypass the daemon
  (hold own connection) or stream through it. Test both paths.

## Test Fixtures

All e2e test fixtures must be recorded from a real Firefox instance — never hand-craft them.
Run with `FF_RDP_LIVE_TESTS_RECORD=1 cargo test -p ff-rdp-core --test live_record_fixtures -- --ignored` to record fixtures.
