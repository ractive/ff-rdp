---
title: "Iteration 72: Transport polish — frame-size knob, dispatch allocations, redact threshold"
type: iteration
date: 2026-05-24
status: planned
branch: iter-72/transport-polish
depends_on:
  - iteration-61q-resource-command-bus
first_call_sites:
  - primitive: "ff_rdp_core::transport::set_max_frame_bytes"
    site: "crates/ff-rdp-cli/src/main.rs (applied once at startup from --max-frame-mb)"
dogfood_path: |
  # 1. Raise frame cap for heap snapshot.
  ff-rdp --max-frame-mb 256 memory heap-snapshot https://example.com /tmp/snap.bin
  # Expected: 120 MB snapshot lands without FrameTooLarge.

  # 2. Trace shows full URLs (not truncated to <redacted len=N>).
  ff-rdp --log-rdp-trace --redact-threshold 256 navigate 'https://example.com/?utm_source=x&utm_campaign=y'
  grep 'utm_source' ~/.cache/ff-rdp/rdp-trace.log    # expect raw URL visible

  # 3. Dispatch microbench still passes its budget.
  cargo bench -p ff-rdp-core --bench resource_dispatch
tags: [iteration, protocol]
---

# Iteration 72: Transport polish — frame-size knob, dispatch allocations, redact threshold

Three lower-priority but real items from the protocol review:
(1) `MAX_FRAME_BYTES = 64 MiB` is fixed and undocumented; Firefox allows up
to 1 TiB. Realistic heap-snapshot fixtures exceed 64 MiB.
(2) `parse_available_resources` re-wraps each sub-array with `json!()`
before calling typed parsers — pure allocation overhead on the daemon's
hottest path.
(3) `redact()`'s `MAX_INLINE_STR = 32` truncates everything including
non-sensitive long URLs, obscuring debug traces.

## Themes

- **A — `--max-frame-mb` knob.** Configurable; default raised to 256 MiB
  (heap-snapshot workloads).
- **B — Allocation-free dispatch.** Pass sub-arrays directly to typed
  parsers in `parse_available_resources`.
- **C — Tunable redact threshold.** Raise the default and expose a CLI
  flag; sensitive-keyed values still get redacted regardless.

## Tasks

### A. `--max-frame-mb`
- [ ] Promote `MAX_FRAME_BYTES` (`crates/ff-rdp-core/src/transport.rs:153`) to a `pub fn set_max_frame_bytes(usize)` plus a `OnceLock<usize>` (or thread it through `Transport::new` if a constructor knob is preferred).
- [ ] Add `--max-frame-mb <usize>` CLI flag; default 256.
- [ ] Document in the transport module that the receive parser refuses oversized frames before allocating.
- [ ] Test: feed a 200 MiB frame with default cap → bail with `FrameTooLarge`; raise the knob → accept.

### B. Allocation-free dispatch
- [ ] In `crates/ff-rdp-core/src/resources/command.rs:282-332`, refactor `parse_network_resources` and `parse_console_resources` to accept `&[Value]` instead of `&Value`.
- [ ] Remove the per-iteration `json!()` rewrap.
- [ ] Re-run the existing fan-out bench at `command.rs:644-699`; budget unchanged but the new measurement should improve.

### C. Redact threshold
- [ ] Raise `MAX_INLINE_STR` (`transport.rs:45-126`) default to 256.
- [ ] Add `--redact-threshold <bytes>` CLI flag.
- [ ] Sensitive-keyed values (tokens, cookies, auth headers) continue to be redacted regardless of length — the threshold only affects untyped long strings.
- [ ] Test: a long URL renders in full; a long token still renders as `<redacted len=N>`.

## Acceptance Criteria [0/5]

- [ ] `max_frame_mb_knob_works`: with `--max-frame-mb 256`, a 200 MiB frame is accepted; with default 64, it bails.
- [ ] `dispatch_no_per_resource_json_alloc`: `parse_available_resources` no longer calls `json!()` per-resource (assert via heaptrack fixture or `#[allow(clippy::single_match)]`-style structural test).
- [ ] `redact_threshold_tunable`: raising the threshold preserves long URLs; sensitive keys still redact.
- [ ] `bench_resource_dispatch_within_budget`: existing fan-out bench passes its 5 ms budget.
- [ ] `cargo fmt && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace -q` clean.

## Design notes

The `OnceLock` approach for `MAX_FRAME_BYTES` is preferred over plumbing a
constructor parameter through every `Transport::new` call — the knob is
process-global and rarely changes.

`MAX_INLINE_STR = 32` was the right call when traces were short-lived and
mostly aimed at protocol-level debugging; 256 is the right call now that
people use traces to chase per-URL bugs. The default-on redaction for
sensitive keys is unchanged.

## Out of scope

- Streaming bulk-packet support (sending, not just skipping).
- A general-purpose protocol fuzzer (covered by iter-68 fuzz harnesses).

## References

- [[iteration-61q-resource-command-bus]]
- Protocol review report (2026-05-24), §2.5, §4 (parse_available_resources
  allocations, redact threshold)
- `kb/rdp/protocol/transport.md`
