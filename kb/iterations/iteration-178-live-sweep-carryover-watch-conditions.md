---
title: "Iteration 178: four watch-conditions carried over from iteration 173's live-sweep fix — no plan currently owns them"
type: iteration
date: 2026-08-17
status: planned
branch: iter-178/live-sweep-carryover-watch-conditions
depends_on:
  - iteration-173-live-sweep-port-6000-firefox-does-not-survive
first_call_sites: []
dogfood_path: |
  # This plan has no code to run yet — it exists to hold four trigger
  # conditions from iteration 173's carry-over sweep until one of them fires.
  # The dogfood step, until then, is simply reading the sweep output for the
  # signal each row below names.
  FF_RDP_LIVE_TESTS=1 FF_RDP_LIVE_NETWORK_TESTS=1 cargo run -p xtask -- live-sweep
  # Check the LIVE_SWEEP_SUMMARY line's launch_timeout and vanished counts,
  # and grep the log for live_160_envelope_honesty::live_160_ref_click_asserts_handler_effect.
tags: [iteration, testing, live-tests, tooling, carry-over]
---

# Iteration 178: watch-conditions carried over from iteration 173

Iteration 173 ([[iteration-173-live-sweep-port-6000-firefox-does-not-survive]]) fixed
`live-sweep`'s classification of unmet preconditions (a vanished port-6000 browser, a Firefox
launch timeout) and closed most of its own carry-over sweep in place — three of its seven rows
were genuinely resolved (closed in that PR, or already filed elsewhere). The other four rows are
**not** resolved; they are conditional "if this happens again, act on it" items with no next
iteration currently watching for the trigger. Iteration 173 was the last iteration of its run, so
nothing folds these forward automatically — this plan is that fold, filed so the trigger
conditions are not silently lost.

This plan intentionally does **not** prescribe a fix for any of the four items — none of them has
enough evidence yet to design one. It exists to be the place a future iteration starts from once
the evidence arrives.

## Watch conditions

### 1. `live_160_envelope_honesty::live_160_ref_click_asserts_handler_effect` — intermittent, cause unknown
Failed iteration 168's and iteration 171's sweeps; passed iteration 173's. Its cause was never
established. iteration 172 added `meta.route` / `meta.daemon_fallback` to the printed diagnostic
specifically so that the *next* failure would carry attribution evidence it did not have before.

**Trigger**: this test fails a `live-sweep` run again.
**Action then**: read the printed `meta.route` / `meta.daemon_fallback` values from the failure
output and file a plan against whatever they attribute the failure to. Do not guess ahead of that
evidence — that is the mistake iteration 172 and 173 were originally filed to correct (both were
filed against a sweep later found to be contaminated by an external kill).

### 2. Firefox launch-timeout budget (`FF_RDP_LIVE_LAUNCH_TIMEOUT_SECS`) may need to scale with sweep load
iteration 170's `live_daemon_autostart_tabless` hit the 30 s launch budget once, under a fully
loaded serial sweep, and passed on every re-run since (including iteration 173's 277/277 sweep,
`launch_timeout=0`). iteration 173 built the reporting (a distinct `launch_timeout=L` count) but
left the budget itself untouched, deliberately, for lack of evidence either way.

**Trigger**: a future `live-sweep` reports `launch_timeout>0`.
**Action then**: the count now names the specific tests, so file a plan that either raises
`FF_RDP_LIVE_LAUNCH_TIMEOUT_SECS` or reduces sweep-time contention, backed by that run's numbers
— evidence that did not exist before iteration 173 added the count.

### 3. `vanished`/`launch_timeout` runtime paths are unit-tested only, never exercised live
iteration 173's own sweep reported `vanished=0 launch_timeout=0` — both new code paths in
`repartition_for_probe` / `classify_failures` ran only their no-op branches during that sweep. The
six deterministic unit tests over captured libtest output are the only coverage of the actual
attribution logic.

**Trigger**: a future sweep reports `vanished>0` (or `launch_timeout>0`, see above) with numbers
that look wrong — e.g. a count that doesn't match the tests actually printed as `ignored`/`FAILED`
in the log, or a `total=T` that doesn't conserve against `executed+skipped+preexisting+vanished+
launch_timeout`.
**Action then**: that is a bug in iteration 173's code, not an environmental artifact — file a
plan against `crates/xtask/src/live_sweep.rs`'s `repartition_for_probe`/`classify_failures`.
Until triggered, no action is warranted: forcing a live demonstration costs a 38-40 minute sweep
plus a deliberately timed `kill` of the operator's own Firefox, for a result the unit tests already
predict.

### 4. Iteration 168's dogfood step 3 (poll the port-6000 pid through a sweep) — deliberately not run
iteration 173's Theme A established the iteration-168 death as an external human kill (three other
suspects ruled out by name and evidence in that plan's Theme A table). Its `dogfood_path` step 3 —
"poll for the pid throughout a sweep and record which phase it dies in" — was therefore left
unticked rather than run as a hunt: against a browser nobody kills, it can only reproduce a
non-event.

**Trigger**: a port-6000 browser dies during a `live-sweep` run on a machine no human touched
during that run (i.e. `vanished>0` is reported and there is no operator action to attribute it
to — distinguish this from watch-condition 3 above, which is about the *counting* being wrong,
not the browser actually dying).
**Action then**: file a plan and run the polling hunt iteration 173 skipped, using the same suspect
list Theme A used (`daemon stop`'s `kill_process_group_force`, a concurrent sweep from another
working tree, Firefox self-exit) plus any new candidate the evidence points at.

## Out of scope

- Designing a fix for any of the four items above ahead of its trigger. Every fix here needs
  evidence this plan does not yet have.
- [[iteration-169-navigate-status-delivery-and-nav-verb-parity]] Theme C's single unexplained
  port-6000 death from iteration 166's sweep — that is a fifth, pre-existing watch condition, but
  it already has an owning plan and is not duplicated here.

## Acceptance Criteria [0/4]

- [ ] Watch condition 1 (`live_160` intermittent failure) has either fired (and been forked into
      its own plan per the action above) or has not fired since this plan was filed
- [ ] Watch condition 2 (launch-timeout budget) has either fired (and been forked into its own
      plan) or has not fired since this plan was filed
- [ ] Watch condition 3 (vanished/launch_timeout numbers look wrong) has either fired (and been
      forked into its own plan) or has not fired since this plan was filed
- [ ] Watch condition 4 (an unprovoked port-6000 death) has either fired (and the polling hunt run)
      or has not fired since this plan was filed

None of these boxes can be ticked by inspection alone — each requires either a live-sweep run that
observes the trigger and forks a follow-up plan, or a deliberate decision that this plan is
obsolete because the underlying code changed enough that the watch condition no longer applies.
Ticking one speculatively, without an actual sweep run to point at, would be exactly the kind of
premature "done" this repo's discipline rules exist to prevent.

## References

- [[iteration-173-live-sweep-port-6000-firefox-does-not-survive]] — source of all four watch
  conditions, and the carry-over sweep that filed this plan
- [[iteration-172-daemon-registry-torn-read-on-autostart]] — added the
  `meta.route`/`meta.daemon_fallback` diagnostic that watch condition 1 depends on
- [[iteration-170-eval-scanner-residual-gaps]] — original iteration where the launch-timeout
  (watch condition 2) was first observed
- [[iteration-169-navigate-status-delivery-and-nav-verb-parity]] — Theme C, the pre-existing fifth
  watch condition this plan does not duplicate
