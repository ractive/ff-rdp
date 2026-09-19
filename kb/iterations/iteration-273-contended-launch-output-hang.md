---
type: iteration
status: planned
date: 2026-09-19
branch: iter-273/contended-launch-output-hang
title: "Iteration 273: contended launch waits reading subprocess output"
depends_on: []
first_call_sites: []
dogfood_path: >-
  FF_RDP_LIVE_TESTS=1 cargo test -p ff-rdp-cli --test live
  live_158_launch_lifecycle::live_158_launch_survives_contended_bind -- --include-ignored --exact
  --nocapture
tags:
  - iteration
  - carry-over
---
# Iteration 273: contended launch waits indefinitely reading subprocess output

## Evidence and scope

Iteration 261's first closing sweep at base `3a398566281abfa48ce96f1f8d855e6c1116a302`
plus its reviewed cleanup-warning change timed out
`live_158_launch_lifecycle::live_158_launch_survives_contended_bind` after 300 seconds
without a verdict. The test launches four Firefox instances concurrently. The watchdog
captured a worker in `Command::output -> read_output -> read_to_end -> read`, while
the test thread joined a worker. This identifies the wait location, not the child or
pipe owner responsible. Four managed Firefox processes were reaped after the kill.

The raw port-6000 browser was absent before this sweep, leaving nine core tests
unexecuted. That separate setup defect does not explain this self-launching test's
hang. The corrected sweep passed the test; that is a positive control, not a fix.

Durable evidence: `.git/ralph-loop/20260919-queue/iter261/logs/live-sweep.log`
and `iter261/watchdog/live_158_launch_survives_contended_bind-1789815577-pid89280.txt`
plus companion `pid88604.txt` in the same run directory. Corrected control is
`iter261/logs/live-sweep-rerun.log`. Independent review R261-1 preserved the finding.

## Tasks [0/3]

- [ ] During a bounded reproduction or recurrence, capture child identities, process tree,
      pipe ownership and command outputs before teardown; distinguish a running command
      from a child inheriting an output pipe.
- [ ] Name the mechanism from failed-occurrence evidence, or record a bounded unreproduced
      outcome without calling the later pass a repair.
- [ ] Apply a scoped repair only if evidence supports one, with a regression that fails
      for that mechanism and preserves launch lifecycle and ownership guarantees.

## Acceptance Criteria [0/3]

- [ ] The original hang and its unknown cause remain traceable to retained stacks and logs.
- [ ] An attributable failed occurrence explains the subprocess-output wait, or the bounded
      investigation explicitly reports the remaining evidence gap without claiming resolution.
- [ ] Any landed repair has a meaningful regression and the required own closing sweep;
      no launch or watchdog bound is widened without measured justification.

## Execution boundary

Filed as iteration 261 carry-over, not selected for the authorized September 19 queue.
Do not execute as part of that queue. This is distinct from iteration 264's three timing
bounds and iteration 260's profile-removal investigation.
