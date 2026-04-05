---
title: "E2E Test Strategy"
type: research
date: 2026-04-06
status: completed
tags: [testing, e2e, mock-server, fixtures, research]
---

# E2E Test Strategy

## Overview

Tests are organized in three tiers, each trading speed for realism:

| Tier | Location | Runs in CI | External deps |
|------|----------|-----------|---------------|
| **Unit tests** | `crates/*/src/**` (inline `#[cfg(test)]`) | Yes | None |
| **Mock-server integration** | `crates/ff-rdp-core/tests/` | Yes | None |
| **Live Firefox tests** | `crates/ff-rdp-core/tests/live_firefox_test.rs` | No (opt-in) | Firefox |

Unit tests cover pure logic. Mock-server tests exercise the full protocol path (TCP connect, length-prefix framing, JSON parsing) against a controlled server. Live tests validate against a real Firefox instance.

## Mock TCP Server

`MockRdpServer` (in `crates/ff-rdp-core/tests/support/mock_server.rs`) is a single-connection TCP server that speaks the Firefox RDP length-prefixed JSON protocol.

Key behavior:
- Binds `127.0.0.1:0` (OS-assigned port) — no port conflicts in parallel test runs
- Sends a configurable greeting immediately on accept
- Matches incoming requests by `"type"` field, responds with registered handlers (first match wins)
- Returns all received requests when the client disconnects — useful for asserting what the client sent
- Unmatched requests get a generic `unknownMethod` error reply so the client does not hang

Builder pattern usage:

```rust
let fixture = load_fixture("list_tabs_response.json");

let server = MockRdpServer::new()
    .with_greeting(load_fixture("handshake.json"))
    .on("listTabs", fixture);

let port = server.port();
let handle = std::thread::spawn(move || server.serve_one());

let mut conn = RdpConnection::connect("127.0.0.1", port, TIMEOUT).unwrap();
let tabs = RootActor::list_tabs(conn.transport_mut()).unwrap();

drop(conn);
let requests = handle.join().unwrap();
assert_eq!(requests[0]["type"], "listTabs");
```

## Fixture Files

Location: `crates/ff-rdp-core/tests/fixtures/`

Current fixtures:
- `handshake.json` — greeting sent by Firefox on connect (`applicationType: "browser"`, traits)
- `list_tabs_response.json` — `listTabs` response with two sanitized example tabs

Format: raw JSON, one complete RDP response per file. Loaded at test time via the `load_fixture()` helper in `tests/support/mod.rs`, which reads from `CARGO_MANIFEST_DIR/tests/fixtures/` and parses into `serde_json::Value`.

## How to Capture / Refresh Fixtures

1. **Start Firefox in debugger mode:**
   ```sh
   firefox --start-debugger-server 6000 -headless
   ```

2. **Capture raw responses** using `socat` or `nc`:
   ```sh
   # Connect and see the greeting
   socat - TCP:127.0.0.1:6000

   # Or use nc to send a request and capture the response
   echo -n '20:{"type":"listTabs"}' | nc 127.0.0.1 6000
   ```
   Alternatively, add temporary `eprintln!` logging in the transport layer's `recv_from` and rebuild.

3. **Sanitize** the captured JSON:
   - Replace personal URLs, page titles, and bookmark data with generic values
   - Keep actor IDs in the `server1.conn0.*` format
   - Preserve the full structure — do not remove fields the parser ignores today (they may matter later)

4. **Save** to `tests/fixtures/<descriptive_name>.json` — one response per file, pretty-printed.

## Live Firefox Tests

Located in `crates/ff-rdp-core/tests/live_firefox_test.rs`. Gated behind two mechanisms:

- `#[ignore]` attribute — skipped by default in `cargo test`
- Runtime check: `FF_RDP_LIVE_TESTS=1` env var must be set

To run locally:

```sh
# Terminal 1: start Firefox
firefox --start-debugger-server 6000

# Terminal 2: run the live tests
FF_RDP_LIVE_TESTS=1 cargo test --package ff-rdp-core -- --ignored
```

Override the port with `FF_RDP_PORT`:

```sh
FF_RDP_PORT=9222 FF_RDP_LIVE_TESTS=1 cargo test --package ff-rdp-core -- --ignored
```

## CLI E2E Tests

Located in `crates/ff-rdp-cli/tests/`. These tests:

1. Start a `MockRdpServer` on a random port
2. Spawn the `ff-rdp` binary as a subprocess, passing `--port <mock_port>`
3. Assert on the JSON stdout output (exit code, structure, field values)
4. Optionally pipe output through `--jq` filters and verify the filtered result

This validates the full path from CLI argument parsing through protocol interaction to JSON output formatting.

## CI Integration

**Always run:** unit tests + mock-server integration tests on all three platforms (Linux, macOS, Windows). These have zero external dependencies and complete in seconds.

**Skipped by default:** live Firefox tests. The `#[ignore]` attribute ensures they never run unless explicitly requested.

**Future option:** a nightly or weekly CI job that installs Firefox headless and runs the live test suite. This would catch regressions from Firefox RDP protocol changes. Not needed until the command surface is larger.

## Adding Tests for New Commands

Follow this pattern when implementing a new RDP command (e.g., `navigateTo`):

1. **Capture a fixture** — connect to a real Firefox, send the request, save the response to `tests/fixtures/navigate_response.json`
2. **Register a mock handler** — in your integration test: `.on("navigateTo", load_fixture("navigate_response.json"))`
3. **Write integration tests** — happy path, error path, edge cases (see `connection_test.rs` for examples)
4. **Add CLI e2e tests** — spawn the binary, assert JSON output matches expectations
5. **Update live tests** — add an `#[ignore]` test that exercises the command against real Firefox

See [[decision-log]] for architectural decisions that affect the test strategy.
