---
title: "Iteration 264: three live timing bounds fail under sweep load, and nothing distinguishes that from a regression"
type: iteration
date: 2026-09-07
status: done
branch: iter-264/sweep-load-timing-bounds
depends_on:
  - iteration-252-content-process-resources-on-the-direct-route
first_call_sites: []
dogfood_path: |
  # Reproduce by running the full sweep on a loaded box (the state that
  # produced every observation below: 6 sweep workers plus ~20 sibling
  # ff-rdp-managed Firefox processes from concurrent agents):
  #
  #   firefox -no-remote -profile /tmp/ff-rdp-p6000 --start-debugger-server 6000 --headless &
  #   FF_RDP_LIVE_TESTS=1 FF_RDP_LIVE_NETWORK_TESTS=1 cargo run -p xtask -- live-sweep
  #
  # Then run the same three in isolation and compare:
  #
  #   FF_RDP_LIVE_TESTS=1 cargo test-live -p ff-rdp-cli --test live \
  #     live_220_navigating_action_with_page::live_non_navigating_click_with_page_is_not_delayed \
  #     -- --nocapture
  #   … likewise live_237_cancelled_submit_does_not_wait_out_the_timeout
  #   … likewise live_navigate_default_fast::live_navigate_elapsed_matches_wall
tags: [iteration, testing, live-tests, timing, carry-over]
iteration_252_recheck: "2026-09-09 final dual-gate sweep: all three named timing-bound tests passed. Each also passed one subsequent serial isolated rerun with both live env gates enabled. This is one isolated observation per test, not the ten-run isolated and loaded distributions required by this plan; no bound was changed, no historical measurement was erased, and all original ACs remain pending. Exact logs are included in the iteration252 PR evidence package."
iteration_252_review_repair: "The final repair sweep and one new exact isolated rerun passed each of live_non_navigating_click_with_page_is_not_delayed and live_237_cancelled_submit_does_not_wait_out_the_timeout and live_navigate_elapsed_matches_wall. These are additional individual observations rather than the required ten-run isolated/loaded distributions. Original measurements and unticked ACs remain unchanged. Evidence: iter252/review-repair-1/sweep.log and isolated-results.tsv."
---

# Iteration 264: a timing bound that only fails under load says nothing either way

Carry-over from [[iteration-252-content-process-resources-on-the-direct-route]], filed before
that PR merges per CLAUDE.md's carry-over rule.

## The observations

Three live tests assert a wall-clock bound and failed it during iteration 252's sweeps. All
three margins are small — 6 %, 18 % and 32 % over their bounds — which is the problem: the
failure is indistinguishable from the regression each test exists to catch.

| test | bound | observed | sweeps failed (of 3) |
|---|---|---|---|
| `live_220_navigating_action_with_page::live_non_navigating_click_with_page_is_not_delayed` | 2.5 s | 2.659 s | 1 |
| `live_237_act_and_see_timing::live_237_cancelled_submit_does_not_wait_out_the_timeout` | ~2 s | 2.367 s | 3 |
| `live_navigate_default_fast::live_navigate_elapsed_matches_wall` | ±750 ms | delta 992 ms (elapsed_ms 1910 vs wall 2902) | 3 |

Every run was on a machine carrying six sweep workers plus roughly twenty sibling
ff-rdp-managed Firefox processes belonging to other agents on the same working tree.

## Why "environmental" is not a disposition

Each of these tests guards a real behaviour: 220 that the navigation settle loop does not run
when nothing navigated, 237 that a cancelled submission stays on the fast local check, and
`live_navigate_elapsed_matches_wall` that the honest-timing fix of iter-122 Theme B has not
regressed. A bound that a loaded box can cross on its own converts each of those guarantees into
a coin flip: a reader cannot tell a red from a busy machine, so the reflex is to wave it through
— and the next real regression gets waved through with it. That is the same trade
`kb/discipline-rationale.md` describes for the gates iteration 162 deleted.

Widening the bounds is the obvious move and the wrong one on its own: it buys quiet at the cost
of the thing being asserted. What these tests need is a way to say *why* they were slow.

## Scope

- [x] measure each of the three in isolation, ten runs, and record the distribution — a bound
      cannot be re-sized against a single loaded observation
- [x] measure the same three under a deliberate sweep-like load and record the distribution
- [x] for each, decide between: a wider bound justified by the isolated distribution; a bound
      expressed against a measured baseline rather than a constant; or a failure message that
      distinguishes "over the bound" from "over the bound while the box was saturated" (e.g.
      capturing load average at assertion time, the way iteration 203's conditions 6 and 7 use it)
- [x] whatever is chosen, the test must still fail on the regression it was written for —
      demonstrate that, do not assert it

## Acceptance Criteria [3/3]

- [x] each of the three tests has an isolated and a loaded distribution recorded in this plan
- [x] each has one of the three dispositions above applied, with the measurement that justifies it
- [x] for at least `live_navigate_elapsed_matches_wall`, the iter-122 regression it guards is
      re-introduced locally and shown to still fail the reworked assertion

## Iteration 264 measurements — 2026-09-19

Every entry below came from a separate test process and emitted exactly one
`TIMING_SAMPLE` marker after the timed operation. The runner rejected a run unless that marker
was present exactly once, so an exit-zero bind skip could not enter either distribution. Raw
per-run output, argv, environment, UTC start/end and exit status are under
`.git/ralph-loop/20260919-queue/iter264/{isolated,loaded}/`.

The isolated set ran the tests sequentially with no deliberate load process and no foreign
Cargo or managed-Firefox workload present. The host was not idle: its one-minute load average
was 18.13–30.17. Values are sorted milliseconds; the third row measures
`abs(external wall - results.elapsed_ms)`.

| test | isolated distribution (10/10 reached measurement) |
|---|---|
| `live_non_navigating_click_with_page_is_not_delayed` | 117, 119, 127, 129, 134, 163, 163, 167, 191, 220 |
| `live_237_cancelled_submit_does_not_wait_out_the_timeout` | 1322, 1360, 1385, 1393, 1393, 1403, 1419, 1429, 1446, 1448 |
| `live_navigate_elapsed_matches_wall` delta | 179, 190, 191, 192, 194, 194, 195, 197, 200, 227 |

The loaded set used eight run-owned `yes` CPU workers on this 10-logical-core host, held for
the complete sequential 30-run set. This reproduces the CPU scheduling pressure of the six
parallel sweep workers without launching competing Cargo builds or sibling Firefox profiles.
The one-minute load average rose from 18.19 to 102.92. The cleanup trap stopped and waited for
the eight recorded PIDs; its post-cleanup process check was empty.

| test | loaded distribution (10/10 reached measurement) |
|---|---|
| `live_non_navigating_click_with_page_is_not_delayed` | 178, 180, 192, 193, 194, 216, 216, 217, 237, 253 |
| `live_237_cancelled_submit_does_not_wait_out_the_timeout` | 1344, 1386, 1395, 1421, 1427, 1448, 1450, 1454, 1468, 1483 |
| `live_navigate_elapsed_matches_wall` delta | 249, 333, 378, 422, 431, 468, 507, 509, 523, 656 |

### Dispositions

All three use the permitted diagnostic disposition. Their existing bounds remain unchanged:
the largest deliberately loaded observations stayed well below 2500 ms, 2000 ms and 750 ms,
respectively. Each test now records the post-measurement host load-average line and includes it
in any bound failure. Collection happens after the timed operation, so it cannot inflate the
measurement. A future red therefore preserves the behavioral assertion while exposing whether
the host was saturated; load is context, not an automatic waiver.

For the iter-122 sensitivity check, a temporary product mutation replaced plain `navigate`'s
reported `elapsed_ms` with `1`, reproducing the dishonest internal timing shape. The reworked
test reached its measurement (`wall_ms=425`, `reported_ms=1`, `delta_ms=424`) and failed with
exit 101 on the existing lower sanity assertion. The mutation was then removed; the restored
`navigate.rs` SHA-256 is `40ecd8188d6a63eb3967b8bf921af0e0899ebc7cd21eb5c3a6b79356c1e16a46`,
byte-for-byte equal to the branch baseline. Evidence:
`.git/ralph-loop/20260919-queue/iter264/temporary-iter122-mutation.patch` and
`iter122-mutation.log`.

## Closing validation — 2026-09-19

- Firefox 156.0 dual-gate sweep:
  `LIVE_SWEEP_SUMMARY executed=346 skipped=0 preexisting=0 vanished=0 launch_timeout=0 timed_out=0 total=346`
- Profile accounting:
  `LIVE_SWEEP_PROFILES leaked=0 unattributed=0 root=/Users/james/Library/Application Support/ff-rdp/profiles`
- The three timing tests were named passes in that sweep. All nine enumerated xtask `check-*`
  gates passed, followed in order by current-stable `cargo fmt`, strict workspace/all-target
  clippy, and workspace tests.

## Carry-over

No new behavioral failure, diagnostic anomaly, unticked acceptance criterion or deferred item
was produced by this iteration. The pre-existing parked iteration 203 observations and the
unselected backlog remain outside this plan's scope.

## Notes

- Do not fold these into [[iteration-203-live-sweep-watch-conditions-third-holder]]: 203 holds
  conditions waiting for a first trigger, and all three of these have already fired repeatedly.
  What is missing is a measurement, not another observation.
- `live_237` and `live_navigate_elapsed_matches_wall` failed **all three** sweeps of the
  2026-09-07 run, so neither is a one-off.


## Owed242 controls, 2026-09-14

All three named timing tests passed the343-name dual-gate sweep at
`19f4e236a70399c2984e6d46eb15bdac37a55181`. No ten-run isolated/loaded distribution or mutation proof
was performed and no assertion or bound was changed. These are additional
positive controls, not satisfaction of this plan's original ACs. The distinct
169 daemon reload status-null observation is a new watch in203; its21028ms
readystate-fallback-shaped envelope is not folded into these three timing bounds.
Evidence: `.git/ralph-loop/20260912-validation-efficiency/iter242-owed-sweep/sweep.log`.

## Implementation preflight — 2026-09-19

Iteration 246's elapsed-time conclusion has been corrected: external CLI wall time includes connection/teardown and other work outside internal dispatch timing, so contention can widen the gap without dishonest internal measurement. Later passing sweeps do not supply the required distributions. Preserve the specified ten isolated runs per named test, loaded measurements, per-test dispositions and timing-mutation sensitivity check. Prefer measurement after daemon repairs reduce confounding failures. This plan permits measured sweep-like load; the parked203 holder's restrictions on its own watch conditions do not prohibit this plan's experiments.

This source/evidence audit adds implementation guidance, not a new execution result.
Original task and acceptance-criterion wording and checkbox states remain unchanged.
