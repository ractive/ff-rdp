---
title: "Iteration 184: check-dogfood-script's sentinel is a fixed /tmp path, so two runs of the same iteration race each other"
type: iteration
date: 2026-08-22
status: done
branch: iter-184/dogfood-sentinel-shared-tmp-path
depends_on: []
first_call_sites: []
dogfood_path: |
  # Tooling defect in the discipline gate itself. Found while running
  # iteration 179's quality gates on a machine with several agents sharing one
  # working tree.
  
  # 1. Read the contract. The sentinel path is derived from the iteration
  #    number alone — no PID, no run id, no temp dir:
  #      crates/xtask/src/check_dogfood_script.rs:244
  #      let sentinel = PathBuf::from(format!("/tmp/ff-rdp-iter-{iter_num}-dogfood-ok"));
  #    Every concurrent `check-dogfood-script` for the same N therefore shares
  #    one file, and the gate's own pre-clean deletes it.
  
  # 2. Reproduce the observable symptom — the unit test that exercises the gate
  #    inherits the same fixed path and loses the race:
  #      thread 'check_dogfood_script::tests::xtask_check_dogfood_script_smoke'
  #      panicked: sentinel should exist after successful run
  #    Observed 2026-08-22 under load; passes when run alone.
  for i in 1 2 3 4; do cargo test -q -p xtask --bins & done; wait
  
  # 3. Note what iteration 179 tried and reverted: pointing only the TEST at a
  #    TempDir does not work, because `run_inner` computes the expected path
  #    itself. The gate's contract has to change, and any dogfood script that
  #    writes the sentinel changes with it.
tags:
  - iteration
  - xtask
  - tooling
  - discipline-gates
---

# Iteration 184: give the dogfood sentinel a per-run identity

Carry-over from [[iteration-179-live-62-runner-sees-no-network-events]]'s quality-gate run.

## The defect

`check-dogfood-script` verifies that a plan's `dogfood_script` ran by checking for
`/tmp/ff-rdp-iter-<N>-dogfood-ok`. The path depends only on the iteration number, so:

- two agents running the gate for the same iteration share one sentinel;
- the gate's "pre-clean in case a prior run left it" step deletes a sentinel a *parallel* run has
  just written, and that run then reports a false FAIL;
- a stale sentinel from an earlier crashed run can produce a false PASS.

The false-PASS direction is the serious one: this gate exists to prove a dogfood script really
executed, and a leftover file makes it say yes without anything having run.


### It is not a Windows portability bug — do not "fix" it as one

`PathBuf::from(format!("/tmp/ff-rdp-iter-{n}-dogfood-ok"))` looks like a cross-platform defect
and is not one: `run_script` is `#[cfg(unix)]`, and its `#[cfg(not(unix))]` twin returns SKIP, so
this path never executes on Windows. Verified during 179's review, after 179's own reviewer
initially flagged it as a portability issue.

What is left once that is set aside is still real, and is the defect this plan owns: a hardcoded
absolute path where `std::env::temp_dir()` belongs, and — the part that actually bites — the
false PASS. Iteration 179's `17ae94c` fixed only the *test fixture* by deriving the path from the
pid. The production contract at `crates/xtask/src/check_dogfood_script.rs:244` is untouched and
can still report PASS off a sentinel a concurrent run wrote.

## Themes

- **A — Per-run sentinel.** Pass the expected sentinel path *into* the script (an env var the
  script writes to, chosen fresh per run) instead of deriving it from the iteration number, so two
  runs cannot collide and a stale file cannot satisfy a later run.
- **B — Migrate the writers.** Every `kb/iterations/*.dogfood.sh` that touches the old path has to
  move with the contract, and the gate should fail loudly — not silently pass — on a script still
  writing the old fixed path.

## Tasks

### A. Contract [2/2]
- [x] The sentinel path is unique per run and communicated to the script —
      `run_script` now creates a private directory per invocation
      (`tempfile::Builder` with prefix `ff-rdp-iter-<N>-dogfood-`) and exports its
      `dogfood-ok` path to the script as `FF_RDP_DOGFOOD_SENTINEL`
      (`crates/xtask/src/check_dogfood_script.rs`). `tempfile` moved from
      `[dev-dependencies]` to `[dependencies]` in `crates/xtask/Cargo.toml`.
- [x] A stale sentinel from a previous run cannot satisfy a later run — the gate no
      longer pre-cleans; a freshly created private directory has nothing to clean, and
      the gate bails if the sentinel somehow already exists. Covered by
      `xtask_check_dogfood_script_stale_sentinel_does_not_pass`.

### B. Migration [2/2]
- [x] Every checked-in `*.dogfood.sh` writes the new sentinel — all 16 scripts under
      `kb/iterations/` and all 6 pre-existing lint fixtures under
      `tools/tests/lint-dogfood-script/` now assign
      `SENTINEL="${FF_RDP_DOGFOOD_SENTINEL:?...}"`; `bash tools/lint-dogfood-script.sh
      kb/iterations/*.dogfood.sh` exits 0.
- [x] A script still writing the old fixed path fails the gate rather than passing it —
      new `fixed-sentinel-path` lint rule in `tools/lint-dogfood-script.sh`, fixture
      `tools/tests/lint-dogfood-script/fixed-sentinel-path-bad.sh`, test
      `unit_lint_dogfood_script_flags_fixed_sentinel_path`. Second line of defence: such
      a script writes nothing at `$FF_RDP_DOGFOOD_SENTINEL`, so the run stage FAILs too.

## Acceptance Criteria [2/2]

- [x] Four concurrent `cargo test -p xtask --bins` runs all pass, repeatedly —
      5 rounds x 4 concurrent runs (20 processes) on 2026-08-23, all
      `91 passed; 0 failed`, no panics. The suite now also contains
      `xtask_check_dogfood_script_concurrent_runs_do_not_collide`, which runs 8 gate
      invocations for the same iteration number in parallel threads in-process.
- [x] A hand-planted stale sentinel does not make the gate report success — verified
      end to end with the real binary on 2026-08-23, both directions:
      an unmigrated script + planted `/tmp/ff-rdp-iter-99-dogfood-ok` fails at the lint
      stage (`[fixed-sentinel-path]`), and a lint-clean script that writes nothing fails
      at the run stage with *"script succeeded but wrote no sentinel at
      $FF_RDP_DOGFOOD_SENTINEL=..."* while the planted file is present.

## References

- [[iteration-179-live-62-runner-sees-no-network-events]] — where it surfaced, and where a
  test-only fix was tried and correctly reverted
