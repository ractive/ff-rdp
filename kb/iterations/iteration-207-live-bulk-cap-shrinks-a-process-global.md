---
title: "Iteration 207: live_bulk_cap still shrinks the process-global frame cap for every other live test"
type: iteration
date: 2026-08-24
status: planned
branch: iter-207/live-bulk-cap-process-global
depends_on: [196]
first_call_sites: []
dogfood_path: |
  # 1. The last writer. iter-196 removed every shrink of MAX_FRAME_BYTES_CELL
  #    from the ff-rdp-core unit-test binary; this one survives, in the CLI's
  #    live-test binary:
  grep -rn "set_max_frame_bytes" crates/ff-rdp-cli/tests/
  #    expected TODAY: crates/ff-rdp-cli/tests/live/live_bulk_cap.rs — two calls,
  #                    one of them set_max_frame_bytes(1024)
  #    expected AFTER: no call that lowers the cap

  # 2. The blast radius is a whole test binary, not one test. Anything in the
  #    live suite that parses a frame over 1 KiB while this test holds the cap
  #    gets FrameTooLarge — a screenshot data URL or a longString body does:
  grep -rn "recv_bulk_with_handler\|\.recv()" crates/ff-rdp-cli/tests/live/ | wc -l
  #    expected TODAY: a two-digit count, none of them synchronised with the cap

  # 3. It has already happened once — DEC-022, iter-114, live_console_no_double_delivery
  #    went red on a leaked 1 KiB cap. The RAII guard added then fixes the leak
  #    (sequential), not the window (concurrent).

  # 4. After the fix, the live suite must still prove the cap rejects an
  #    oversized bulk announcement promptly:
  FF_RDP_LIVE_TESTS=1 cargo test-live -p ff-rdp-cli --test live live_bulk_cap
  #    expected: pass, with no process-global mutation in the diff
tags: [iteration, testing, flaky, ff-rdp-cli, live-tests, carry-over]
---

# Iteration 207: the live suite still has the writer iteration 196 removed everywhere else

## Where this came from

Carry-over from [[iteration-196-frame-cap-lock-has-no-readers]]. That iteration removed every
shrink of the process-global frame cap from `ff-rdp-core`'s unit-test binary by giving each parser
a `*_with_cap` form and making `RaisedFrameCap::raise_to` panic on any value below
`DEFAULT_MAX_FRAME_BYTES`. `crates/ff-rdp-cli/tests/live/live_bulk_cap.rs` was left alone
deliberately: it is a different binary, so it cannot make `cargo test --workspace` red, and
iteration 196's plan says to file further process-global state rather than absorb it.

## The defect

```rust
let _cap_guard = FrameCapGuard(max_frame_bytes());
set_max_frame_bytes(1024);
```

`live_bulk_cap.rs` shrinks a process-global to 1 KiB for the duration of its own round-trip. Every
other test in the live binary that parses a frame larger than 1 KiB during that window fails with
`FrameTooLarge` — and live tests parse screenshot data URLs and longString bodies, which are
routinely far larger.

The RAII guard added in iter-114 (DEC-022) restores the cap afterwards, which fixes the *leak*.
It does nothing about the *window*. Today the live suite runs `--test-threads=1`, so the window is
empty in practice — but that is an accident of the runner's flags, not a property of the test, and
[[iteration-198-live-tests-red-only-under-concurrency]] is explicitly about raising live-suite
parallelism.

## What this iteration must decide

`RdpTransport::recv` reads the global cap, so unlike the core tests this one cannot simply pass a
cap to a free function. Two shapes:

- **Per-instance cap on the transport.** `RdpTransport`/`FramedReader` snapshot the cap at
  construction (or take an explicit override), so a test can cap one connection without touching
  the process. Note the review rule: a new `pub` item needs a non-test consumer in the same PR, so
  this only lands honestly if the CLI itself has a reason to set a per-connection cap.
- **Keep the global, prove the property differently.** The AC being defended is "an oversized bulk
  announcement is rejected before any body read, promptly". A cap of 1 KiB is convenient but not
  essential — announcing more than the *default* 256 MiB proves the same thing with no mutation at
  all, at the cost of a larger number in the header (no allocation either way, since the cap check
  precedes the read).

The second is a two-line change and removes the last writer outright. Prefer it unless something
about the 1 KiB cap turns out to be load-bearing.

## Tasks

### A. The fix [0/2]
- [ ] Remove the `set_max_frame_bytes` call from `live_bulk_cap.rs`, keeping the AC it defends
- [ ] Confirm no other file under `crates/*/tests/` mutates the cap

### B. Proof [0/1]
- [ ] `FF_RDP_LIVE_TESTS=1 cargo test-live -p ff-rdp-cli --test live` green, and the oversize
      rejection still measured under 500 ms

## Acceptance Criteria [0/3]

- [ ] `grep -rn "set_max_frame_bytes" crates/*/tests/` returns nothing that lowers the cap
- [ ] `live_bulk_frame_oversize_rejected` still asserts announced-length round-trip, the `max`
      field, and the sub-500 ms rejection
- [ ] The live suite would survive `--test-threads>1` with respect to the frame cap — stated with
      the reason, not just asserted

## Out of scope

- **Raising live-suite parallelism.** That is [[iteration-198-live-tests-red-only-under-concurrency]];
  this iteration only removes one reason it would be unsafe.
- **Other process-global state in the live binary.** File it, do not absorb it.

## References

- `crates/ff-rdp-cli/tests/live/live_bulk_cap.rs:56` — the surviving writer
- `kb/decision-log.md` — DEC-048 (iter-196, supersedes DEC-029), DEC-022 (iter-114 leak)
