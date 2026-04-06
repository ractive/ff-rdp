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
| **Live Firefox tests** | `crates/ff-rdp-core/tests/live_*.rs` | No (opt-in) | Firefox |

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

All fixtures are recorded from a live Firefox instance using the unified recording tests.

**Single command to refresh all fixtures:**

```sh
# 1. Start Firefox (see Starting Firefox below)
# 2. Record all fixtures:
FF_RDP_LIVE_TESTS_RECORD=1 cargo test -p ff-rdp-core --test live_record_fixtures -- --ignored
```

This runs each live test against real Firefox AND writes the validated response to the corresponding fixture file on disk. Fixture files are written to both `ff-rdp-core/tests/fixtures/` and `ff-rdp-cli/tests/fixtures/` as appropriate.

**Environment variables:**
- `FF_RDP_LIVE_TESTS=1` — run live tests without recording (validation only)
- `FF_RDP_LIVE_TESTS_RECORD=1` — run live tests AND write fixture files (implies live tests)
- `FF_RDP_PORT=6000` — override the default Firefox debugger port

**What happens during recording:**
1. Each live test exercises a real Firefox command and validates the response structure
2. When `RECORD=1`, the validated response is normalized (actor connection IDs `conn\d+` → `conn0`) and saved as the fixture file
3. CLI e2e tests (mock-based) continue to load these fixture files as before

**Normalization:** Only actor connection IDs are normalized for cross-fixture consistency. Timestamps, window IDs, and other volatile fields are left as raw Firefox output.

## Starting Firefox with the Debug Server

Firefox requires explicit configuration to enable the remote debugging server. The `--start-debugger-server` flag alone is not enough — the `devtools.debugger.remote-enabled` preference must also be `true`.

### macOS Setup

You can run a debug Firefox instance alongside your normal browser using `-no-remote` and a separate profile:

```sh
# 1. Create a test profile with remote debugging enabled (one-time setup)
mkdir -p /tmp/ff-rdp-test-profile
cat > /tmp/ff-rdp-test-profile/user.js << 'PREFS'
user_pref("devtools.debugger.remote-enabled", true);
user_pref("devtools.debugger.prompt-connection", false);
user_pref("devtools.chrome.enabled", true);
PREFS

# 2. Launch a separate Firefox process with the debug server
/Applications/Firefox.app/Contents/MacOS/firefox \
  --start-debugger-server 6000 \
  -no-remote \
  -profile /tmp/ff-rdp-test-profile

# 3. Verify it's listening
nc -z localhost 6000 && echo "listening"
```

Key flags:
- **`-no-remote`**: Forces a new Firefox process instead of joining the already-running instance. This lets you keep your normal browser open.
- **`-profile <path>`**: Uses a separate profile directory so the debug prefs don't affect your main profile.
- **`user.js`**: Firefox reads this on startup and applies the preferences. The three prefs enable the debug server, disable the connection prompt, and enable chrome debugging.

### Linux / Headless

```sh
firefox --start-debugger-server 6000 -headless -no-remote -profile /tmp/ff-rdp-test-profile
```

Same `user.js` setup applies. Headless mode is useful for CI.

### Windows

```sh
"C:\Program Files\Mozilla Firefox\firefox.exe" --start-debugger-server 6000 -no-remote -profile %TEMP%\ff-rdp-test-profile
```

## Live Firefox Tests

Located in:
- `crates/ff-rdp-core/tests/live_firefox_test.rs` — basic connection tests
- `crates/ff-rdp-core/tests/live_record_fixtures.rs` — comprehensive fixture recording tests

Gated behind two mechanisms:
- `#[ignore]` attribute — skipped by default in `cargo test`
- Runtime check: `FF_RDP_LIVE_TESTS=1` or `FF_RDP_LIVE_TESTS_RECORD=1` env var must be set

To run locally:

```sh
# Terminal 1: start Firefox (see setup above)
/Applications/Firefox.app/Contents/MacOS/firefox \
  --start-debugger-server 6000 -no-remote \
  -profile /tmp/ff-rdp-test-profile

# Terminal 2: run live tests only (no recording)
FF_RDP_LIVE_TESTS=1 cargo test --package ff-rdp-core -- --ignored

# Or: run live tests AND record all fixtures
FF_RDP_LIVE_TESTS_RECORD=1 cargo test --package ff-rdp-core -- --ignored
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

1. **Add a live test** in `live_record_fixtures.rs` — exercise the command against real Firefox, validate the response, and call `save_cli_fixture()` to record the fixture
2. **Record the fixture** — run `FF_RDP_LIVE_TESTS_RECORD=1 cargo test -p ff-rdp-core --test live_record_fixtures -- --ignored live_your_test`
3. **Register a mock handler** — in your CLI e2e test: `.on("yourMethod", load_fixture("your_fixture.json"))`
4. **Write CLI e2e tests** — spawn the binary, assert JSON output matches expectations
5. **Write integration tests** — if needed, add mock-server tests for edge cases (see `connection_test.rs`)

See [[decision-log]] for architectural decisions that affect the test strategy.
