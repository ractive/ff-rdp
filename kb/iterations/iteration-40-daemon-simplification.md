---
title: "Iteration 40: Daemon Simplification & Security Hardening"
type: iteration
status: completed
date: 2026-04-09
branch: iter-40/daemon-simplification
tags:
  - iteration
  - daemon
  - simplification
  - security
  - architecture
  - review
---

# Iteration 40: Daemon Simplification & Security Hardening

## Goal

Simplify the daemon by having more commands bypass it (like screenshot already does), and address the security findings from the architecture review.

## Context

### Architecture review findings

The daemon provides genuine value for **streaming commands** (`console --follow`, `network --follow`, `navigate --with-network`) where it buffers events across CLI invocations. For **one-shot commands** (most commands), the daemon adds complexity without benefit — it's just a TCP proxy that can interfere with protocol interactions.

Current workarounds already in place:
- `screenshot` → always bypasses daemon (watcher subscription interferes with two-step protocol)
- `cookies` → opens a **separate direct connection** when via daemon (watcher intercepts `resources-available-array`)

### Security review findings

Overall security posture is **good**:
- Daemon binds to `127.0.0.1` only (no remote access)
- File permissions are `0o600`/`0o700` on Unix
- Atomic writes with file locking prevent corruption
- URL validation rejects `javascript:`/`data:` by default
- No shell injection in process spawning

One design limitation found:
- **RPC writer replacement race**: When multiple CLI clients connect simultaneously, the last one becomes the RPC writer and may receive responses meant for the previous client. Not exploitable (same user, localhost only) but causes confusing behavior.

## Part A: Commands That Should Bypass Daemon [4/4]

### Principle
Commands should bypass the daemon unless they **specifically benefit from event buffering or streaming**. This means most commands should use `connect_direct()`.

### A1: Make `cookies` use `connect_direct()` [1/1]
- [x] Replace the current workaround in `cookies.rs` (which opens a separate direct connection when `via_daemon`) with simply calling `connect_direct(cli)` like screenshot does. Remove the `via_daemon` branching code.

### A2: Make `storage` use `connect_direct()` [1/1]
- [x] `storage` (localStorage, sessionStorage) has the same issue as cookies — the daemon's watcher subscription can intercept storage actor responses. Switch to `connect_direct()`.

### A3: Make `a11y` use `connect_direct()` [1/1]
- [x] `a11y` falls back to JS eval when the accessibility walker actor doesn't respond as expected through the daemon. Direct connection avoids this issue.

### A4: Make `sources` use `connect_direct()` [1/1]
- [x] `sources` has the same JS eval fallback pattern as `a11y`. Direct connection would let the native actor path work correctly.

## Part B: Simplify Daemon Message Routing [3/3]

### B1: Remove watcher-interception guard for cookies/storage [1/1]
- [x] The daemon's `firefox_reader_loop` has special logic to distinguish its own watcher events from other actors' events (to avoid intercepting cookies/storage responses). Once cookies and storage bypass the daemon (Part A), this guard may be simplifiable. Review whether it's still needed for any remaining daemon-proxied command.

### B2: Document which commands use daemon vs direct [1/1]
- [x] Add a clear table/comment at the top of `dispatch.rs` or in a doc file listing:
  - **Direct**: screenshot, cookies, storage, a11y, sources
  - **Daemon (streaming)**: console --follow, network --follow, navigate --with-network
  - **Daemon (proxied)**: all other commands (eval, click, type, wait, dom, geometry, etc.)

### B3: Consider: should non-streaming `network` and `console` bypass daemon? [1/1]
- [x] `network` (without `--follow`) currently drains buffered events from the daemon. This is useful if the daemon has been capturing events in the background. Evaluate the trade-off: buffer access vs simpler code path. **Decision point for discussion** — document pros/cons but don't implement without user input.

## Part C: Security Hardening [3/3]

### C1: Add daemon port to error context [1/1]
- [x] When daemon-related errors occur, include the daemon port and log path in the error message: `"Check ~/.ff-rdp/daemon.log for details (daemon on port NNNNN)"`

### C2: Document the RPC writer limitation [1/1]
- [x] Add a comment in `server.rs` near the `rpc_writer` replacement logic explaining that concurrent CLI clients may receive each other's responses. This is a known limitation, not a bug — document it clearly so future contributors understand.

### C3: Validate daemon registry on read [1/1]
- [x] When reading `daemon.json`, validate that `proxy_port` is in valid range (1-65535) and `pid` is positive. Currently the code trusts the JSON content. A corrupted or malicious registry file could cause confusing errors.

## Part D: Test Updates [2/2]

### D1: Update e2e tests for commands that now bypass daemon [1/1]
- [x] Remove or update any daemon parity tests for commands that now always use direct mode
- [x] Ensure existing e2e tests still pass (they already test direct mode)

### D2: Add test for daemon registry validation [1/1]
- [x] Add unit test: corrupted `daemon.json` (invalid port, negative PID, malformed JSON) should return a clean error, not panic

## Acceptance Criteria

- [x] `cookies`, `storage`, `a11y`, `sources` all bypass daemon like `screenshot` does
- [x] Workaround code removed from `cookies.rs` (direct connection branching)
- [x] Daemon routing documented in a clear table
- [x] RPC writer limitation documented in code
- [x] Registry validation added
- [x] `cargo fmt`, `cargo clippy`, `cargo test` pass
- [x] No regressions in daemon streaming commands (`console --follow`, `network --follow`)

## Related

- [[iterations/iteration-13-connection-daemon]] — original daemon implementation
- [[iterations/iteration-14-security-code-review]] — prior security review
- [[iterations/iteration-38-daemon-client-timeout]] — screenshot bypass daemon + client timeout improvements
