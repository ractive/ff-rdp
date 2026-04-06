---
title: "Iteration 2: Connect + List Tabs"
type: iteration
date: 2026-04-06
tags:
  - iteration
  - connection
  - tabs
  - e2e
status: completed
branch: iter-2/connect-tabs
---

# Iteration 2: Connect + List Tabs (First Working E2E)

Establish TCP connection to Firefox, handle the RDP handshake, and implement the first working command: `ff-rdp tabs`.

## Tasks

### Core Protocol

- [x] Implement `ff-rdp-core/src/connection.rs` — `RdpConnection`: connect to host:port, read handshake greeting from root actor, validate `applicationType`
- [x] Implement `ff-rdp-core/src/actor.rs` — base `Actor` struct with `request(&mut transport, to, type, params) -> Result<Value>` method
- [x] Implement `ff-rdp-core/src/actors/root.rs` — `RootActor` with `list_tabs()` returning typed `TabInfo` structs (actor_id, title, url, selected, browsing_context_id)
- [x] Implement `ff-rdp-core/src/actors/root.rs` — `get_root()` returning device/preferences actor IDs
- [x] Implement `ff-rdp-core/src/actors/tab.rs` — `TabDescriptor` struct with metadata fields

### CLI

- [x] Implement tab targeting in CLI: resolve `--tab <index|url-pattern>` and `--tab-id <actor>` to a specific tab actor ID
- [x] Implement `ff-rdp-cli/src/commands/tabs.rs` — `ff-rdp tabs` command outputting tab list through JSON envelope

### E2E Test Infrastructure

- [x] Create `crates/ff-rdp-core/tests/support/mock_server.rs` — reusable mock TCP server that speaks RDP framing (length-prefixed JSON), matches requests by `type` field and replies with registered responses, runs on `std::net::TcpListener` on a random port
- [x] Create `crates/ff-rdp-core/tests/fixtures/` directory with captured JSON fixtures: `handshake.json` (greeting packet) and `list_tabs_response.json` (listTabs response)
- [x] Capture fixtures: connect to a real Firefox instance (`--start-debugger-server`), record the handshake greeting and a `listTabs` round-trip, sanitize and save as fixture files
- [x] Write mock-server integration tests for `RdpConnection` + `RootActor.list_tabs()` — verify the full flow from connect → handshake → listTabs → typed TabInfo output, using the mock server with captured fixtures
- [x] Write CLI e2e tests in `crates/ff-rdp-cli/tests/` — spawn `ff-rdp tabs` as a subprocess against the mock server, assert JSON envelope structure, test `--jq` filtering
- [x] Add live Firefox tests behind `#[ignore]` + env var gate (`FF_RDP_LIVE_TESTS=1`) — connect to a real Firefox debug server, run `ff-rdp tabs`, validate output against a running browser. Skipped in CI by default, runnable locally with `cargo test -- --ignored`
- [x] Document test strategy in `kb/research/e2e-test-strategy.md` — mock server design, fixture capture workflow, how to refresh fixtures when protocol changes, live test setup instructions

## Acceptance Criteria

- `ff-rdp tabs` connects to Firefox and outputs tab list as JSON envelope
- `ff-rdp tabs --jq '.results[0].url'` extracts first tab's URL
- `ff-rdp tabs --jq '.total'` outputs tab count
- Tab targeting by index works: `ff-rdp tabs --tab 1` (for future commands)
- Connection timeout produces clear error with hint about starting Firefox
- Mock server e2e tests pass in CI without requiring a running Firefox instance
- Live Firefox tests pass locally when `FF_RDP_LIVE_TESTS=1` is set and Firefox is running with `--start-debugger-server`
- Fixture capture workflow is documented so fixtures can be refreshed when Firefox changes protocol behavior
