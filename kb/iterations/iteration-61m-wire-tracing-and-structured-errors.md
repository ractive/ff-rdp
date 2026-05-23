---
title: "Iteration 61m: Wire-level tracing + structured errors"
type: iteration
date: 2026-05-23
status: planned
branch: iter-61m/wire-tracing-structured-errors
depends_on:
  - iteration-61l-dogfood-53-fixes
tags: [iteration, tracing, errors, foundation, stability-roadmap]
---

# Iteration 61m: Wire-level tracing + structured errors

The foundation for everything downstream. Until we can see what bytes go across the RDP socket, every refactor in iter-61n…61s is debugged by squinting at JSON fixtures. Also: today's error reporting smuggles RDP-side errors, IO errors, and shape mismatches into the same untyped `anyhow::Error`, so failure modes are indistinguishable to callers and to live tests.

## Themes

- **A — `tracing`-crate wire dump.** `RUST_LOG=ff_rdp_core::transport=trace` prints every packet in/out, redacting sensitive payload fields (cookies, auth tokens, eval source).
- **B — Structured error taxonomy.** `thiserror` discriminant for `RdpError`: `Transport`, `Protocol{actor, name, message}`, `Shape{path, expected, got}`, `Timeout{phase, after_ms}`, `RemoteClosed`. CLI surfaces these as `meta.error_type` in JSON output.
- **C — Per-command error context.** Every command-layer call wraps errors with `anyhow::Context` so the chain reads `eval > consoleActor evaluateJSAsync > shape error: missing 'result' field`.

## Tasks

### A. Wire tracing
- [ ] Add `tracing` + `tracing-subscriber` to `ff-rdp-core` and `ff-rdp-cli`.
- [ ] In transport (send/receive packet loops): `tracing::trace!(target: "ff_rdp_core::transport", direction = ?dir, actor = %actor, kind = ?kind, payload_size = sz, body = %redact(json))`.
- [ ] Redactor: replace cookie values, auth-token strings, and `request.text`/`eval.text` bodies with `<redacted len=N>` by default. Add `FF_RDP_TRACE_RAW=1` to disable redaction for local debugging.
- [ ] `--log-level trace|debug|info|warn|error` CLI flag that maps onto `RUST_LOG`.

### B. Structured error type
- [ ] In `ff-rdp-core/src/error.rs`, define `RdpError` enum (`thiserror`) with the discriminants above.
- [ ] Replace `anyhow::anyhow!("…")` in core code paths with typed variants. CLI keeps `anyhow` at the outer boundary.
- [ ] CLI: when a command fails, JSON output is `{"error": "…", "error_type": "<discriminant>", "context": ["…", "…"]}` and exit code maps deterministically (`Protocol` → 3, `Shape` → 4, `Timeout` → 5, `Transport`/`RemoteClosed` → 6, others → 1).

### C. Error context plumbing
- [ ] Audit `crates/ff-rdp-cli/src/commands/*.rs`: every `.send_request(...)?` gets a `.with_context(|| format!("{actor} {method}"))`.
- [ ] Snapshot test in `tests/error_shapes.rs`: run a command against the mock server with a fault injected at each layer, assert the JSON output's `error_type` and `context` chain.

## Acceptance Criteria [0/6]

- [ ] `RUST_LOG=ff_rdp_core::transport=trace ff-rdp tabs` prints every request/reply on stderr in a one-line-per-packet format with redaction.
- [ ] `FF_RDP_TRACE_RAW=1` disables redaction and includes full bodies.
- [ ] CLI JSON errors include `meta.error_type` matching the `RdpError` discriminant.
- [ ] CLI exit codes map deterministically per the table above.
- [ ] Snapshot tests cover at least Protocol / Shape / Timeout / Transport faults.
- [ ] `cargo fmt && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace -q` clean.

## Design notes

- Tracing must not introduce allocations on the hot path when disabled. Use `tracing::trace!` (compile-time-optional) not `eprintln!`.
- The redactor's allowlist is conservative: by default, anything that looks like a JS string > 32 chars in `eval.text` or `set-cookie`/`cookie` headers is redacted. Make it tighter later if needed.
- Avoid double-wrapping: if a `ff_rdp_core::RdpError` reaches the CLI boundary, map it once to the JSON shape, don't bury it under another `anyhow::Error::from`.

## References

- [[ff-rdp-architecture-review]] §8 (Error handling) — current `anyhow` everywhere
- [[firefox-devtools-patterns-for-ff-rdp]] §10 (Logging and tracing), §8 (Errors as data)
- [[stability-roadmap]]
