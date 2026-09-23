---
type: iteration
status: done
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

## Tasks [2/3]

- [x] During a bounded reproduction or recurrence, capture child identities, process tree,
      pipe ownership and command outputs before teardown; distinguish a running command
      from a child inheriting an output pipe.
- [x] Name the mechanism from failed-occurrence evidence, or record a bounded unreproduced
      outcome without calling the later pass a repair.
- [ ] Apply a scoped repair only if evidence supports one, with a regression that fails
      for that mechanism and preserves launch lifecycle and ownership guarantees.

## Acceptance Criteria [2/3]

- [x] The original hang and its unknown cause remain traceable to retained stacks and logs.
- [x] An attributable failed occurrence explains the subprocess-output wait, or the bounded
      investigation explicitly reports the remaining evidence gap without claiming resolution.
- [ ] Any landed repair has a meaningful regression and the required own closing sweep;
      no launch or watchdog bound is widened without measured justification.

## Execution boundary

Filed as iteration 261 carry-over, not selected for the authorized September 19 queue.
Do not execute as part of that queue. This is distinct from iteration 264's three timing
bounds and iteration 260's profile-removal investigation.

## Execution clarification — 2026-09-23

The owner authorized the 272–278 queue after parking 268. Earlier dated unselected-run
boundaries are historical; 271 remains excluded and 259/266 constraints remain binding.
Original tasks and acceptance criteria above are unchanged.

Current merged main c761c1b1 retains four Command::output workers joined before cleanup
guards. Distinguish outer test-to-CLI output pipes from separately piped Firefox stderr:
an immediate browser exit can lead to an unbounded stderr read in launch.rs. This is a
hypothesis, not historical attribution. Qualify observers with one long-running direct-
child control and one exited-child/inherited-writer control; add one inner-stderr
surrogate only if testing that hypothesis. Then allow one exact four-launch live attempt
and at most one observation in an otherwise-required closing sweep. Preserve actual
waits, stream completion, process birth identities, pipe writer ownership and scoped
cleanup. Stop answered experiments; passing controls do not establish historical cause.
The original bounded-unreproduced disposition remains available. Prefer 278 first
operationally, without inventing a correctness dependency.

## Bounded outcome — 2026-09-23

The documentation-only investigation is **done as of 2026-09-23**, using the
original bounded-unreproduced route. This completes the permitted investigation;
it does not certify a resolved launch defect. No product mechanism or repair is claimed. The original 261 hang above and
272's separate second-closing-sweep recurrence remain unexplained: 272 timed out
this same outer Command::output worker at 300 seconds, with 347 executed/1 timed_out
of 348 and zero profile leaks. Its stacks, process snapshots and lsof do not join
an actual CLI wait to a surviving writer or establish worker returns. Launch and
contended-test source remained byte-identical to c761c1b1. Iteration 272 remains
blocked; this disposition does not convert its failed sweep into a pass.

One independently admitted diagnostic four-launch attempt passed in 2.868802 seconds:
four CLI exit-zero waits, eight EOFs, eight actual reader joins and four actual worker
joins; four distinct live Firefox identities on 7101–7104; original 30-second product
limit and 300+30-second diagnostic bound unchanged. Initial native discovery matched
each CLI's stdout/stderr writer endpoints before Firefox birth; scanned 580,
unavailable 281, 8 matches, non-atomic. No failed wait or later inherited-writer
mechanism was captured. All four proven browser roots/groups subsequently became
absent; that nonchild cleanup is separate from the recorded waits and joins.
Desktop Firefox remained unchanged. The one attempt is consumed; no retry ran.

This is explicitly a variant of Command::output: independent reaping,
nonblocking readers, identity persistence and observations alter timing. Passing
controls distinguish a running command from an exited command with a surviving
writer, but neither those controls nor this pass attribute either historical
failure. No inner-stderr surrogate was selected. No product source changed, so
an own live sweep is **not applicable to this documentation-only closure**; none
is claimed passed, reused or deferred. The optional observation in an otherwise
required own sweep has no applicable sweep here.

Tasks 1–2 and AC 1–2 are supported by the bounded capture and explicit remaining
gap. Task 3 and AC 3 stay unticked: their conditional product-repair branch was not
triggered, so no product repair, mechanism regression or repair-closing sweep is
asserted. This is a bounded investigation outcome, not a resolved launch defect.

Detailed private history, every failed observer version/control, the two admission
reviews, frozen binaries, raw output and cleanup are retained under
`.git/ralph-loop/20260923-remaining272-278/iter273/`: `live1-outcome-report.md`,
`live1-evidence-manifest.sha256`, `adapter-repair-review-report.md` and
`closure/closure-report.md` and the reviewed carry-over correction in
`closure-repair1/report.md`. New 272 evidence remains under `iter272/closing2/`.
The four live1 profiles were archived with verified recoverable contents and
metadata, then only those exact owned paths were removed after fresh root/group
absence checks. Four older baseline profiles were absent at the final check;
this scoped cleanup did not target them, and their disappearance is not attributed.

## Carry-over

| Observation | Disposition |
| --- | --- |
| Original 261 outer-output hang, cause unknown | **No plan now:** original bounded-outcome route is exhausted without an evidenced repair. Reopen 273 on a new failed occurrence with preserved child birth/actual-wait/stream-writer/worker-return evidence, or newly recovered historical evidence that selects a mechanism; obtain a fresh finite capture schedule before execution. |
| Separate 272 300-second recurrence | **No plan now for 273:** same explicit reopening condition. 272 retains its existing blocked closing work and failed sweep; no criteria or evidence were transferred into a passing result. |
| Unticked conditional task 3/AC 3 | **No plan:** no product repair is justified; the conditional regression/sweep obligations apply only if a mechanism-supported repair is later proposed. |
| Native census unavailable/non-atomic rows and changed diagnostic timing | **No plan:** explicit attribution limits, with all required successful-run waits/EOFs/joins present. A recurrence whose needed identity/holder is unavailable reopens diagnostic admission, not a completeness assumption. |
| Observer 1 allowed 20s plus 5s cleanup | **Closed in this iteration's diagnostic work:** observer2 reserved cleanup inside 20s; both targeted controls passed. |
| Adapter3 transient EPERM cleanup failure | **Closed in diagnostic work:** adapter4 retained unknown until ESRCH; adapter5 also qualified the real 12s deadline within 20s. Original failure retained. |
| Capture1 F1 omitted launched-browser accounting | **Closed in diagnostic work:** adapter8 expected-identity reconciliation; missing-marker owned mock and fresh capture2 zero-findings review. |
| Capture1 F2 observation errors skipped finalization/later profiles | **Closed in diagnostic work:** accumulated errors/all-resource finalization; injected-error and malformed-first-profile mocks, reviewed. |
| Capture1 F3 one unknown group exhausted cleanup time | **Closed in diagnostic work:** simultaneous category cleanup under unchanged total; persistent-unknown-group mock, reviewed. |
| Capture1 F4 cancellation only after WouldBlock; first continuous control nondiscriminating | **Closed in diagnostic work:** every-loop cancellation; one declared successful-read-triggered retest with partial bytes and real joins, reviewed. |
| Profile archive extraction changed quarantine metadata | **Closed in diagnostic work:** original metadata bytes retained; private recovery restored exact metadata/content before owned removal. Earlier failed verification remains recorded. |
| Four preexisting unmarked baseline profiles absent at final check | **Filed:** [[iteration-280-preexisting-profile-disappearance]] tracks the exact four paths, PID74584/no owner-test markers, failed broader post-check and disjoint four-path removal. Timing/actor remain unknown. Planning only, outside272–278; existing-record audit first, with an explicit bounded unattributed route and fresh authorization required for any future live work. |

## Closing validation

Final documentation gates and ordered fmt → strict workspace clippy → normal
parallel workspace tests are recorded with actual commands/exits in the private
`closure/closure-report.md`; affected documentation checks are retained in
`closure-repair1/`. Live dogfood/sweep coverage is not claimed. The authorized
bounded-completion status retains tasks/ACs at2/3 with the conditional third items
unticked. Filing280 closes the carry-over bookkeeping gap without executing its
investigation. Root retains final scoped review and checkpoint/remote actions;
no commit, merge or product repair is claimed here.
