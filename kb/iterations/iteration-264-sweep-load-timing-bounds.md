---
title: "Iteration 264: three live timing bounds fail under sweep load, and nothing distinguishes that from a regression"
type: iteration
date: 2026-09-07
status: planned
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

- [ ] measure each of the three in isolation, ten runs, and record the distribution — a bound
      cannot be re-sized against a single loaded observation
- [ ] measure the same three under a deliberate sweep-like load and record the distribution
- [ ] for each, decide between: a wider bound justified by the isolated distribution; a bound
      expressed against a measured baseline rather than a constant; or a failure message that
      distinguishes "over the bound" from "over the bound while the box was saturated" (e.g.
      capturing load average at assertion time, the way iteration 203's conditions 6 and 7 use it)
- [ ] whatever is chosen, the test must still fail on the regression it was written for —
      demonstrate that, do not assert it

## Acceptance Criteria [0/3]

- [ ] each of the three tests has an isolated and a loaded distribution recorded in this plan
- [ ] each has one of the three dispositions above applied, with the measurement that justifies it
- [ ] for at least `live_navigate_elapsed_matches_wall`, the iter-122 regression it guards is
      re-introduced locally and shown to still fail the reworked assertion

## Notes

- Do not fold these into [[iteration-203-live-sweep-watch-conditions-third-holder]]: 203 holds
  conditions waiting for a first trigger, and all three of these have already fired repeatedly.
  What is missing is a measurement, not another observation.
- `live_237` and `live_navigate_elapsed_matches_wall` failed **all three** sweeps of the
  2026-09-07 run, so neither is a one-off.
