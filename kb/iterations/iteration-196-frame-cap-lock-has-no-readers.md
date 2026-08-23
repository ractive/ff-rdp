---
title: "Iteration 196: FRAME_CAP_LOCK has writers but no readers, so cargo test --workspace is randomly red"
type: iteration
date: 2026-08-23
status: planned
branch: iter-196/frame-cap-lock-readers
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

### A. Diagnosis [0/2]
- [ ] Enumerate every test in `transport.rs` that reads the frame cap with a frame larger than
      100 bytes — the full reader set, not just the observed failure
- [ ] Confirm whether any non-test code path reads the cap concurrently with a writer

### B. The fix [0/2]
- [ ] Either thread the cap through the call path or give every reader a read guard, with the
      choice and its reasoning recorded at `FRAME_CAP_LOCK`'s definition
- [ ] The self-test at 1471-1493 still asserts whatever invariant survives the change

### C. Proof [0/1]
- [ ] 200 consecutive runs of `transport::tests:: -- --test-threads=16` are green

## Acceptance Criteria [0/3]

- [ ] The repro loop in the dogfood path produces no failures across 200 runs
- [ ] The reader set is enumerated in the PR body, so a reviewer can check none was missed
- [ ] No `#[serial]`-style blanket serialisation of the whole transport test module — that would
      hide the race rather than remove it, and slow every unrelated test

## Out of scope

- **The 85 plans failing `check-iteration-plan`.** Separate carry-over,
  [[iteration-195-check-iteration-plan-fails-on-85-of-222-plans]].
- **Auditing other process-global test state.** If this iteration finds more, file it; do not
  absorb it.

## References

- `crates/ff-rdp-core/src/transport.rs:190` — `MAX_FRAME_BYTES_CELL`
- `crates/ff-rdp-core/src/transport.rs:1215` — `FRAME_CAP_LOCK`
- `crates/ff-rdp-core/src/transport.rs:1238` — `FrameCapGuard`
- `crates/ff-rdp-core/src/transport.rs:2176` — `recv_bulk_with_handler_chunked`, the observed victim
