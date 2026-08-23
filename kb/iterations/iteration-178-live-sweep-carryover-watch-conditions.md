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

## Out of scope

- Designing a fix for any of the seven items above ahead of its trigger. Every fix here needs
  evidence this plan does not yet have.
- [[iteration-169-navigate-status-delivery-and-nav-verb-parity]] Theme C's single unexplained
  port-6000 death from iteration 166's sweep — that is a further, pre-existing watch condition, but
  it already has an owning plan and is not duplicated here.

## Findings — the 2026-08-23 sweep this plan exists to read

Gates: `FF_RDP_LIVE_TESTS=1 FF_RDP_LIVE_NETWORK_TESTS=1`.

```text
LIVE_SWEEP_SUMMARY executed=284 skipped=0 preexisting=0 vanished=0 launch_timeout=0 total=284
SWEEP_EXIT=1
```

283 passed / 1 failed. Machine: 10-core macOS, 1-minute load average 23.78 at sweep start,
sampled between 4.4 and 28.1 across the run (other agents of the same batch were running
concurrently — this was a contended run, not a quiet one).

**A port-6000 Firefox was deliberately started before the sweep** (`-no-remote -profile
/tmp/ff-rdp-iter178-profile --start-debugger-server 6000 --headless`, with the three
`devtools.*` prefs `launch` writes). That is why `preexisting=0` here means *"nothing was deferred
for want of a browser"* rather than *"there are no such tests"*: every tier reported `0 will report
ignored (no Firefox on 6000)`, and all nine `ff-rdp-core` tier tests
(`live_129_frame_targets`, `live_61p_registry`, `live_61u`, `live_firefox_test`) executed and
passed. Without that browser, watch conditions 3 and 4 would have been structurally unobservable —
`vanished` can only be non-zero when a port-6000 browser exists at classification time.

### None of the seven conditions fired

| # | Signal watched | Observed 2026-08-23 | Fired? |
|---|---|---|---|
| 1 | `live_160_..._ref_click_asserts_handler_effect` fails | `... ok` (log line 140) | no |
| 2 | `launch_timeout>0` | `launch_timeout=0` | no |
| 3 | `vanished`/`launch_timeout` counts inconsistent | counts conserve (below) | no |
| 4 | port-6000 browser dies unprovoked | 117 samples, all alive | no |
| 5 | `live_104_security_pwa::live_manifest_fetch_canonical` fails | `... ok` (log line 20) | no |
| 6 | `live_145_..._click_element_not_found_unchanged` fails | `... ok` (log line 103) | no |
| 7 | `live_109_throttle_block::live_throttle_slow3g_slows_fetch` fails at setup | `... ok` (log line 23) | no |

Condition 3's arithmetic, checked against the log rather than trusted: the five tiers classified
275 + 1 + 3 + 3 + 2 = 284 tests, `total=284` conserves against
`executed(284) + skipped(0) + preexisting(0) + vanished(0) + launch_timeout(0)`, and the libtest
result lines account for exactly 284 (274 passed + 1 failed, then 1, 3, 3, 2 passed). Nothing is
inconsistent — but note this run **again** exercised only the no-op branches of
`repartition_for_probe`/`classify_failures`, so the coverage gap named in condition 3's heading is
unchanged; only its trigger is answered.

Condition 4 is the one row where this run produced evidence that did not exist before. Iteration
168's `dogfood_path` step 3 — *poll for the port-6000 pid throughout a sweep and record which phase
it dies in* — had never been run, on the reasoning that against a browser nobody kills it can only
reproduce a non-event. It was run here as cheap instrumentation riding along with a sweep that
needed the browser anyway: 117 samples at 10 s intervals from 10:01:31Z to 10:21:02Z, spanning the
whole sweep, every one of them `alive=1 listen=1`. So the non-event is now measured rather than
assumed, on a run where the browser was actually being connected to by nine tests. That is not
proof no browser ever dies; it is one clean negative on a machine where nobody killed anything.

Condition 5's alternative trigger — *fails once at a 1-minute load average below ~30, which would
remove contention as the explanation* — is **not** answered by this run. `live_104` passed, and it
passed under load averages that spanned the very threshold the condition names, so this run
discriminates nothing about the 20 s budget. It only records a non-firing.

### What the sweep did find: `live_110_replace_never_kills_foreign_firefox`

The sweep's single failure is **not** one of the seven, and it is not a load artifact. Verbatim:

```text
---- live_110_kill_scoping::live_110_replace_never_kills_foreign_firefox stdout ----
thread '...' panicked at crates/ff-rdp-cli/tests/live/live_110_kill_scoping.rs:76:5:
refusal message must explain ff-rdp will not stop an unowned process; got: {"error":"port 51371
is still in use after stopping the prior instance (pid 65225). Run `ff-rdp doctor` or
`lsof -i :51371` to investigate.","error_type":"User"}
```

The test's *core* assertion held — the foreign Firefox was still alive — and `--replace` did fail
as required. What failed is the refusal message, and chasing that turned up a mechanism worth its
own plan: `~/.ff-rdp/launch-record.51371.json`, written 2026-08-16 and never cleaned up, named
pid 65225; that pid had since been recycled onto an unrelated process on this machine (the
operator's `Pencil.app` MCP server, started 2026-08-20). `stop_prior_instance`'s first branch
matched the record on port, saw `is_alive(65225)`, and sent it a SIGTERM/SIGKILL escalation with
`reverify: None` — a signal at a process ff-rdp never launched, reached without ever consulting the
`pid_is_ff_rdp_spawned` ownership proof that guards the port-owner branch below it. Filed as
[[iteration-191-stale-launch-record-recycled-pid-kill]]; the record leak that supplies the stale
file is already owned by [[iteration-186-launch-records-leak-one-file-per-port]] and is not
re-diagnosed here.

## Carry-over

| Row | Disposition |
|---|---|
| `live_110_replace_never_kills_foreign_firefox` FAILED (only non-green line in the sweep) | **file** — [[iteration-191-stale-launch-record-recycled-pid-kill]] |
| Watch conditions 1-7, none fired, all still live going forward | **file** — [[iteration-192-live-sweep-watch-conditions-carried-forward]], the successor holder, since ticking this plan's ACs closes it and nothing else folds them forward |
| `live_62_page_map_index::live_runner_page_map_resolution` — passed serially here, failed iteration 179's `-j6` run | **fold** — already owned by [[iteration-181-playbook-scoped-network-subscription]]; one green serial run is not evidence, and 181 is the fix |
| Condition 3's coverage gap: `vanished`/`launch_timeout` non-zero branches still never run live | **file** — carried in [[iteration-192-live-sweep-watch-conditions-carried-forward]] with the same standing reason (a live demonstration costs a full sweep plus a timed kill, for a result the unit tests predict) |
| `~/.ff-rdp` holds 4 619 files on this machine, one leaked launch record per port | **no plan here, with reason** — owned by [[iteration-186-launch-records-leak-one-file-per-port]]; 178 only measured the count as a side effect of the `live_110` diagnosis |

## Acceptance Criteria [7/7]

Every tick below is answered by one run and names it: the 2026-08-23 sweep,
`FF_RDP_LIVE_TESTS=1 FF_RDP_LIVE_NETWORK_TESTS=1 → LIVE_SWEEP_SUMMARY executed=284 skipped=0
preexisting=0 vanished=0 launch_timeout=0 total=284`, 283 passed / 1 failed. Each is ticked on the
*second* disjunct — "has not fired" — never on the first. None of these conditions is disproven,
and all seven are carried forward in
[[iteration-192-live-sweep-watch-conditions-carried-forward]].

- [x] Watch condition 1 (`live_160` intermittent failure) has either fired (and been forked into
      its own plan per the action above) or has not fired since this plan was filed
      [2026-08-23: `live_160_envelope_honesty::live_160_ref_click_asserts_handler_effect ... ok`]
- [x] Watch condition 2 (launch-timeout budget) has either fired (and been forked into its own
      plan) or has not fired since this plan was filed [2026-08-23: `launch_timeout=0`]
- [x] Watch condition 3 (vanished/launch_timeout numbers look wrong) has either fired (and been
      forked into its own plan) or has not fired since this plan was filed
      [2026-08-23: `vanished=0 launch_timeout=0`; the counts conserve against the per-tier
      classification (275+1+3+3+2) and the libtest result lines. The heading's *other* claim —
      that those branches have never run live — is still true and is carried forward, not ticked
      away here]
- [x] Watch condition 4 (an unprovoked port-6000 death) has either fired (and the polling hunt run)
      or has not fired since this plan was filed [2026-08-23: a port-6000 browser was up for the
      whole sweep and used by nine tests; 117 pid samples at 10 s, all `alive=1 listen=1`;
      `vanished=0`. Iteration 168's dogfood step 3 was finally run, and recorded the non-event it
      was predicted to record]
- [x] Watch condition 5 (`live_104` daemon timeout) has either fired a second time (and been forked
      into its own plan per the action above) or has not fired since this plan was filed
      [2026-08-23: `live_104_security_pwa::live_manifest_fetch_canonical ... ok`. Its *second*
      trigger — a failure at load < ~30, which would rule contention out — is untested: the test
      passed, so this run says nothing about the 20 s budget]
- [x] Watch condition 6 (`live_145` under load) has either fired with its message captured (and
      been forked into its own plan per the action above) or has not fired since this plan was
      filed [2026-08-23: `live_145_error_envelope_completeness::live_145_click_element_not_found_unchanged ... ok`
      in a serial sweep. No `-j6` run was made, so the load-sensitive arm of its trigger is
      untested — this is exactly the "one green run is not evidence" case, hence the carry-forward]
- [x] Watch condition 7 (`live_109_throttle_block` unattributed setup failure under load) has
      either fired again with a captured message (and been forked into its own plan) or has not
      fired since this plan was filed
      [2026-08-23: `live_109_throttle_block::live_throttle_slow3g_slows_fetch ... ok`; no synthetic
      spinner load was run, per the condition's own instruction not to invent one]

The original note on these boxes said none could be ticked by inspection alone — that each needed
"a live-sweep run that observes the trigger and forks a follow-up plan, or a deliberate decision
that this plan is obsolete". The run happened; what it observed was *no* trigger, plus one
unrelated failure that did get its own plan. The ticks therefore record a disposition, not a
resolution: what closes here is this plan's obligation to *look*, not the conditions themselves.

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
- [[iteration-191-stale-launch-record-recycled-pid-kill]] — the defect this plan's own sweep
  turned up, which was not one of the seven
- [[iteration-192-live-sweep-watch-conditions-carried-forward]] — successor holder for all seven
  conditions, filed because ticking the criteria above closes this plan
- [[iteration-186-launch-records-leak-one-file-per-port]] — owns the leaked-record population that
  made the `live_110` failure above possible; deliberately not re-diagnosed here
