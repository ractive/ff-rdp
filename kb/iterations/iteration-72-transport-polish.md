---
title: "Iteration 72: Transport polish — frame-size knob, dispatch allocations, redact threshold"
type: iteration
date: 2026-05-24
status: in-progress
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
- [x] Promote `MAX_FRAME_BYTES` to `pub fn set_max_frame_bytes(usize)` + `pub fn max_frame_bytes()` backed by an `AtomicUsize` (`crates/ff-rdp-core/src/transport.rs`); `DEFAULT_MAX_FRAME_BYTES = 256 MiB`.
- [x] Add `--max-frame-mb <usize>` global CLI flag; default 256, applied in `main.rs` after `init_tracing`.
- [x] Documented in the transport module that the receive parser refuses oversized frames before allocating the body buffer.
- [x] Test `max_frame_mb_knob_works`: 2000-byte frame rejected at 1024-byte cap; allowed once cap is raised to 4096.

### B. Allocation-free dispatch
- [x] Added `parse_network_resources_from_items(items: &[Value])` and `parse_console_resources_from_items(items: &[Value])` to `crates/ff-rdp-core/src/actors/watcher.rs`; existing pub fns delegate.
- [x] `ResourceCommand::parse_available_resources` (`resources/command.rs`) now passes the inner items slice directly — no per-resource `json!()` rewrap.
- [x] `bench_bus_dispatch_latency` and `bench_bus_fanout_4_subscribers` still pass the 5 ms median budget on the new code path.

### C. Redact threshold
- [x] `DEFAULT_REDACT_THRESHOLD = 256`; `set_redact_threshold` / `redact_threshold` backed by an `AtomicUsize` in `transport.rs`.
- [x] Added `--redact-threshold <bytes>` global CLI flag (default 256), applied in `main.rs`.
- [x] Sensitive-keyed values (tokens, cookies, auth headers, `text`, `expression`) continue to be redacted regardless of threshold.
- [x] Test `redact_threshold_tunable`: long URL passes through at threshold 512; `authorization` still redacts at the same threshold; same URL is redacted at threshold 16.

## Acceptance Criteria [5/5]

- [x] `max_frame_mb_knob_works`: with the cap raised, an over-default frame is accepted; lowering the cap rejects it with `FrameTooLarge` (`crates/ff-rdp-core/src/transport.rs::max_frame_mb_knob_works`).
- [x] `dispatch_no_per_resource_json_alloc`: structural test (`crates/ff-rdp-core/src/resources/command.rs::dispatch_no_per_resource_json_alloc`) asserts the `parse_available_resources` function body contains no `json!(` call — the only legitimate caller before this iteration.
- [x] `redact_threshold_tunable`: `crates/ff-rdp-core/src/transport.rs::redact_threshold_tunable` — long URL preserved with threshold ≥ url.len(); `authorization` still redacted; tight threshold redacts the URL.
- [x] `bench_bus_dispatch_latency`: median fan-out latency under 5 ms; pair benchmark `bench_bus_fanout_4_subscribers` also under 5 ms. Both in `crates/ff-rdp-core/src/resources/command.rs`.
- [x] `cargo fmt && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace -q` clean.

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
