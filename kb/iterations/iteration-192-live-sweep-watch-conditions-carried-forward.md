---
title: "Iteration 192: the seven live-sweep watch conditions, carried forward past iteration 178's negative sweep"
type: iteration
date: 2026-08-23
status: planned
branch: iter-192/live-sweep-watch-conditions-carried-forward
depends_on:
  - iteration-178-live-sweep-carryover-watch-conditions
first_call_sites: []
dogfood_path: |
  # No code to run. This plan holds trigger conditions until one fires; the
  # dogfood step is reading a sweep's output for the signals below.
  FF_RDP_LIVE_TESTS=1 FF_RDP_LIVE_NETWORK_TESTS=1 cargo run -p xtask -- live-sweep
  # Read the LIVE_SWEEP_SUMMARY line's `vanished` and `launch_timeout` counts,
  # then grep the log for these four test names:
  #   live_160_envelope_honesty::live_160_ref_click_asserts_handler_effect
  #   live_104_security_pwa::live_manifest_fetch_canonical
  #   live_145_error_envelope_completeness::live_145_click_element_not_found_unchanged
  #   live_109_throttle_block::live_throttle_slow3g_slows_fetch
  # To make conditions 3 and 4 observable at all, a port-6000 browser must be up
  # BEFORE the sweep — without one, `vanished` can never be non-zero:
  #   firefox -no-remote -profile /tmp/ff-rdp-p6000 --start-debugger-server 6000 --headless
  #   (with devtools.debugger.remote-enabled / prompt-connection / chrome.enabled
  #    in that profile's user.js, or the debug port never opens)
tags: [iteration, testing, live-tests, tooling, carry-over]
---

# Iteration 192: still watching, one sweep later

[[iteration-178-live-sweep-carryover-watch-conditions]] held seven conditional
"if this happens again, act on it" items and ran one sweep to see whether any had fired.
**None had.** Ticking its acceptance criteria therefore closes 178 — and closing 178 would drop
all seven, which is precisely the failure mode 178 itself was filed to prevent (iteration 173 was
the last of its run, so nothing folded its rows forward automatically). This plan is that fold,
again.

Nothing here is new evidence. The conditions, their triggers and their prescribed actions are
unchanged from 178; what is added is one negative observation each, and the reason that
observation does not close them.

## The 2026-08-23 negative

Gates `FF_RDP_LIVE_TESTS=1 FF_RDP_LIVE_NETWORK_TESTS=1`:

```text
LIVE_SWEEP_SUMMARY executed=284 skipped=0 preexisting=0 vanished=0 launch_timeout=0 total=284
```

283 passed / 1 failed. The one failure
(`live_110_kill_scoping::live_110_replace_never_kills_foreign_firefox`) was none of these seven
and has its own plan: [[iteration-191-stale-launch-record-recycled-pid-kill]].

## Carried-forward conditions

| # | Condition | Trigger (unchanged) | 2026-08-23 | Why it stays open |
|---|---|---|---|---|
| 1 | `live_160_..._ref_click_asserts_handler_effect` intermittent, cause unknown | it fails a sweep again | `ok` | Failed iteration 168's and 171's sweeps, passed 173's and now 178's. An intermittent that passes twice is still intermittent. On the next failure, read `meta.route` / `meta.daemon_fallback` (added by iteration 172) before guessing |
| 2 | Firefox launch-timeout budget may need to scale with sweep load | a sweep reports `launch_timeout>0` | `launch_timeout=0` | One observation (iteration 170) and three clean runs since. The count names the tests when it fires; until then there is nothing to size a budget change against |
| 3 | `vanished`/`launch_timeout` runtime paths unit-tested only | a sweep reports non-zero counts that do not reconcile with the log | `vanished=0 launch_timeout=0`, counts reconcile | 178 checked the arithmetic (275+1+3+3+2 = `total=284` = `executed`), but with both counts zero **only the no-op branches ran again**. The live-coverage gap in the heading is untouched, and forcing it still costs a full sweep plus a deliberately timed kill for a result the unit tests already predict |
| 4 | An unprovoked port-6000 death | a port-6000 browser dies during a sweep on a machine nobody touched | 117 pid samples at 10 s across the whole sweep, all `alive=1 listen=1` | 178 finally ran iteration 168's dogfood step 3 and it recorded the predicted non-event. One clean negative on one machine is not a guarantee; if `vanished>0` ever appears with no operator action to blame, run the polling hunt against iteration 173's Theme A suspect list |
| 5 | `live_104_..._manifest_fetch_canonical` daemon-starved past 20 s | it fails again, **or** fails once at a 1-minute load average below ~30 | `ok`, under load averages spanning 4.4–28.1 | It passed, so the second arm was never exercised — this run cannot tell "starved" from "wedged" any better than the first. Capture the daemon-side timing, not only the client `Timeout` envelope, when it does fire |
| 6 | `live_145_..._click_element_not_found_unchanged` 21.9 s under `-j6` | it fails in a sweep, or fails under `-j6` with its message captured | `ok` serially | No `-j6` run was made, so the load-sensitive arm is untested. This is the textbook "one green run is not evidence a load-sensitive defect is fixed" row |
| 7 | `live_109_throttle_block::live_throttle_slow3g_slows_fetch` unattributed setup failure at load ~190 | it (or any test) fails during launch/daemon start, with a captured message, under contended load | `ok` | 178 ran no synthetic-spinner load, per this condition's own instruction not to invent a reproduction merely to close the box. Until a message is captured it remains indistinguishable from condition 2 |
| 8 | `live_137_daemon_mode_parity::live_137_consent_accept_via_daemon` fails under sweep load with `live_target_count: 0` while `target_count: 1` | it fails a sweep again | **FIRED 2026-08-23** (iteration 186's sweep): failed run 2, passed run 1, passes in 6.6 s in isolation | Added by [[iteration-186-launch-records-leak-one-file-per-port]]. Load-sensitive by the same shape as conditions 5-7: the daemon has a target but has not yet marked it live when the test polls. "Environmental" is a diagnosis, not a disposition — the message is already captured (`daemon never reported live frame targets`, status envelope inline in the sweep log), so the next occurrence should compare `uptime_seconds` and `buffer_sizes` at poll time rather than re-confirm the flake |
| 9 | Starting the port-6000 browser with `ff-rdp launch` breaks `live_96`'s precondition | anyone reports `live_profiles_prune_removes_all_when_no_firefox_running` failing with "precondition violated ... owned by a live process" | **FIRED 2026-08-23**, self-inflicted, in iteration 186's run 1 | The `preexisting` tier needs a browser on 6000; `ff-rdp launch --debug-port 6000` creates an ff-rdp-**managed** profile, and `live_96` requires no managed profile to have a live owner, so the two requirements are mutually exclusive. This plan's own `dogfood_path` already prescribes the raw `firefox -profile … --start-debugger-server 6000` form with hand-written prefs; run 1 is the empirical proof of why that wording is load-bearing, not a stylistic preference. Re-running that way turned the failure into `ok` |

## Out of scope

- Designing a fix for any of the seven ahead of its trigger. Every one of them still lacks the
  evidence a fix would need — that is why they are conditions and not plans.
- Running a `-j6` or synthetic-load experiment to try to force conditions 6 or 7. Both 177 and 178
  recorded, deliberately, that manufacturing load to close a checkbox produces plans for defects
  that were never there. Iteration 188 makes the sweep parallel for its own reasons; if it lands,
  conditions 6 and 7 get their load experiment for free and honestly.
- The `live_110` failure and the stale-record kill path — [[iteration-191-stale-launch-record-recycled-pid-kill]].
- The `~/.ff-rdp` launch-record leak — [[iteration-186-launch-records-leak-one-file-per-port]].

## Acceptance Criteria [0/10]

- [ ] Watch condition 1 (`live_160` intermittent failure) has either fired (and been forked into
      its own plan per 178's action) or has not fired since this plan was filed
- [ ] Watch condition 2 (launch-timeout budget) has either fired (and been forked into its own
      plan) or has not fired since this plan was filed
- [ ] Watch condition 3 (vanished/launch_timeout numbers look wrong) has either fired (and been
      forked into its own plan) or has not fired since this plan was filed
- [ ] Watch condition 4 (an unprovoked port-6000 death) has either fired (and the polling hunt run)
      or has not fired since this plan was filed
- [ ] Watch condition 5 (`live_104` daemon timeout) has either fired again (and been forked into
      its own plan) or has not fired since this plan was filed
- [ ] Watch condition 6 (`live_145` under load) has either fired with its message captured (and
      been forked into its own plan) or has not fired since this plan was filed
- [ ] Watch condition 7 (`live_109_throttle_block` setup failure under load) has either fired again
      with a captured message (and been forked into its own plan) or has not fired since this plan
      was filed
- [ ] Watch condition 8 (`live_137_consent_accept_via_daemon` under sweep load) has been forked
      into its own plan, or a second observation has confirmed the poll-timing diagnosis
- [ ] Watch condition 9 (port-6000 browser started via `ff-rdp launch` breaks `live_96`) is either
      made impossible by the harness, or the raw-profile instruction is stated where a sweep runner
      will actually read it
- [ ] Whoever closes this plan has decided, explicitly, whether the still-open conditions get a
      successor holder or are dropped with a written reason — the one question 178 could not answer
      for itself, and the reason this file exists at all

As in 178: none of these can be ticked by inspection. Each needs a sweep that observed the trigger,
or a deliberate decision that the underlying code changed enough that the condition no longer
applies. A tick records that somebody looked — never that a condition was resolved.

## References

- [[iteration-178-live-sweep-carryover-watch-conditions]] — direct predecessor; full trigger and
  action text for all seven conditions lives there and is not duplicated
- [[iteration-173-live-sweep-port-6000-firefox-does-not-survive]] — origin of conditions 1-4
- [[iteration-179-live-62-runner-sees-no-network-events]] — origin of conditions 5 and 6
- [[iteration-177-slow3g-assertion-has-two-percent-headroom]] — origin of condition 7
- [[iteration-188-live-sweep-cost-and-parallelism]] — if it lands, conditions 6 and 7 get their
  load exposure from the real sweep rather than a synthetic one
