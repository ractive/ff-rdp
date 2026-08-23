---
title: "Iteration 192: the live-sweep and toolchain watch conditions, carried forward past iteration 178's negative sweep"
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
| 9 | ~~Starting the port-6000 browser with `ff-rdp launch` breaks `live_96`~~ **RESOLVED 2026-08-23** | — | Fired 4x (iters 174, 175, 177, 186) | Not a test-vs-skill conflict. `.claude/skills/iteration-close/SKILL.md` buried the raw-browser command inside a bullet explaining the `preexisting` counter; it now states it as a setup step with the reason. Closed, not carried. See [[iteration-189-content-process-resources-on-the-direct-route]] for the withdrawn mis-diagnosis. |
| 10 | Nothing lints `main` immediately after a merge — two individually-green PRs can merge into a lint-red `main`, and `ci.yml` is `pull_request`-only | a merge-introduced red `main` survives a full weekly canary cycle uncaught | not yet observed | Folded from iteration 194. Accepted in iter-185 rather than fixed: the weekly canary bounds exposure to 7 days, and `push: [main]` would double CI cost per merge without having caught the original incident (zero commits involved). DEC-044 |
| 11 | The canary does not alert on failure — it relies on GitHub's default scheduled-failure notification reaching one maintainer | two consecutive scheduled runs fail on the same cause with no intervening fix | not yet observed | Folded from iteration 194. Check with `gh run list --workflow=toolchain-watch.yml --limit=10 --json conclusion,createdAt` |
| 12 | [[iteration-191-stale-launch-record-recycled-pid-kill]]'s registry-path fix (`daemon stop`'s `Recycled`-PID refusal inside `stop_daemon_and_build_result_with`, the ~50 lines gating the direct kill and the `firefox_pid.unwrap_or(info.pid)` fallback) shipped with no direct unit or live test — only the launch-record branch (`stop_prior_instance_with`) got the three dedicated `unit_191_*` tests and a `live_110` phase B | a live sweep or a real incident exercises the registry-path refusal (correctly or incorrectly) with a captured message, or `StopDeps` grows a `registry_dir` override that makes the branch unit-testable the way `record_dir` already does for the launch record | not yet observed (found during PR #224 review, 2026-08-23) | Iteration 191's own ACs scoped test coverage to `stop_prior_instance_with` only; branch 2 was decided and written up (`DEC-045`) but `registry::read_registry`/`write_registry` read `registry_dir()` directly rather than through an injectable dir like `daemon_record::read_in`'s, so a unit test today would need `std::env::set_var` — a pattern this codebase deliberately avoids (`util/profile_dir.rs`'s `resolve_profile_root` split exists for exactly that reason) — or a small `StopDeps` refactor first. The logic was reviewed by hand at merge time and reads correct; this row exists so an actual incident, or the refactor, is what closes it, not inspection |

## Out of scope

- Designing a fix for any of the seven ahead of its trigger. Every one of them still lacks the
  evidence a fix would need — that is why they are conditions and not plans.
- Running a `-j6` or synthetic-load experiment to try to force conditions 6 or 7. Both 177 and 178
  recorded, deliberately, that manufacturing load to close a checkbox produces plans for defects
  that were never there. Iteration 188 makes the sweep parallel for its own reasons; if it lands,
  conditions 6 and 7 get their load experiment for free and honestly.
- The `live_110` failure and the stale-record kill path — [[iteration-191-stale-launch-record-recycled-pid-kill]].
- The `~/.ff-rdp` launch-record leak — [[iteration-186-launch-records-leak-one-file-per-port]].

## Acceptance Criteria [1/12]

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
- [x] Watch condition 9 (port-6000 browser started via `ff-rdp launch` breaks `live_96`) is either
      made impossible by the harness, or the raw-profile instruction is stated where a sweep runner
      will actually read it — **done 2026-08-23** via the second arm: the raw
      `firefox -no-remote --start-debugger-server 6000 --headless` command is now a setup step in
      `.claude/skills/iteration-close/SKILL.md`, with the `ff-rdp launch` substitution named as
      wrong and the four occurrences cited. Not made impossible by the harness — a future runner
      can still substitute, they will just be told not to
- [ ] Watch condition 10 (merge-introduced red `main` survives a full canary cycle) has either
      fired (and been forked into its own plan) or has not fired since this plan was filed —
      checked against `gh run list --workflow=toolchain-watch.yml` history
- [ ] Watch condition 11 (a red canary unnoticed for a week) has either fired (and been forked into
      its own plan) or has not fired — checked against consecutive canary run conclusions
- [ ] Watch condition 12 (iter-191's registry-path recycled-PID refusal has no direct test) has
      either been given direct coverage (a `StopDeps` registry-dir override plus a unit test, or a
      live test analogous to `live_110`'s phase B) or has not caused an incident since this plan
      was filed
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
- [[iteration-194-toolchain-watch-carryover-conditions]] — folded in here 2026-08-23 as conditions 10 and 11; that plan is now obsolete
- [[iteration-188-live-sweep-cost-and-parallelism]] — if it lands, conditions 6 and 7 get their
  load exposure from the real sweep rather than a synthetic one
