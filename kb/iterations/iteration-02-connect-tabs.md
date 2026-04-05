---
title: "Iteration 2: Connect + List Tabs"
type: iteration
date: 2026-04-06
tags: [iteration, connection, tabs, e2e]
status: planned
branch: iter-2/connect-tabs
---

# Iteration 2: Connect + List Tabs (First Working E2E)

Establish TCP connection to Firefox, handle the RDP handshake, and implement the first working command: `ff-rdp tabs`.

## Tasks

- [ ] Implement `ff-rdp-core/src/connection.rs` — `RdpConnection`: connect to host:port, read handshake greeting from root actor, validate `applicationType`
- [ ] Implement `ff-rdp-core/src/actor.rs` — base `Actor` struct with `request(&mut transport, to, type, params) -> Result<Value>` method
- [ ] Implement `ff-rdp-core/src/actors/root.rs` — `RootActor` with `list_tabs()` returning typed `TabInfo` structs (actor_id, title, url, selected, browsing_context_id)
- [ ] Implement `ff-rdp-core/src/actors/root.rs` — `get_root()` returning device/preferences actor IDs
- [ ] Implement `ff-rdp-core/src/actors/tab.rs` — `TabDescriptor` struct with metadata fields
- [ ] Implement tab targeting in CLI: resolve `--tab <index|url-pattern>` and `--tab-id <actor>` to a specific tab actor ID
- [ ] Implement `ff-rdp-cli/src/commands/tabs.rs` — `ff-rdp tabs` command outputting tab list through JSON envelope
- [ ] Integration test with mock TCP server replaying captured Firefox RDP handshake + listTabs response
- [ ] Manual E2E test: start Firefox with `--start-debugger-server 6000`, run `ff-rdp tabs`

## Acceptance Criteria

- `ff-rdp tabs` connects to Firefox and outputs tab list as JSON envelope
- `ff-rdp tabs --jq '.results[0].url'` extracts first tab's URL
- `ff-rdp tabs --jq '.total'` outputs tab count
- Tab targeting by index works: `ff-rdp tabs --tab 1` (for future commands)
- Connection timeout produces clear error with hint about starting Firefox
- Mock server tests pass without requiring a running Firefox instance
