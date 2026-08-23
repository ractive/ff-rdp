---
title: "Iteration 203: the live-sweep and toolchain watch conditions, third holder"
type: iteration
date: 2026-08-24
status: planned
branch: iter-203/live-sweep-watch-conditions-third-holder
depends_on:
  - iteration-192-live-sweep-watch-conditions-carried-forward
first_call_sites: []
dogfood_path: |
  # No code to run. This plan holds trigger conditions until one fires; the
  # dogfood step is reading a sweep's output for the signals below.
  # Start the port-6000 browser FIRST — a raw browser, never `ff-rdp launch`
  # (see .claude/skills/iteration-close/SKILL.md; four agents got this wrong):
  #   firefox -no-remote -profile /tmp/ff-rdp-p6000 --start-debugger-server 6000 --headless
  #   (with devtools.debugger.remote-enabled / prompt-connection / chrome.enabled
  #    in that profile's user.js, or the debug port never opens)
  # Without one, `vanished` can never be non-zero and conditions 3 and 4 are
  # unobservable.
  FF_RDP_LIVE_TESTS=1 FF_RDP_LIVE_NETWORK_TESTS=1 cargo run -p xtask -- live-sweep
  # Read the LIVE_SWEEP_SUMMARY line's `vanished` and `launch_timeout` counts,
  # check `passed + failed == executed`, then grep the log for these five names:
  #   live_160_envelope_honesty::live_160_ref_click_asserts_handler_effect
  #   live_104_security_pwa::live_manifest_fetch_canonical
  #   live_145_error_envelope_completeness::live_145_click_element_not_found_unchanged
  #   live_109_throttle_block::live_throttle_slow3g_slows_fetch
  #   live_137_daemon_mode_parity::live_137_consent_accept_via_daemon
  # Sample the port-6000 pid every 10 s across the sweep (condition 4) and record
  # the 1-minute load range, which is what makes conditions 5-8 readable.
  gh run list --workflow=toolchain-watch.yml --limit=10 \
    --json conclusion,createdAt,event   # conditions 10, 11, 13
tags: [iteration, testing, live-tests, tooling, carry-over]
---

# Iteration 203: still watching, two sweeps later

Third holder in the line [[iteration-178-live-sweep-carryover-watch-conditions]] →
[[iteration-192-live-sweep-watch-conditions-carried-forward]] → here. The mechanism is
unchanged and so is the reason for it: ticking a holder's acceptance criteria closes it, and
closing it would drop every condition it carries unless something folds them forward. Nothing
does that automatically. This file is that fold.

192 answered the one question 178 could not answer for itself — *do the still-open conditions get
a successor holder or are they dropped with a written reason?* — with **yes, a successor**. This
is it.

Two of 192's twelve rows are now closed and are not carried:

- **Condition 9** (port-6000 browser started via `ff-rdp launch` breaks `live_96`) — closed
  2026-08-23 by 178's second arm.
- **Condition 12** (iter-191's registry-path recycled-PID refusal had no direct test) — closed
  2026-08-24 in iteration 192's PR. `StopDeps` grew a `registry_dir` override and four
  `unit_192_*` tests now cover the branch. That was the condition's own second arm, taken.

## The 2026-08-24 negative

Gates `FF_RDP_LIVE_TESTS=1 FF_RDP_LIVE_NETWORK_TESTS=1`, raw port-6000 browser up before the run:

```text
A  LIVE_SWEEP_SUMMARY executed=285 skipped=0 preexisting=0 vanished=0 launch_timeout=0 total=285
B  LIVE_SWEEP_SUMMARY executed=285 skipped=0 preexisting=0 vanished=0 launch_timeout=0 total=285
```

Two runs (A against the tree at the start of 192, B against the tree 192 shipped). 285 passed /
0 failed each, and `passed + failed == executed == 285` both times, so the record reconciles. These
are the first fully-green sweeps in this line — 178's had one failure (`live_110`), which
[[iteration-191-stale-launch-record-recycled-pid-kill]] fixed and which passed in both.

**What changed about the evidence, and it is not nothing:** [[iteration-188-live-sweep-cost-and-parallelism]]
landed, so the 276-test CLI suite now runs at `--test-threads=6`. Across the two sweeps the
1-minute load average ranged 4.35 → 347.96, with 57 of 64 samples above 100 and 40 above 190.
192 predicted exactly this — "if it lands, conditions 6 and 7 get their load experiment for free and honestly" —
and it did. Conditions 6 and 7 are no longer rows whose load-sensitive arm is *untested*; they are
rows whose load-sensitive arm is *routinely exercised by every sweep and has not fired*. That is a
real change in what a green run means for them, and it is why their triggers below are simplified
rather than left as written.

It is **not** a reason to close them. One green run is not evidence a load-sensitive defect is
fixed; that rule is the whole point of this file.

## Carried-forward conditions

Trigger and action text for conditions 1-7 lives in
[[iteration-178-live-sweep-carryover-watch-conditions]] and is not duplicated here.

| # | Condition | Trigger | 2026-08-24 | Why it stays open |
|---|---|---|---|---|
| 1 | `live_160_..._ref_click_asserts_handler_effect` intermittent, cause unknown | it fails a sweep again | `ok` in both runs | Failed iters 168 and 171; passed 173, 178 and now 192 twice. Four consecutive passes, still no cause. On the next failure read `meta.route` / `meta.daemon_fallback` (iter-172) before guessing |
| 2 | Firefox launch-timeout budget may need to scale with sweep load | a sweep reports `launch_timeout>0` | `launch_timeout=0` (both runs) | One observation (iter-170), five clean runs since — now including one at load 347.96, which is the condition's own worst case and did not reproduce it. The count names the tests when it fires; until then there is nothing to size a budget change against |
| 3 | `vanished`/`launch_timeout` runtime paths unit-tested only | a sweep reports non-zero counts that do not reconcile with the log | `vanished=0 launch_timeout=0`; `285 passed + 0 failed == executed=285`, both runs | Third and fourth sweeps in a row where **only the no-op branches ran**. The live-coverage gap is untouched. Forcing it still costs a full sweep plus a deliberately timed kill for a result the unit tests already predict — and note 192 got the reconciliation check itself to pass cleanly, which the 2026-08-23 nine-test verdict gap did not |
| 4 | An unprovoked port-6000 death | a port-6000 browser dies during a sweep on a machine nobody touched | 64 pid samples at 10 s across two runs, all `alive=1 listen=1`, peaking at load 347.96 | Second and third clean negatives, and better ones than 178's: the browser survived far heavier contention this time. Still one machine. If `vanished>0` ever appears with no operator action to blame, run the polling hunt against iteration 173's Theme A suspect list |
| 5 | `live_104_..._manifest_fetch_canonical` daemon-starved past 20 s | it fails again, **or** fails once at a 1-minute load average below ~30 | `ok` in both runs, across loads 4.35–347.96 | Passed, so the second arm was never exercised — a pass still cannot tell "starved" from "wedged". Note the shape of this negative though: it passed *at load 347.96*, which weakens the starvation hypothesis more than 178's 4.4–28.1 range did. Capture the daemon-side timing, not only the client `Timeout` envelope, when it fires |
| 6 | `live_145_..._click_element_not_found_unchanged` 21.9 s under `-j6` | **simplified**: it fails a sweep. The old second arm ("fails under `-j6` with its message captured") is now redundant — every sweep *is* a `-j6` run since iter-188 | `ok` at `--test-threads=6`, both runs | The textbook "one green run is not evidence a load-sensitive defect is fixed" row, except the load arm is now genuinely covered rather than merely asserted. Keep watching; do not manufacture load |
| 7 | `live_109_throttle_block::live_throttle_slow3g_slows_fetch` unattributed setup failure at load ~190 | **simplified**: it (or any test) fails during launch/daemon start with a captured message. The "under contended load" qualifier is now satisfied by default | `ok` in both runs, with 40 of 64 samples above load 190 and a peak of 347.96 | The original observation was at load ~190; these sweeps spent most of their samples above that and the test passed each time. Until a message is captured it remains indistinguishable from condition 2. Still no synthetic spinner — 177, 178 and 192 all declined to invent a reproduction, and iter-188 made that unnecessary |
| 8 | `live_137_daemon_mode_parity::live_137_consent_accept_via_daemon` fails under sweep load with `live_target_count: 0` while `target_count: 1` | it fails a sweep again | `ok` in both runs | Fired once (iter-186's sweep, run 2 of 2); passed 192's. **192 left its AC unticked**: a pass is not "a second observation confirming the poll-timing diagnosis", and it was not forked into its own plan either, so neither arm of that AC was satisfied. Carried unchanged. Next occurrence: compare `uptime_seconds` and `buffer_sizes` at poll time rather than re-confirm the flake |
| 10 | Nothing lints `main` immediately after a merge — two individually-green PRs can merge into a lint-red `main`, and `ci.yml` is `pull_request`-only | a merge-introduced red `main` survives a full weekly canary cycle uncaught | not yet observed — **and see condition 13: no canary cycle has actually completed yet** | Accepted in iter-185 rather than fixed: the weekly canary is supposed to bound exposure to 7 days, and `push: [main]` would double CI cost per merge without having caught the original incident (zero commits involved). DEC-044. The acceptance is only as good as the canary actually running, which is why 13 now sits beside this row |
| 11 | The canary does not alert on failure — it relies on GitHub's default scheduled-failure notification reaching one maintainer | two consecutive scheduled runs fail on the same cause with no intervening fix | **structurally unobservable** — zero scheduled runs exist | Cannot fire until at least two Mondays have passed with the cron firing. Earliest possible observation 2026-08-31. Check with `gh run list --workflow=toolchain-watch.yml --limit=10 --json conclusion,createdAt,event` and confirm `event` is `schedule`, not `workflow_dispatch` |
| 13 | **New (2026-08-24).** `toolchain-watch.yml`'s cron has never fired. Its entire run history is one `workflow_dispatch` on 2026-08-23; the workflow landed in iter-185 (`63b25a4`) and its first scheduled run was still in the future when 192 checked | the first `event: schedule` run of `toolchain-watch.yml` does not appear by 2026-08-25, or any later Monday passes with no scheduled run | first scheduled run due 2026-08-24 04:00 UTC; 192 observed at 2026-08-23 22:14 UTC, six hours early | Cron demonstrably works in this repo — `live.yml` has fired every Monday since 2026-07-13 — so the default expectation is that it simply had not come due. But conditions 10 and 11 both *rest* on this canary running, and nobody has yet seen it do so unattended. One `gh run list` closes this row; it is cheap and it is load-bearing. Related: `live.yml`'s scheduled runs land 46 min – 3 h 26 min after their nominal 03:00 UTC, so toolchain-watch's "one hour after `live.yml` so they do not contend" comment is optimistic — worth a glance if either job starts flaking |

## Out of scope

- Designing a fix for any carried condition ahead of its trigger. Every one still lacks the
  evidence a fix would need — that is why they are conditions and not plans.
- Manufacturing load to force conditions 6 or 7. 177, 178 and 192 all recorded, deliberately, that
  synthetic reproductions produce plans for defects that were never there. Since iter-188 the real
  sweep supplies the load anyway.
- Adding `push: [main]` to `ci.yml` (condition 10's obvious fix). Rejected in iter-185 on cost
  grounds; DEC-044. Reopening that needs the condition to fire, not a preference.
- Forcing the `vanished` / `launch_timeout` branches with a timed kill (condition 3). Costed and
  declined three times now; record the decision rather than re-deriving it.

## Acceptance Criteria [0/12]

- [ ] Watch condition 1 (`live_160` intermittent failure) has either fired (and been forked into
      its own plan) or has not fired since this plan was filed
- [ ] Watch condition 2 (launch-timeout budget) has either fired (and been forked into its own
      plan) or has not fired since this plan was filed
- [ ] Watch condition 3 (vanished/launch_timeout numbers look wrong) has either fired (and been
      forked into its own plan) or has not fired since this plan was filed
- [ ] Watch condition 4 (an unprovoked port-6000 death) has either fired (and the polling hunt run)
      or has not fired since this plan was filed
- [ ] Watch condition 5 (`live_104` daemon timeout) has either fired again (and been forked into
      its own plan) or has not fired since this plan was filed
- [ ] Watch condition 6 (`live_145`) has either fired in a sweep (and been forked into its own
      plan) or has not fired since this plan was filed
- [ ] Watch condition 7 (`live_109_throttle_block` setup failure) has either fired again with a
      captured message (and been forked into its own plan) or has not fired since this plan was
      filed
- [ ] Watch condition 8 (`live_137_consent_accept_via_daemon` under sweep load) has been forked
      into its own plan, or a second observation has confirmed the poll-timing diagnosis —
      **192 could satisfy neither arm and left this unticked; do not tick it for a mere pass**
- [ ] Watch condition 10 (merge-introduced red `main` survives a full canary cycle) has either
      fired (and been forked into its own plan) or has not fired — checked against
      `gh run list --workflow=toolchain-watch.yml`
- [ ] Watch condition 11 (a red canary unnoticed for a week) has either fired (and been forked into
      its own plan) or has not fired — checked against consecutive **scheduled** run conclusions,
      and only tickable once condition 13 confirms scheduled runs exist at all
- [ ] Watch condition 13 (the canary's cron has never fired) is closed by observing an
      `event: schedule` run of `toolchain-watch.yml`, or has fired and been forked into its own
      plan
- [ ] Whoever closes this plan has decided, explicitly, whether the still-open conditions get a
      fourth holder or are dropped with a written reason

As in 178 and 192: none of these can be ticked by inspection. Each needs a sweep that observed the
trigger, or a deliberate decision that the underlying code changed enough that the condition no
longer applies. A tick records that somebody looked — never that a condition was resolved.

## References

- [[iteration-192-live-sweep-watch-conditions-carried-forward]] — direct predecessor
- [[iteration-178-live-sweep-carryover-watch-conditions]] — full trigger and action text for
  conditions 1-7
- [[iteration-173-live-sweep-port-6000-firefox-does-not-survive]] — origin of conditions 1-4
- [[iteration-179-live-62-runner-sees-no-network-events]] — origin of conditions 5 and 6
- [[iteration-177-slow3g-assertion-has-two-percent-headroom]] — origin of condition 7
- [[iteration-186-launch-records-leak-one-file-per-port]] — origin of condition 8
- [[iteration-188-live-sweep-cost-and-parallelism]] — why conditions 6 and 7 now get their load
  exposure from the real sweep
- [[iteration-191-stale-launch-record-recycled-pid-kill]] — the fix whose registry-path coverage
  gap was 192's condition 12, closed there
