---
title: "Iteration 192: the live-sweep and toolchain watch conditions, carried forward past iteration 178's negative sweep"
type: iteration
date: 2026-08-23
status: in-review
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

## The 2026-08-24 observation

**Two** sweeps were run, both with gates `FF_RDP_LIVE_TESTS=1 FF_RDP_LIVE_NETWORK_TESTS=1` and a
raw `firefox -no-remote --start-debugger-server 6000 --headless` up *before* each run (so the
`preexisting` tier folds into `executed` and `vanished` is observable at all). Sweep A ran against
the tree as it stood at the start of this iteration; sweep B ran against the tree this PR actually
ships, so the numbers below correspond to the diff rather than to something adjacent to it.

```text
A  LIVE_SWEEP_SUMMARY executed=285 skipped=0 preexisting=0 vanished=0 launch_timeout=0 total=285
B  LIVE_SWEEP_SUMMARY executed=285 skipped=0 preexisting=0 vanished=0 launch_timeout=0 total=285
```

285 passed / 0 failed in each, and `passed + failed == executed == 285` both times — the
reconciliation the `iteration-close` skill demands (five of eight sweeps on 2026-08-23 failed it,
with nine tests producing no verdict). **This is the first fully-green sweep in this line**: 178's
had one failure, `live_110_kill_scoping::live_110_replace_never_kills_foreign_firefox`, which
[[iteration-191-stale-launch-record-recycled-pid-kill]] fixed and which passes in both runs here.
All six watched tests — `live_160`, `live_104`, `live_145`, `live_109`, `live_137`, `live_110` —
report `ok` in both.

### The load exposure conditions 6 and 7 were waiting for

[[iteration-188-live-sweep-cost-and-parallelism]] has landed, so the 276-test CLI suite now runs at
`--test-threads=6`. 192 inherited a prediction from its own "Out of scope" section — *"if it lands,
conditions 6 and 7 get their load experiment for free and honestly"* — and that is exactly what
happened. Sampling the 1-minute load average every 10 s across both runs:

| | samples | load range | above 100 | above 190 |
|---|---|---|---|---|
| A | 34 | 4.35 – 224.94 | 30 | 16 |
| B | 30 | 10.98 – 347.96 | 27 | 24 |

Condition 6 asked for a `-j6` run; the sweep *is* one now. Condition 7's original observation was
an unattributed setup failure at load ~190; 40 of these 64 samples sit above that, peaking at
347.96. Both tests passed. No synthetic spinner was run — 177, 178 and 192 all declined to
manufacture load to close a checkbox, and iter-188 made doing so unnecessary.

This does **not** close either condition. One green run is not evidence a load-sensitive defect is
fixed. What changed is narrower and still worth writing down: their load-sensitive arms are no
longer *untested*, they are *routinely exercised and not firing*, which is why
[[iteration-203-live-sweep-watch-conditions-third-holder]] restates their triggers without the
"under `-j6`" / "under contended load" qualifiers instead of copying them forward verbatim.

### Condition 4's polling hunt

64 pid samples across the two sweeps, every one `alive=1 listen=1`. The port-6000 browser survived
a run that peaked at load 347.96. Second and third clean negatives, on one machine.

### What the canary check turned up

Conditions 10 and 11 both rest on `toolchain-watch.yml` running weekly. Checking it produced
something neither row anticipated: **its entire run history is one `workflow_dispatch` on
2026-08-23.** The cron (`0 4 * * 1`) has never fired. The workflow landed in iter-185 (`63b25a4`)
and its first scheduled run was due 2026-08-24 04:00 UTC — still six hours in the future when this
was observed at 2026-08-23 22:14 UTC, so the benign reading is simply "not due yet", and
`live.yml`'s cron has fired every Monday since 2026-07-13, which shows the mechanism works here.

But condition 11's trigger is *two consecutive scheduled runs failing*, and there have been zero.
That trigger is not merely unfired, it is **structurally unobservable** until at least 2026-08-31.
And condition 10 was accepted in iter-185 on the strength of the canary bounding exposure to seven
days — a bound nobody has yet watched hold unattended. Filed as condition 13 in 203, together with
the observation that `live.yml`'s scheduled runs land 46 min – 3 h 26 min after their nominal
03:00 UTC, which makes toolchain-watch's "one hour after `live.yml` so they do not contend"
comment optimistic.

## Out of scope

- Designing a fix for any of the seven ahead of its trigger. Every one of them still lacks the
  evidence a fix would need — that is why they are conditions and not plans.
- Running a `-j6` or synthetic-load experiment to try to force conditions 6 or 7. Both 177 and 178
  recorded, deliberately, that manufacturing load to close a checkbox produces plans for defects
  that were never there. Iteration 188 makes the sweep parallel for its own reasons; if it lands,
  conditions 6 and 7 get their load experiment for free and honestly.
- The `live_110` failure and the stale-record kill path — [[iteration-191-stale-launch-record-recycled-pid-kill]].
- The `~/.ff-rdp` launch-record leak — [[iteration-186-launch-records-leak-one-file-per-port]].

## Acceptance Criteria [12/13]

- [x] Watch condition 1 (`live_160` intermittent failure) has either fired (and been forked into
      its own plan per 178's action) or has not fired since this plan was filed
      [2026-08-24: `ok` in both sweeps — third and fourth consecutive passes (173, 178, 192×2); not
      fired, carried to 203]
- [x] Watch condition 2 (launch-timeout budget) has either fired (and been forked into its own
      plan) or has not fired since this plan was filed
      [2026-08-24: `launch_timeout=0` in both sweeps, at a peak 1-min load of 347.96 — the
      condition's own worst case did not reproduce it; not fired, carried to 203]
- [x] Watch condition 3 (vanished/launch_timeout numbers look wrong) has either fired (and been
      forked into its own plan) or has not fired since this plan was filed
      [2026-08-24: `vanished=0 launch_timeout=0` in both sweeps, and `285 passed + 0 failed ==
      executed=285` each time, so
      the reconciliation the iteration-close skill demands holds. Both counts zero again, so once
      more **only the no-op branches ran** — the live-coverage gap in the condition's heading is
      still untouched. Not fired, carried to 203]
- [x] Watch condition 4 (an unprovoked port-6000 death) has either fired (and the polling hunt run)
      or has not fired since this plan was filed
      [2026-08-24: 64 pid samples at 10 s across the two sweeps, all `alive=1 listen=1`, under a
      load range of 4.35–347.96. Second and third clean negatives, under far heavier contention
      than 178's. Not fired, carried to 203]
- [x] Watch condition 5 (`live_104` daemon timeout) has either fired again (and been forked into
      its own plan) or has not fired since this plan was filed
      [2026-08-24: `ok`. The second arm ("fails once at a 1-minute load average below ~30") was
      again never exercised, because it needs a *failure*. Worth recording that it passed at loads
      up to 347.96, which weakens the starvation hypothesis more than 178's 4.4–28.1 range did.
      Not fired, carried to 203]
- [x] Watch condition 6 (`live_145` under load) has either fired with its message captured (and
      been forked into its own plan) or has not fired since this plan was filed
      [2026-08-24: `ok`, and this time the load-sensitive arm was genuinely exercised —
      [[iteration-188-live-sweep-cost-and-parallelism]] landed, so the 276-test CLI suite ran at
      `--test-threads=6`, which is exactly the `-j6` the condition asked for, obtained from the
      real sweep rather than a synthetic reproduction. Not fired. Still carried to 203: one green
      run is not evidence a load-sensitive defect is fixed]
- [x] Watch condition 7 (`live_109_throttle_block` setup failure under load) has either fired again
      with a captured message (and been forked into its own plan) or has not fired since this plan
      was filed
      [2026-08-24: `ok` in both sweeps. The original observation was an unattributed setup failure
      at load ~190; 40 of the 64 samples sit above 190, peaking at 347.96, and the test passed
      each time. No synthetic spinner was run, per the condition's own instruction. Not fired,
      carried to 203]
- [ ] Watch condition 8 (`live_137_consent_accept_via_daemon` under sweep load) has been forked
      into its own plan, or a second observation has confirmed the poll-timing diagnosis
      — **deliberately left unticked.** `live_137_consent_accept_via_daemon` passed this sweep,
      and a pass is not "a second observation confirming the poll-timing diagnosis"; nor was it
      forked into a plan of its own. Neither arm of this AC was satisfied, so the box stays empty.
      The condition is carried to [[iteration-203-live-sweep-watch-conditions-third-holder]]
      unchanged, with the same note there against ticking it for a mere pass
- [x] Watch condition 9 (port-6000 browser started via `ff-rdp launch` breaks `live_96`) is either
      made impossible by the harness, or the raw-profile instruction is stated where a sweep runner
      will actually read it — **done 2026-08-23** via the second arm: the raw
      `firefox -no-remote --start-debugger-server 6000 --headless` command is now a setup step in
      `.claude/skills/iteration-close/SKILL.md`, with the `ff-rdp launch` substitution named as
      wrong and the four occurrences cited. Not made impossible by the harness — a future runner
      can still substitute, they will just be told not to
- [x] Watch condition 10 (merge-introduced red `main` survives a full canary cycle) has either
      fired (and been forked into its own plan) or has not fired since this plan was filed —
      checked against `gh run list --workflow=toolchain-watch.yml` history
      [2026-08-24: not fired — but the check turned up something the row did not anticipate. The
      canary's entire run history is a single `workflow_dispatch` on 2026-08-23; **no scheduled
      run has ever happened**, so no canary cycle has yet completed and the 7-day exposure bound
      this condition was accepted on is so far untested in practice. Filed as new condition 13 in
      203]
- [x] Watch condition 11 (a red canary unnoticed for a week) has either fired (and been forked into
      its own plan) or has not fired — checked against consecutive canary run conclusions
      [2026-08-24: not fired, and **structurally unobservable** — the trigger needs two consecutive
      *scheduled* runs and there have been zero. Earliest possible observation is 2026-08-31.
      Carried to 203 with the check corrected to require `event: schedule`]
- [x] Watch condition 12 (iter-191's registry-path recycled-PID refusal has no direct test) has
      either been given direct coverage (a `StopDeps` registry-dir override plus a unit test, or a
      live test analogous to `live_110`'s phase B) or has not caused an incident since this plan
      was filed
      [2026-08-24: **closed by the first arm, in this PR.** `StopDeps` gained `registry_dir`, and
      `stop_daemon_and_build_result_with`'s registry reads and removals now route through it. The
      refactor was smaller than this row assumed: `registry::read_registry_in` /
      `remove_registry_in` already existed, so no `std::env::set_var` and no new injection point
      were needed — only the wiring. Four `unit_192_*` tests cover the branch: `Recycled` sends no
      signal and its message never claims a stop happened, `Unknown` (a pre-191 registry with no
      token) and `Confirmed` both still signal — that last one is what proves the token
      *comparison* gates, not merely a token's presence — and the override scopes removal as well
      as read. Not carried to 203]
- [x] Whoever closes this plan has decided, explicitly, whether the still-open conditions get a
      successor holder or are dropped with a written reason — the one question 178 could not answer
      for itself, and the reason this file exists at all
      [2026-08-24: **decision — a successor holder.**
      [[iteration-203-live-sweep-watch-conditions-third-holder]] is filed and validated. Reasoning:
      of the twelve rows, exactly two are genuinely closed (9 by 178's second arm, 12 by this PR's
      code), and the other ten are all in the state "trigger not observed", which is the state this
      mechanism exists to preserve rather than to resolve. Dropping them would repeat precisely the
      failure 178 was filed to prevent. Two rows changed character enough to be worth restating in
      203 rather than copied verbatim: conditions 6 and 7 keep their subject but lose their
      "under `-j6`" / "under contended load" qualifiers, because since iter-188 *every* sweep
      supplies that load, so those arms are now covered by default rather than untested. One new
      row (13) is added for the canary cron that has never fired]

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
