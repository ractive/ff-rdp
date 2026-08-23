---
title: "Iteration 178: seven watch-conditions carried over from live-sweep runs — no plan currently owns them"
type: iteration
date: 2026-08-17
status: in-progress
branch: iter-178/live-sweep-carryover-watch-conditions
depends_on:
  - iteration-173-live-sweep-port-6000-firefox-does-not-survive
  - iteration-179-live-62-runner-sees-no-network-events
  - iteration-177-slow3g-assertion-has-two-percent-headroom
first_call_sites: []
dogfood_path: |
  # This plan has no code to run yet — it exists to hold seven trigger
  # conditions from live-sweep carry-over until one of them fires.
  # The dogfood step, until then, is simply reading the sweep output for the
  # signal each row below names.
  FF_RDP_LIVE_TESTS=1 FF_RDP_LIVE_NETWORK_TESTS=1 cargo run -p xtask -- live-sweep
  # Check the LIVE_SWEEP_SUMMARY line's launch_timeout and vanished counts, and
  # grep the log for these test names:
  #   live_160_envelope_honesty::live_160_ref_click_asserts_handler_effect
  #   live_104_security_pwa::live_manifest_fetch_canonical
  #   live_145_error_envelope_completeness::live_145_click_element_not_found_unchanged
  #   live_109_throttle_block::live_throttle_slow3g_slows_fetch
tags:
  - iteration
  - testing
  - live-tests
  - tooling
  - carry-over
---

# Iteration 178: watch-conditions carried over from live-sweep runs

Iteration 173 ([[iteration-173-live-sweep-port-6000-firefox-does-not-survive]]) fixed
`live-sweep`'s classification of unmet preconditions (a vanished port-6000 browser, a Firefox
launch timeout) and closed most of its own carry-over sweep in place — three of its seven rows
were genuinely resolved (closed in that PR, or already filed elsewhere). The other four rows are
**not** resolved; they are conditional "if this happens again, act on it" items with no next
iteration currently watching for the trigger. Iteration 173 was the last iteration of its run, so
nothing folds these forward automatically — this plan is that fold, filed so the trigger
conditions are not silently lost.

A fifth and a sixth condition were added by
[[iteration-179-live-62-runner-sees-no-network-events]]'s carry-over sweep on 2026-08-22
(`live_104` and `live_145`, conditions 5 and 6 below) — same shape, same reason for being here.
A seventh was added by [[iteration-177-slow3g-assertion-has-two-percent-headroom]]'s carry-over
sweep on 2026-08-23 (condition 7 below) — same shape again.

This plan intentionally does **not** prescribe a fix for any of the seven items — none of them has
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

### 5. `live_104_security_pwa::live_manifest_fetch_canonical` — daemon starved past a 20 s budget under sweep load
Failed [[iteration-179-live-62-runner-sees-no-network-events]]'s sweep (2026-08-22, gates
`FF_RDP_LIVE_TESTS=1 FF_RDP_LIVE_NETWORK_TESTS=1`, `executed=277 skipped=0 preexisting=0
vanished=0 launch_timeout=0 total=277`, 267 passed / 1 failed). It was the sweep's **only**
failure. Verbatim:

```text
manifest must exit 0 (no-manifest is not an error): status=Some(124) stdout={"error":"daemon did
not respond within the timeout after auth — the daemon may be overloaded or the connection is
stale.\nhint: run `ff-rdp daemon stop` then retry, or use --no-daemon.","error_type":"Timeout"}
stderr=
```

Four things are already ruled out, so this is filed as a watch condition rather than a hunt:

- **Not the internet.** The test makes no external request — the page and the manifest are both
  `data:` URLs (`live_104_security_pwa.rs:145`). Nothing leaves the machine.
- **Not daemon startup.** `if ff.with_daemon().is_none() { return; }` makes a missing daemon a
  skip, not a failure. The daemon started, then stopped answering.
- **Not a manifest/PWA defect.** None of the assertions about manifest content fired. The test
  already tolerates Firefox declining the `data:` manifest (it accepts a populated `errors`
  array), and the failure is upstream of all of that — a `--timeout 20000` CLI call returning
  `status=124` with `error_type: "Timeout"`. Nothing about `ManifestActor` is implicated.
- **Not iteration 179's arming race.** This test is daemon-routed, and the daemon holds a standing
  subscription; 179's defect is specific to the per-step `direct` route.

What is left is a fixed time budget losing to machine load — the same family as
[[iteration-177-slow3g-assertion-has-two-percent-headroom]] (2 % margin over a 2.0x threshold) and
[[iteration-179-live-62-runner-sees-no-network-events]] (a 2000 ms watcher-arming window). Load
averages sampled in the same command that recorded `SWEEP_EXIT=1` were 52.41 / 53.69 / 49.51.

**The caveat worth not rounding off:** unlike 177's 2 % margin, 20 seconds is *not* a marginal
budget. Blowing it means the daemon went unanswered for 20+ seconds straight. That is either a
much heavier contention profile than the load average suggests, or a genuine daemon-responsiveness
defect that only contention exposes. One observation cannot tell those apart, which is precisely
why this is a watch condition and not yet a plan.

**Trigger**: this test fails again in any `live-sweep`, **or** it fails once at a 1-minute load
average below ~30 (which would remove contention as the explanation and make it a defect).
**Action then**: file a plan against daemon responsiveness under contention — not against
`manifest`. Capture the daemon's own timing (how long the call was outstanding, and whether the
daemon process was runnable) rather than only the client-side `Timeout` envelope, since the client
envelope cannot distinguish "starved" from "wedged". Two observations would also make it worth
asking whether the 20 s budget in the test is the right knob at all.

## Out of scope

- Designing a fix for any of the five items above ahead of its trigger. Every fix here needs
  evidence this plan does not yet have.
- [[iteration-169-navigate-status-delivery-and-nav-verb-parity]] Theme C's single unexplained
  port-6000 death from iteration 166's sweep — that is a fifth, pre-existing watch condition, but
  it already has an owning plan and is not duplicated here.

### 6. `live_145_error_envelope_completeness::live_145_click_element_not_found_unchanged` — 21.9 s under `-j6`, green serially
Failed during [[iteration-179-live-62-runner-sees-no-network-events]]'s `-j6` load experiment on
2026-08-22 (`FAIL [21.934s] (123/279)`), and **passed** in the serial sweep the same hour. It is
here for the reason the carry-over procedure names explicitly: *one green run is not evidence a
load-sensitive defect is fixed.*

Unlike condition 5, almost nothing is ruled out yet — the `-j6` run was a load generator, not a
measurement, so no envelope was captured and the bound it exceeded has not even been identified.
Recording it as a watch condition is therefore the honest ceiling on what one observation
supports; hunting it now would mean inventing the evidence.

**Trigger**: this test fails in any `live-sweep`, or fails again under `-j6` **with its failure
message captured** (Theme A of iteration 179 means the message will now carry `stdout`, so one
more occurrence should be enough to classify it).
**Action then**: identify the bound it exceeds before proposing any change to it, per
[[iteration-177-slow3g-assertion-has-two-percent-headroom]]'s method — a bound raised without a
measured distribution is the same defect one notch further out.

### 7. `live_109_throttle_block::live_throttle_slow3g_slows_fetch` — one unattributed setup failure under a load average of ~190
Surfaced during [[iteration-177-slow3g-assertion-has-two-percent-headroom]]'s Theme A
under-load measurement on 2026-08-23: run 1 of 11 (10 `yes > /dev/null` spinners on a 10-core
machine, load average ~190) failed **before printing any sample**, i.e. during Firefox launch or
daemon start, not in the delivery-delay assertion this iteration added.

Almost nothing is ruled in or out — the failure's message was not captured, and two further
attempts at the same spinner count did not reproduce it. Iteration 177 recorded it as
"unattributed startup flakiness" and explicitly did not claim it as evidence for or against that
iteration's change (the assertion rewrite touches only how the *test* measures, not how Firefox or
the daemon start up).

**Trigger**: this test (or any other) fails during Firefox launch or daemon start — not during a
timed measurement — with a captured error message, under contended load (synthetic spinners or a
concurrent sweep).
**Action then**: file a plan against launch/daemon-start robustness under heavy contention, using
the captured message. Until a message is captured, this is indistinguishable from watch condition
2 above (a fixed launch-timeout budget losing to load) and no separate hunt is warranted — do not
invent a synthetic-load reproduction attempt merely to close this box.

## Acceptance Criteria [0/7]

- [ ] Watch condition 1 (`live_160` intermittent failure) has either fired (and been forked into
      its own plan per the action above) or has not fired since this plan was filed
- [ ] Watch condition 2 (launch-timeout budget) has either fired (and been forked into its own
      plan) or has not fired since this plan was filed
- [ ] Watch condition 3 (vanished/launch_timeout numbers look wrong) has either fired (and been
      forked into its own plan) or has not fired since this plan was filed
- [ ] Watch condition 4 (an unprovoked port-6000 death) has either fired (and the polling hunt run)
      or has not fired since this plan was filed
- [ ] Watch condition 5 (`live_104` daemon timeout) has either fired a second time (and been forked
      into its own plan per the action above) or has not fired since this plan was filed
- [ ] Watch condition 6 (`live_145` under load) has either fired with its message captured (and
      been forked into its own plan per the action above) or has not fired since this plan was
      filed
- [ ] Watch condition 7 (`live_109_throttle_block` unattributed setup failure under load) has
      either fired again with a captured message (and been forked into its own plan) or has not
      fired since this plan was filed

None of these boxes can be ticked by inspection alone — each requires either a live-sweep run that
observes the trigger and forks a follow-up plan, or a deliberate decision that this plan is
obsolete because the underlying code changed enough that the watch condition no longer applies.
Ticking one speculatively, without an actual sweep run to point at, would be exactly the kind of
premature "done" this repo's discipline rules exist to prevent.

## References

- [[iteration-173-live-sweep-port-6000-firefox-does-not-survive]] — source of watch conditions
  1-4, and the carry-over sweep that filed this plan
- [[iteration-179-live-62-runner-sees-no-network-events]] — source of watch conditions 5 and 6,
  and the iteration whose Theme A made condition 5's failure message readable in the first place
- [[iteration-172-daemon-registry-torn-read-on-autostart]] — added the
  `meta.route`/`meta.daemon_fallback` diagnostic that watch condition 1 depends on
- [[iteration-170-eval-scanner-residual-gaps]] — original iteration where the launch-timeout
  (watch condition 2) was first observed
- [[iteration-169-navigate-status-delivery-and-nav-verb-parity]] — Theme C, the pre-existing fifth
  watch condition this plan does not duplicate
- [[iteration-177-slow3g-assertion-has-two-percent-headroom]] — source of watch condition 7, and
  the plan whose Theme A measurement surfaced it as a side effect
