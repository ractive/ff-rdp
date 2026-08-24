---
title: "Iteration 196: FRAME_CAP_LOCK has writers but no readers, so cargo test --workspace is randomly red"
type: iteration
date: 2026-08-23
status: in-review
branch: iter-196/frame-cap-lock-no-readers
depends_on: []
first_call_sites: []
dogfood_path: |
  # 1. Reproduce. Roughly 1 run in 30 fails; raise the loop count if it does not
  #    hit. This reproduces on origin/main — it is NOT introduced by any branch.
  for i in $(seq 1 30); do
    cargo test -q -p ff-rdp-core --lib transport::tests:: -- --test-threads=16 \
      >/tmp/framecap.log 2>&1 || grep -E "FAILED|panicked at" /tmp/framecap.log | head -3
  done
  #    expected TODAY (intermittent):
  #      transport::tests::recv_bulk_with_handler_chunked --- FAILED
  #      panicked at crates/ff-rdp-core/src/transport.rs:2185:
  #        called `Result::unwrap()` on an `Err` value:
  #        BulkFrameTooLarge { announced: 20000, max: 1024 }
  #    expected AFTER: no output across 200 runs

  # 2. See the asymmetry that causes it — five write-lockers, zero read-lockers
  #    among the tests that build frames larger than the caps those writers set:
  grep -n "FRAME_CAP_LOCK" crates/ff-rdp-core/src/transport.rs
  #    expected TODAY: .write() at 5 call sites, .read() only inside the
  #                    self-test that asserts the lock excludes (1471-1493)
  #    expected AFTER: every test whose frame exceeds 1024 bytes holds a read guard

  # 3. The whole suite, repeatedly, is the real acceptance signal:
  for i in $(seq 1 20); do cargo test --workspace -q >/dev/null || echo "RED on run $i"; done
  #    expected AFTER: no output
tags: [iteration, testing, flaky, ff-rdp-core, carry-over]
---

# Iteration 196: the frame-cap lock protects writers from each other and nobody else

## Where this came from

Iteration 187's quality gates. `cargo test --workspace -q` went red once on
`transport::tests::recv_bulk_with_handler_chunked` on a branch whose entire diff is
`crates/xtask/` plus markdown — it touches no `ff-rdp-core` file at all. Chasing that produced a
reproducible race that exists on `origin/main`.

## The defect

`crates/ff-rdp-core/src/transport.rs:190` holds the frame cap in a process-global
`static MAX_FRAME_BYTES_CELL: AtomicUsize`. Tests that need a small cap mutate it and restore it
on drop:

```rust
let _g = FRAME_CAP_LOCK.write().unwrap();
let _restore = FrameCapGuard::new();
set_max_frame_bytes(1024);
```

Five tests do exactly that (lines 1407, 1736, 1764, 2244, 2326). `FRAME_CAP_LOCK` is an
`RwLock<()>`, so those five exclude each other correctly.

**No test ever takes a read guard.** The only `.read()` calls in the file are inside the
self-test at 1471-1493 that asserts the lock excludes as advertised. So every other test that
builds a frame larger than 1024 bytes runs completely unsynchronised against those five writers.
`recv_bulk_with_handler_chunked` builds a 20,000-byte body and calls
`recv_bulk_with_handler_from(...).unwrap()`; when it interleaves with a writer holding the cap at
1024 it gets `BulkFrameTooLarge { announced: 20000, max: 1024 }` and panics on the `unwrap`.

The lock is a half-implemented convention: the writers were made correct and the readers were
never identified. A reader-less `RwLock` is indistinguishable from a `Mutex` between writers,
which is why the discipline looked complete.

## Measured 2026-08-23

| | command | result |
|---|---|---|
| `origin/main` | 30 × `cargo test -p ff-rdp-core --lib transport::tests:: -- --test-threads=16` | 1 failure |
| iter-187 branch | same | 1 failure |
| `origin/main` | 8 × `cargo test -p ff-rdp-core --lib` (default threads) | 0 failures |
| iter-187 branch | 10 × `cargo test -p ff-rdp-core --lib` (default threads) | 0 failures |

At the default thread count it is rare enough to look like cosmic rays; the single observed
failure came during a full `--workspace` run, where xtask's 104 tests and the CLI suites raise
overall parallelism. CI's machine shape is not this machine's, so the observed rate is a lower
bound, not a budget.

## What this iteration must decide

Two shapes, and the choice matters more than the code:

- **Make the readers explicit.** Every test that reads the cap takes `FRAME_CAP_LOCK.read()`.
  Correct, but it is a convention with no enforcement — the next test that builds a big frame
  will forget, and the failure it causes will land on some unrelated branch's PR, exactly as this
  one did.
- **Remove the shared mutable state from the test path.** Thread the cap through the call (a
  parameter, or a `RecvLimits` struct the callers already own) so `set_max_frame_bytes` is not
  needed by tests at all. More diff, but the class of bug goes away instead of being documented.

Prefer the second if the call graph allows it. If it does not, say why, and then the first is
acceptable **only** with the reason written at `FRAME_CAP_LOCK`'s definition so the next author
sees it.

## Themes

- **A — Identify every reader.** Any test whose frame or body exceeds the smallest cap a writer
  sets (currently 100, at line 2330) is a reader, not only the one that has been caught.
- **B — Close the class, not the instance.** Fixing only `recv_bulk_with_handler_chunked` leaves
  the same trap for the next large-frame test.
- **C — Prove it under load.** A fix verified by one green run proves nothing about a race that
  fires once in 30.

## Tasks

### A. Diagnosis [2/2]
- [x] Enumerate every test in `transport.rs` that reads the frame cap with a frame larger than
      100 bytes — the full reader set, not just the observed failure
- [x] Confirm whether any non-test code path reads the cap concurrently with a writer

### B. The fix [2/2]
- [x] Either thread the cap through the call path or give every reader a read guard, with the
      choice and its reasoning recorded at `FRAME_CAP_LOCK`'s definition
- [x] The self-test at 1471-1493 still asserts whatever invariant survives the change

### C. Proof [1/1]
- [x] 200 consecutive runs of `transport::tests:: -- --test-threads=16` are green

## Acceptance Criteria [3/3]

- [x] The repro loop in the dogfood path produces no failures across 200 runs
- [x] The reader set is enumerated in the PR body, so a reviewer can check none was missed
- [x] No `#[serial]`-style blanket serialisation of the whole transport test module — that would
      hide the race rather than remove it, and slow every unrelated test

## Resolution (iter-196)

**Shape chosen: the second** — the cap is threaded through the call path, and the shared mutable
state is gone from the test path. `FRAME_CAP_LOCK` is deleted rather than documented, so the
"acceptable only with the reason written at its definition" fallback does not apply. The reasoning
now lives at `RaisedFrameCap`, the guard that replaced it, and in `kb/decision-log.md` DEC-048
(which supersedes DEC-029).

Note that step 2 of the `dogfood_path` above predicts "expected AFTER: every test whose frame
exceeds 1024 bytes holds a read guard". That line assumed the *first* shape. It is left as written
rather than rewritten to match: no test holds a read guard now, because no test shrinks the cap.
The grep in that step returns nothing at all.

### A — the reader set

A "reader" is any test that parses a frame through an entry point that reads the process-global
cap: `recv_from`, `RdpTransport::recv`, `FramedReader::recv`, `recv_reply_from`, `recv_event_from`,
`recv_bulk_with_handler`, or `actors::network`'s longString fetch. Workspace-wide that is **108
call sites across 38 files**.

In `transport.rs` itself, **26 tests** reach one: `bulk_frame_empty_body_is_handled`,
`bulk_frame_followed_by_json_frame_parses_correctly`, `bulk_frame_returns_bulk_packet_unsupported`,
`bulk_recv_drains_on_actor_mismatch`, `bulk_recv_drains_on_json_peek`, `max_frame_mb_knob_works`,
`recv_bulk_with_handler_actor_mismatch_returns_error`, `recv_bulk_with_handler_chunked`,
`recv_bulk_with_handler_empty_body`, `recv_bulk_with_handler_json_frame_returns_unexpected`,
`recv_bulk_with_handler_kind_mismatch_returns_error`, `recv_errors_on_empty_length_prefix`,
`recv_errors_on_invalid_json_body`, `recv_errors_on_length_prefix_too_long`,
`recv_errors_on_non_digit_in_length_prefix`, `recv_event_from_forwards_non_matching`,
`recv_event_from_matches_predicate`, `recv_event_from_surfaces_error_reply`,
`recv_handles_multi_digit_length`, `recv_parses_valid_frame`,
`recv_reply_from_forwards_sibling_packet`, `recv_reply_from_maps_error_packet`,
`recv_reply_from_rejects_typed_packet_as_reply`, `recv_reply_from_routes_event_to_sink`,
`recv_reply_from_surfaces_daemon_busy_control_error`, `transport_rejects_deep_json`.

Every one of them is a reader by Theme A's definition (a frame over the 100-byte cap
`bulk_recv_caps_drain_length` used to set). The two that exceed the 1024-byte cap the other four
writers set — the ones that could actually go red — are `recv_bulk_with_handler_chunked` (20,000 B
body, the observed victim) and `transport_rejects_deep_json` (1204 B, previously undetected; its
comment even asserted "cap is at least default 256 MiB so the frame fits").

Outside `transport.rs`, four more readers above 1024 B had **no** guard, against the one that did:

| test | frame | guard before |
|---|---|---|
| `specs::types::…::resolve_slot_longstring_grip_fetches_full_value` | 20 KB | `read()` (the only one in the workspace) |
| `actors::page_style::…::parse_computed_properties_resolves_longstring_value` | 20 KB | none |
| `actors::dom_walker::…::parse_dom_node_resolves_longstring_node_value` | 20 KB | none |
| `actors::dom_walker::…::parse_dom_node_resolves_longstring_attr_value` | 15 KB | none |
| `actors::storage::…::parse_cookie_resolves_longstring_value` | 20 KB | none |

That 1-of-5 adoption rate is the argument against the first shape: iter-150 added the read guard to
the one test whose flake it was chasing, and the four written since never took one.

**Non-test code paths** (task A2): `recv_from`, `recv_bulk_with_handler_from` and
`actors::network`'s `parse_response_content` read the cap in production, but the only production
writer is `main.rs:237`, which runs once during argument parsing before any transport exists. There
is no concurrent production writer, which is why this was only ever a test defect.

### B — the fix

- `recv_from_with_cap`, `drain_bulk_frame_with_cap`, `recv_bulk_with_handler_from_with_cap`, and
  `check_outbound_bulk_size(len, cap)` take the cap as an argument; the argument-less forms are
  one-line wrappers passing `max_frame_bytes()`.
- All four cap-shrinking tests now pass a cap. `max_frame_mb_knob_works` keeps its name and its AC,
  split into a pure part and a raise-only part.
- `FRAME_CAP_LOCK` and `FrameCapGuard` are deleted; so is the read guard in `specs::types`.
- `RaisedFrameCap::raise_to` panics below `DEFAULT_MAX_FRAME_BYTES` and holds a mutex against other
  raisers. The surviving invariant — *tests may raise the cap, never shrink it* — is pinned by
  `raised_frame_cap_refuses_to_shrink` (`#[should_panic]`) and
  `raised_frame_cap_restores_previous_value_on_drop`, which replace the deleted lock self-test.

### C — proof

| loop | runs | failures |
|---|---|---|
| `transport::tests:: -- --test-threads=16` | 200 | 0 |
| whole `ff-rdp-core` lib binary `-- --test-threads=16` | 50 | 0 |
| `cargo test --workspace -q` | 20 | 0 |

The first pass of that loop was **not** green: it caught a flake this iteration introduced
(`raised_frame_cap_restores_previous_value_on_drop` snapshotted the cell outside the raise lock,
35 failures in 200). Theme C earned its place.

### Live sweep (2026-08-24)

`FF_RDP_LIVE_TESTS=1 FF_RDP_LIVE_NETWORK_TESTS=1 cargo run -p xtask -- live-sweep` against a raw
headless Firefox on port 6000:

```
LIVE_SWEEP_SUMMARY executed=285 skipped=0 preexisting=0 vanished=0 launch_timeout=0 total=285
```

285 passed / 0 failed, summed across the five `test result:` lines — `P + F == executed`, so the
record reconciles and no test went without a verdict. Zero non-green lines, hence no sweep rows in
the carry-over table below.

## Carry-over

| item | disposition |
|---|---|
| `live_bulk_cap.rs` still calls `set_max_frame_bytes(1024)` on the process-global cell (found while enumerating writers; different test binary, so it cannot redden `cargo test --workspace`) | **file** — [[iteration-207-live-bulk-cap-shrinks-a-process-global]], `check-iteration-plan: OK` |
| `raised_frame_cap_restores_previous_value_on_drop` was flaky at 35/200 when first written | **closed in this PR** — the snapshot moved inside the raise lock; 200/200 green after |
| `transport_rejects_deep_json` (1204 B) was a second unguarded victim in `transport.rs`, never previously identified | **closed in this PR** — it no longer depends on any test-mutated cap; its stale comment was corrected |
| DEC-029's read-guard contract is now unenforceable prose | **closed in this PR** — DEC-029 marked superseded, DEC-048 added |
| Live sweep non-green lines | **no plan** — there were none (285/285). If a later sweep on this code reds a transport test, it needs its own plan. |

## Out of scope

- **The 85 plans failing `check-iteration-plan`.** Separate carry-over,
  [[iteration-195-check-iteration-plan-fails-on-85-of-222-plans]].
- **Auditing other process-global test state.** If this iteration finds more, file it; do not
  absorb it. Found one and filed it:
  [[iteration-207-live-bulk-cap-shrinks-a-process-global]] — `live_bulk_cap.rs` still shrinks the
  cap to 1 KiB inside the *live* test binary.

## References

- `crates/ff-rdp-core/src/transport.rs:190` — `MAX_FRAME_BYTES_CELL`
- `crates/ff-rdp-core/src/transport.rs:1215` — `FRAME_CAP_LOCK`
- `crates/ff-rdp-core/src/transport.rs:1238` — `FrameCapGuard`
- `crates/ff-rdp-core/src/transport.rs:2176` — `recv_bulk_with_handler_chunked`, the observed victim
