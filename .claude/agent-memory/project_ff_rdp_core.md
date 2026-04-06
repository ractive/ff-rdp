---
name: ff-rdp-core implementation status
description: Transport design, API shape, test infrastructure, and known quirks of the ff-rdp-core crate
type: project
---

## Core crate: `crates/ff-rdp-core`

### Public API surface (re-exported from `lib.rs`)
- `RdpTransport` — low-level; `connect_raw`, `connect`, `from_parts`, `send`, `recv`, `request`
- `RdpConnection` — high-level; `connect(host, port, timeout)` validates greeting; exposes `transport_mut()` and `timeout()`
- `RootActor::list_tabs(transport) -> Vec<TabInfo>`
- `TabInfo` — `actor: ActorId`, `title`, `url`, `selected`, `browsing_context_id: Option<u64>`
- `ProtocolError` — `ConnectionFailed`, `SendFailed`, `RecvFailed`, `InvalidPacket`, `Timeout`, `ActorError`
- `transport::encode_frame(json: &str) -> String` and `transport::recv_from(reader)` are public

### Wire format
Length-prefixed JSON: `{byte_len}:{json}` over TCP.
Firefox sends a greeting immediately on connect — `RdpConnection::connect` reads and validates it (`applicationType == "browser"`).

### Known quirks
- `TabInfo::browsing_context_id` uses `#[serde(rename = "browsingContextID")]` (uppercase D), not the camelCase default `browsingContextId`. Firefox sends the uppercase form. Fixed in iteration 2.
- `RdpTransport` does not derive `Debug` (tokio split halves don't implement it); manual impl using `finish_non_exhaustive`.
- `RdpConnection` derives `Debug` via the manual `RdpTransport` impl.

### Test infrastructure (iteration 2)
- `tests/fixtures/handshake.json` — standard Firefox greeting
- `tests/fixtures/list_tabs_response.json` — two realistic tabs
- `tests/support/mod.rs` + `tests/support/mock_server.rs` — `MockRdpServer` builder
  - Binds `127.0.0.1:0`, configurable greeting + per-method handlers
  - `serve_one()` → `Result<Vec<Value>, String>` (returns all received requests)
  - EOF/ConnectionReset treated as clean client disconnect
  - Unmatched methods get an `unknownMethod` actor-error response
- `tests/connection_test.rs` — 8 integration tests covering connect success/failure, greeting validation, listTabs happy path + empty, timeout, connection refused
- `tests/live_firefox_test.rs` — 2 `#[ignore]`d tests; gate on `FF_RDP_LIVE_TESTS=1`, port from `FF_RDP_PORT` (default 6000)

### All tests passing
`cargo test --workspace` — 34 unit + 8 integration + 2 ignored + 1 doctest = clean
