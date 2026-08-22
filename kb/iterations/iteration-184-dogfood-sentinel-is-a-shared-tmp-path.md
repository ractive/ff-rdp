---
title: "Iteration 184: check-dogfood-script's sentinel is a fixed /tmp path, so two runs of the same iteration race each other"
type: iteration
date: 2026-08-22
status: planned
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
tags: [iteration, xtask, tooling, discipline-gates]
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

## Themes

- **A — Per-run sentinel.** Pass the expected sentinel path *into* the script (an env var the
  script writes to, chosen fresh per run) instead of deriving it from the iteration number, so two
  runs cannot collide and a stale file cannot satisfy a later run.
- **B — Migrate the writers.** Every `kb/iterations/*.dogfood.sh` that touches the old path has to
  move with the contract, and the gate should fail loudly — not silently pass — on a script still
  writing the old fixed path.

## Tasks

### A. Contract [0/2]
- [ ] The sentinel path is unique per run and communicated to the script
- [ ] A stale sentinel from a previous run cannot satisfy a later run

### B. Migration [0/2]
- [ ] Every checked-in `*.dogfood.sh` writes the new sentinel
- [ ] A script still writing the old fixed path fails the gate rather than passing it

## Acceptance Criteria [0/2]

- [ ] Four concurrent `cargo test -p xtask --bins` runs all pass, repeatedly
- [ ] A hand-planted stale sentinel does not make the gate report success

## References

- [[iteration-179-live-62-runner-sees-no-network-events]] — where it surfaced, and where a
  test-only fix was tried and correctly reverted
