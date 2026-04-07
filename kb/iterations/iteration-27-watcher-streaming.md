---
title: "Iteration 27: Watcher & Streaming"
type: iteration
status: planned
date: 2026-04-07
tags: [iteration, watcher, streaming, console]
branch: iter-27/watcher-streaming
---

# Iteration 27: Watcher & Streaming

Enable real-time event streaming via watcher resource subscriptions and target watching.

## Tasks

- [ ] Implement `watchResources(["console-message"])` for real-time console output
  → [[console-message-watching]]
- [ ] Add `ff-rdp console --follow` to tail console messages live
- [ ] Implement `watchTargets("frame")` for seamless navigation target tracking
  → [[target-watching-navigation]]
- [ ] Add `--follow` flag to `ff-rdp network` for live network event streaming
- [ ] Improve RDP error protocol handling: distinguish unknownActor, wrongState,
  threadWouldRun and provide actionable error messages
  → [[structured-error-protocol]]
- [ ] Daemon compatibility: decide whether `--follow` commands bypass the daemon
  (hold own connection) or stream through it. Test both paths.
