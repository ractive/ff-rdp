---
title: "Iteration 203: the live-sweep and toolchain watch conditions, third holder"
type: iteration
date: 2026-08-24
status: done
branch: iter-203/watch-disposition-20260925
depends_on:
  - iteration-192-live-sweep-watch-conditions-carried-forward
first_call_sites: []
dogfood_path: >-
  Reconcile preserved qualified sweep logs and the final selected iterations’
  required closing sweeps by exact test name, all-tier verdict counts and profile
  summaries. Reuse existing raw-browser identity/listener observations only at their
  recorded scope. Inspect scheduled toolchain-watch conclusions through the stated
  cutoff. This holder requires no additional discovery sweep, induced failure or
  future calendar wait.
tags: [iteration, testing, live-tests, tooling, carry-over]
iteration_252_additional_watch: "2026-09-09: fold post-auth daemon Timeout observations into this watch holder as distinct observations, without claiming condition 5 fired. The diagnostic sweep failed live_109_throttle_block::live_block_url_pattern and live_160_envelope_honesty::live_160_click_reachable_fires_handler; both passed the unpaused final sweep and isolated reruns. The final sweep failed live_160_envelope_honesty::live_160_consent_allow_no_cmp_exits_zero with the same post-auth Timeout envelope; its isolated rerun passed. No source change was made for these rows and no common cause is proved. On recurrence in another unpaused sweep or isolation, capture daemon auth/request/dispatcher timing and file a scoped follow-up rather than changing bounds. The diagnostic tabless-launch failure overlapped the brief paused test phase; that test passed both the final sweep and its independent rerun. Preserve its no-tabs error as a failure, not a launch_timeout reclassification. All original watch ACs remain untouched."
takeover_reconciliation_252: "2026-09-09: scheduled toolchain-watch runs now exist and succeeded on Aug24, Aug31 and Sep7 (latest head9937bf8f94903cccd5d346fcdc1080feb9b95cfc); historical no-scheduled-run premise of conditions11/13 is stale. Migrated iteration-close explicitly requires raw-browser ownership check and stop/wait, satisfying the implementation premise of condition14. Keep holder open and original ACs unticked in this maintenance pass; do not execute its wider backlog. Actual current sweep329/329 names, no leaks/reclassifications; existing repeated-failure conditions and new distinct timeout observations remain recorded."
iteration_252_review_repair: "Final repair sweep plus exact isolated reruns passed the earlier post-auth-timeout rows live_block_url_pattern and live_160_click_reachable_fires_handler and live_160_consent_allow_no_cmp_exits_zero. Tabless autostart also passed both. Existing watch conditions and recurrence/instrumentation triggers remain unchanged; these passes do not establish cause or discharge the watch plan. Evidence: iter252/review-repair-1/sweep.log and isolated-results.tsv."
iteration_252_windows_sweep: "2026-09-12 final Windows compatibility correction sweep: live_240_daemon_frame_desync_and_wedge::live_240_sustained_hops_never_desynchronise failed 1 of 40 hops at hop27 with User: daemon auth failed: recv failed: failed to fill whole buffer; reconnects0. Exact isolated rerun passed all40 hops in36.44s. This is a DISTINCT auth-stage EOF observation, not the existing post-auth Timeout signature, not proof of frame desynchronization, and not a proved common cause. The shared daemon log lacks timestamps/PID attribution and cannot identify this failure. Preserve as an open watch: on recurrence capture auth/request/dispatcher timing and file a scoped follow-up; no original watch condition or AC is marked satisfied. Complete sweep330 names:321pass/9fail, no skips/reclassifications/missing verdicts or profile leaks. Evidence: .git/ralph-loop/20260912-validation-efficiency/pr247-windows-abort/sweep.log and isolated-live_240_daemon_frame_desync_and_wedge.log."
iteration_252_windows_receive_sweep: "2026-09-12: additional post-auth Timeout watch recurred in an unpaused sweep at live_160_envelope_honesty::live_160_type_emits_key_events. Its autostart eval1 failed before typing with exit124 and the existing post-auth Timeout envelope; exact isolation passed4.47s. The recurrence trigger is acknowledged and scoped follow-up267 is filed before merge. Attributable failed-occurrence auth/request/dispatcher timing was not captured and remains an explicit unticked267 requirement; successful isolation does not supply it or prove cause. This is separate from the earlier auth-stage EOF at240hop27, whose test passed this sweep without discharging its watch. No original AC or watch condition is silently reset. Current sweep330=320pass+10fail with no skips/reclassifications/profile leaks. Evidence: pr247-windows-recv/sweep.log and isolated-live_160_type_emits_key_events.log in .git/ralph-loop/20260912-validation-efficiency."
iteration_253_observation_2026_09_12: "Condition 8 fired again: live_137_consent_accept_via_daemon, 15038 ms / 47 polls; target_count=1, live_target_count=0, healthy idle dispatcher at uptime 16 s. Exact isolation passed, which does not close the recurrence. Filed owner remains iteration 262, whose dated 253 observation preserves full status. This sweep reconciled all 332 qualified names (324 passed + 8 failed), no skipped/preexisting/vanished/launch_timeout/timed_out, and no profile leaks. No other watched live-test, post-auth Timeout or pre-auth EOF trigger occurred. Canary/locale conditions were not re-audited by this implementation worker; all existing unmet ACs remain unchanged."
iteration_253_precheckpoint_2026_09_12: "2026-09-12 pre-checkpoint final253 sweep334=322passed+12failed, all five tiers/exact names reconciled, zero skips/reclassifications/profile leaks. This supersedes the earlier253 sweep332 as current evidence. Condition8 recurred: consent target promotion failed15108ms/47polls, PID60752, target1/live0, healthy dispatcher99/99, network buffer438; isolation reached targets37ms then separately failed Sourcepoint consent_not_actioned. Both shapes remain assigned262. Three post-auth Timeout failures are assigned267: live_161_fields_and_sort_reject_unknown_names at autostart eval1, live_219_collection_leaves_the_dom_byte_identical at eval, and live_240_sustained_hops_never_desynchronise at hop28/40. Exact isolations passed; failed-occurrence auth/request/dispatcher timing remains required267. No pre-auth EOF occurred, and the historical hop27 pre-auth EOF remains a distinct unmet watch. NEW ADDITIONAL WATCH: live_styles_applied::live_styles_applied_returns_real_rules successfully navigated to its data fixture using readystate, then styles p --applied failed User/no element matching selector p. Exact isolation passed2.21s. No cause established; the test never calls --with-page and does not execute253's changed collection path. On recurrence in another unpaused sweep or exact isolation, capture navigation envelope, before/after URL, document identity/readyState/DOM, actual route and styles target, and file a scoped diagnostic follow-up. Do not increase waits or infer a style-filter defect. This observation is explicitly folded into203, not discarded for passing isolation. Three264 timing tests passed without supplying the required distributions. Canary/locale conditions not re-audited here; original ACs unchanged. Evidence: .git/ralph-loop/20260912-validation-efficiency/iter253/precheckpoint-edge-verification/sweep.log, observed-results.tsv, profile-accounting.txt, isolation-results.tsv and named isolated logs."
iteration_253_supervisor_watch: "2026-09-12 supervisor reconciliation for 253: the final 334-name sweep repeats condition 8's target-promotion failure; the distinct isolated consent-action failure also remains assigned to 262. Condition 7's broader daemon-start-failure trigger includes live_161_fields_and_sort_reject_unknown_names: autostart eval1 returned a post-auth Timeout. Scoped owner 267 already exists, alongside the two other post-auth observations. Named condition 1 ref-click, condition 5 manifest, condition 6 click-not-found and condition 7 throttle tests passed. No launch_timeout, vanished, timed_out, profile-leak or pre-auth EOF recurrence was observed. No 10-second PID/load-sampling claim is made. Latest scheduled toolchain canaries on Sep 7, Aug 31 and Aug 24 all succeeded (runs 34101762334, 33380953812, 32690383287); no consecutive scheduled red pair, and cron existence is confirmed. These canaries predate today's merges and do not prove post-merge main is green. Migrated iteration-close requires checking ownership first and stopping/waiting afterward; raw browser 56568 was stopped/reaped and port 6000 is free. Preserve every original watch AC and the distinct styles recurrence trigger; this does not close the holder or establish causes."
iteration_253_review_repair_1_2026_09_12: "Current final repair sweep336=328passed+8failed, exact336names across five tiers, no missing verdicts/skips/preexisting/vanished/launch_timeout/timed_out/profile leaks. Condition8 recurred with promotion failure15005ms/47polls, target1/live0, healthy78/78dispatcher and no RPCowner; owner262 preserves exact status. Isolation passed after live readiness113ms, not a fix. The prior three post-auth Timeout rows161/219/240 passed; missing attributable failed-occurrence timing stays required and unticked267. No post-auth Timeout or distinct pre-auth EOF occurred. The new styles missing-selector watch test passed: its recurrence capture-and-file trigger did not fire and remains unchanged, not reset. All three264 timing cases passed without supplying their required distributions. Other named live watch tests passed. No new canary/locale audit or ten-second PID/load sampling claim; supervisor historical reconciliation remains intact, and all original watch ACs remain untouched. Evidence: .git/ralph-loop/20260912-validation-efficiency/iter253/review-repair-1/sweep.log, observed-results.tsv, profile-accounting.txt, isolation-results.tsv. Raw ownedFirefox39150 stopped/reaped143;6000free and unrelateddesktop37270 preserved."
iteration_253_review_repair_2_2026_09_12: "Final253 repair2 sweep338=330passed+8failed, all five tiers/exact names, zero missing/verdict reclassification/skips/profile leaks. Condition8 recurred with promotion15095ms/47polls, daemon40471 target1/live0 and healthy103/103dispatcher. Exact isolation reached targets78ms then separately failed Sourcepoint consent_not_actioned; both distinct outcomes remain262. Prior post-auth Timeout161/219/240 rows passed; no new post-auth Timeout or distinct pre-auth EOF, with missing attributable failed-occurrence timing still required and unticked267. Current styles missing-selector watch test passed; its capture-and-file recurrence trigger remains unchanged, not reset, and no new plan is filed absent recurrence. Named264 elapsed controls passed without supplying required distributions. All original watch/task/AC wording untouched. No ten-secondPID/load sampling or new canary/locale audit claimed; supervisor owns pending-plan reconciliation. RawFirefox36353 stopped/reaped143,6000free; unrelateddesktop37270 preserved. Evidence: .git/ralph-loop/20260912-validation-efficiency/iter253/review-repair-2/sweep.log, observed-results.tsv, watch-results.tsv, profile-accounting.txt and isolation-results.tsv. Earlier336/334closures remain historical."
iteration_254_dogfood_watch: "2026-09-12 iteration254 dogfood first run exited124 after installer lifecycle assertions passed and before writing its sentinel. The original script suppressed navigate stdout and redirected home output into a scratch file removed on teardown, so the failing command/envelope is not attributable; do not label it post-auth267 or prove a cause from this occurrence. The script now prints launch identity, navigate output and home payload. Exact rerun passed with owned Firefox, headed Hook target dogfood and Continue button. No product fix was made. Folded here as a watch: on recurrence retain the exact failing command/envelope, port/PID and route; route explicit post-auth Timeout to267 only with evidence. Original failure remains unresolved. Evidence .git/ralph-loop/20260912-validation-efficiency/iter254/check-dogfood-script-attempt1.log and check-dogfood-script.log."
iteration_254_review_repair_1_2026_09_12: "Final repair sweep338=330pass+8fail, all5tiers/exact names, zero skipped/preexisting/vanished/launch_timeout/timed_out, all5 profile scans clean. Condition7 broader daemon-start trigger fired: live_161_eval_and_flag_strictness::live_161_build_script_matrix_evaluates autostart eval1 exited124 with explicit post-auth Timeout on Firefoxport51734 before matrix assertions. Exact isolation passed8.07s. Folded into existing267 as a new named observation; failed-occurrence attributable auth/request/dispatcher timing remains missing/mandatory, no cause or common mechanism proved. The named styles missing-selector test, consent-promotion test, ref-click, manifest, click-not-found, throttle, contended-bind and sustained-hops tests passed; these do not close existing styles capture/file trigger,262 distinct promotion/Sourcepoint outcomes,267 or pre-auth-EOF watch. First254dogfood124 remains unlocalized/unresolved; current output-retaining customhome/browser dogfood passed without discharging its exact recurrence trigger. No new canary audit or10secondPID/load sampling claim; original ACs untouched. Evidence iter254/review-repair-1/sweep.log, watch-results.txt, profile-accounting.txt and exact isolated logs."
iteration_255_observation_2026_09_13: "The pre-auth connection-loss watch recurred during the unpaused255sweep: live_224_with_page_connection_reset::live_repeated_hop_never_loses_the_connection failed hop7of12 with User daemon auth failed: recv failed: Connection reset by peer (os error54), daemonproxy58890. This is a distinct wire outcome from historical240hop27authEOF and is not post-auth267. The capture-and-file trigger is acknowledged; scoped plan268owns both pre-auth observations and the still-missing attributable failed-occurrence auth/request/dispatcher timing. Exact224isolation passed12hops/0reconnects15.76s, not a closure or proved cause. Condition8targetpromotion and separate140zero-frame recurrence remain262. Existing post-auth267 and styles watches did not recur; their requirements and original ACs stay untouched. Sweep341=331passed+10failed, all5tiers/exact names, no skips/reclassifications/profile leaks. Evidence: .git/ralph-loop/20260912-validation-efficiency/iter255-implementation."
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
| 8 | `live_137_daemon_mode_parity::live_137_consent_accept_via_daemon` fails under sweep load with `live_target_count: 0` while `target_count: 1` | it fails a sweep again | **FIRED 2026-09-07** (iter-252's sweep): failed with `LIVE_TARGET_WAIT port=64640 reached=false elapsed_ms=15116 polls=48 bound_ms=15000`, and `live_145_error_envelope_completeness::live_145_click_frame_scan_js_exception_envelope` failed the same way in the same run. The shape is now owned by [[iteration-262-daemon-live-target-never-promoted]], filed for exactly this; this row records the third observation and the second affected test rather than duplicating that plan. Earlier: `ok` in both of 192's runs | Fired once (iter-186's sweep, run 2 of 2); passed 192's. **192 left its AC unticked**: a pass is not "a second observation confirming the poll-timing diagnosis", and it was not forked into its own plan either, so neither arm of that AC was satisfied. Carried unchanged. Next occurrence: compare `uptime_seconds` and `buffer_sizes` at poll time rather than re-confirm the flake |
| 10 | Nothing lints `main` immediately after a merge — two individually-green PRs can merge into a lint-red `main`, and `ci.yml` is `pull_request`-only | a merge-introduced red `main` survives a full weekly canary cycle uncaught | not yet observed — **and see condition 13: no canary cycle has actually completed yet** | Accepted in iter-185 rather than fixed: the weekly canary is supposed to bound exposure to 7 days, and `push: [main]` would double CI cost per merge without having caught the original incident (zero commits involved). DEC-044. The acceptance is only as good as the canary actually running, which is why 13 now sits beside this row |
| 11 | The canary does not alert on failure — it relies on GitHub's default scheduled-failure notification reaching one maintainer | two consecutive scheduled runs fail on the same cause with no intervening fix | **structurally unobservable** — zero scheduled runs exist | Cannot fire until at least two Mondays have passed with the cron firing. Earliest possible observation 2026-08-31. Check with `gh run list --workflow=toolchain-watch.yml --limit=10 --json conclusion,createdAt,event` and confirm `event` is `schedule`, not `workflow_dispatch` |
| 13 | **New (2026-08-24).** `toolchain-watch.yml`'s cron has never fired. Its entire run history is one `workflow_dispatch` on 2026-08-23; the workflow landed in iter-185 (`63b25a4`) and its first scheduled run was still in the future when 192 checked | the first `event: schedule` run of `toolchain-watch.yml` does not appear by 2026-08-25, or any later Monday passes with no scheduled run | first scheduled run due 2026-08-24 04:00 UTC; 192 observed at 2026-08-23 22:14 UTC, six hours early | Cron demonstrably works in this repo — `live.yml` has fired every Monday since 2026-07-13 — so the default expectation is that it simply had not come due. But conditions 10 and 11 both *rest* on this canary running, and nobody has yet seen it do so unattended. One `gh run list` closes this row; it is cheap and it is load-bearing. Related: `live.yml`'s scheduled runs land 46 min – 3 h 26 min after their nominal 03:00 UTC, so toolchain-watch's "one hour after `live.yml` so they do not contend" comment is optimistic — worth a glance if either job starts flaking |
| 14 | **New (2026-08-24).** The `iteration-close` skill tells a sweep runner to start a raw port-6000 Firefox and never tells them to stop it. Three were found alive at the start of 192 — `/tmp/ff-rdp-sweep188-profile`, `/tmp/ff-rdp-sweep188b-profile`, `/tmp/ff-rdp-sweep-6000-profile` — left by earlier iterations' runners. Only one can hold the port; the other two were inert processes competing for it | a sweep runner finds a port-6000 browser they did not start, **or** a sweep's `preexisting`/`vanished` counts are misread because the browser on the port belonged to a previous run | 3 found and reaped by hand before 192's sweeps | Not a product defect — `live_151_*` and `live_146_no_orphan_firefox_after_suite` both pass, so nothing ff-rdp *launches* is leaking; these were started by hand, by the skill's own instruction. The risk is interpretive rather than resource: a stale browser on port 6000 satisfies the sweep's probe, so the `preexisting` tier runs against a profile whose prefs and state nobody checked. Cheapest fix is one line in `.claude/skills/iteration-close/SKILL.md` telling the runner to kill the browser afterwards and to check for an existing one first; deliberately not done here because 192 was a watch-holder and editing a skill mid-loop is what condition 9's history warns about |

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

## Acceptance Criteria [0/13]

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
- [ ] Watch condition 14 (hand-started port-6000 browsers outliving their sweep) has either fired
      again, or the `iteration-close` skill has grown the stop/check-first instruction that makes
      it unnecessary
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


## Owed242 sweep reconciliation and new reload watch, 2026-09-14

Sweep343=331pass+12fail at `19f4e236a70399c2984e6d46eb15bdac37a55181` reconciles every compiled
ignored name across all five tiers, with zero skips/reclassifications/profile
leaks. Condition6's named145 click-not-found test failed with the measured
promotion signature; its current owner is262 alongside a distinct140 zero-frame
recurrence. The137 consent test passed, so no137 recurrence is claimed here.
New named164 post-auth Timeout is filed under267; no pre-auth268 recurrence.
BBC144 `consent_no_cmp` recurred in exact isolation and has its own proposed
[[iteration-271-bbc-consent-no-cmp-recurrence]] diagnostic follow-up; it is neither
HN's diagnosed mechanism nor the ready-target Sourcepoint action failure.

**New separate watch:** `live_169_nav_verb_status_parity::live_169_nav_verbs_report_status_daemon`
failed reload of local `/b`: ready_state complete, elapsed_ms21028,
status null/status_reason not_observed; expected200. Firefox63163/debug58577,
proxy58738, fixture58749. Exact isolation passed8.63s (command wall8.86s).
The payload is consistent with the documented events-budget/readystate fallback
in `navigate::wait_for_navigation_commit`, not evidence that HTTP returned no
status. No failed-occurrence event, document identity, route or dispatcher trace
establishes why fallback was reached. This daemon occurrence is distinct from
historical174's fixed direct-route prelude and from267's explicit Timeout.
On recurrence in another unpaused sweep or exact isolation, capture per-client
and daemon event/subscription timing, route, document identity, actual network
status and fallback reason, and file a scoped diagnostic plan before changing
waits. Passing isolation does not reset this watch or prove load was the cause.

Existing styles and unlocalized254 dogfood watches remain unchanged. Named
ref-click, manifest, throttle, contended-bind and sustained-hop controls passed;
no canary/locale audit or full-run ten-second sampling is claimed. Partial PID/load
samples begin after startup and show the owned raw browser available through the
sweep. It was stopped and waited for after isolations; desktop37270 remained.
All original watch ACs stay untouched. Evidence: `.git/ralph-loop/20260912-validation-efficiency/iter242-owed-sweep/`.

## Iteration257 watch reconciliation, 2026-09-14

The screenshot fix's own closing sweep reconciled344 exact names across five
tiers:342pass/2fail, zero missing/extra/duplicate names, skips or reclassifications.
All five per-tier profile scans and the one final profile summary were clean.
All seven screenshot regressions and the new direct drawSnapshot guard passed.

The named137 Guardian test failed **after** live-target readiness106ms/1poll,
with `consent_no_cmp`, not promotion failure or detected-not-actioned. Its exact
isolation accepted Sourcepoint after readiness18ms, passing5.09s. This new
distinct detection outcome is explicitly folded into262's existing named137
requirement, with no failed-page/banner evidence or cause claimed. The BBC144
direct-route no-CMP test failed both sweep and exact isolation4.77s, and remains
owned by271. Identical error types do not establish a shared cause across sites.

The owed242 reload169 watch test passed; styles, manifest, ref-click, both145,
throttle, contended-bind, and sustained-hop controls also passed. No post-auth267
or pre-auth268 observation recurred. These negatives do not reset the watches,
supply missing failed-occurrence timing, diagnose load, or close their owners.
The three264 timing controls passed without providing required distributions.
No new canary or non-English locale audit is claimed. RawFirefox47188 was sampled
with PID/listener/load every ten seconds throughout the sweep, then stopped and
waited for. A later supervisor check at2026-09-14 01:00:43 CEST found no port6000
listener and desktop37270 still alive, with its original September8 start time.
Exact commands, exits and output are retained in
`.git/ralph-loop/20260912-validation-efficiency/iter257-supervisor-verification/post-cleanup-processes.json`;
no continuous-availability claim is made. Original watch ACs remain
untouched. Evidence: `.git/ralph-loop/20260912-validation-efficiency/iter257-implementation/`,
including `observed-verdicts.tsv`, both isolated logs and `sweep-pid-load-samples.txt`.

## Iteration261 styles recurrence — 2026-09-19

Documentation only; this holder remains parked. The first 261 sweep emitted
`live_styles_applied::live_styles_applied_returns_real_rules ... FAILED`, but its
panic body was not emitted before an unrelated contended-launch watchdog termination.
No failed-page, URL, route or error category was captured; the cause is unknown.
The corrected sweep passed this test, which does not diagnose or close the watch.
The existing frontmatter filing trigger has fired. Its diagnostic follow-up is now
[[iteration-274-styles-applied-unattributed-recurrence]], preserving the missing
panic/URL/document/DOM/route evidence rather than resetting the trigger. The new plan
is outside the selected execution queue; this holder remains parked.
Evidence: `.git/ralph-loop/20260919-queue/iter261/logs/live-sweep.log` and
`live-sweep-rerun.log`; exact-name counts are in `iter261-review/report.md`.

## Terminal reconciliation — 2026-09-25

Close this holder after the selected execution queue is reconciled. Record a
dated evidence cutoff and one explicit disposition for every original row and
additional frontmatter/body watch. Ticks record the original observed/forked
or deliberate code-change arm, not proof that an intermittent cause vanished.

No fourth holder is created. Conditions already assigned to iteration plans
remain with those owners. Remaining nonactionable historical watches are
retired with their evidence limits and concrete reopening triggers recorded
here. Existing per-iteration closing review handles a new occurrence; this
decision creates no timer, periodic task or extra sweep.

Reconcile conditions 6/8 with completed 262, 7 with 267, 13 with the recorded
scheduled runs and 14 with the installed ownership-first stop/wait guidance.
Use exact names and all-tier/profile summaries for 1/2/3/5; limit condition 4
claims to retained identity/listener samples. Conditions 10/11 can use recorded
scheduled conclusions, without claiming notification delivery or future health.
Preserve additional owners 274/267/268/271/275 and explicitly disposition the
unlocalized 254 dogfood 124 and 169 reload/null-status watches. No row silently
disappears; no new calendar wait, induced load or deliberate process death
is required.

## Iteration284 carry-over inputs — September 25

These records extend this holder's terminal reconciliation input, without ticking
its original criteria or scheduling extra sweeps.284closing1's direct166 case
returned the correct complete canonical document but null/no_document_request
after21125ms. Its separately admitted original direct-parity case passed both
legs with200 and all new event phases. The old failure has no attributable
event/subscription/document trace: it is distinct from the169 daemon reload
watch and is not explained by that pass. Reopen on a new unexpected null-status
occurrence with actual route, document/request identity, event progression and
fallback reason; preserve the original status assertion and deadline.

284closing1's original220 fragment case failed before action: Firefox77389
did not open port62813 within30s.281closing3 subsequently recorded four
separate old-launch timeouts:20025/64204,20047/64239,20073/64263,20075/64266.
Their retained logs lack natural-exit/cleanup distinction and failed-child
birth/profile/crash joins. The six retained281 profiles belonged to other
launches. Keep each historical cause unknown; generic load classification
is not attribution.284's controlled-child proof establishes two independent
launcher weaknesses and repairs them without extending waits. On recurrence
use the new redacted stderr-count/EOF/error and observed-child-status evidence,
then declare only the finite experiment needed for a remaining distinction.
No unchanged reproduction is scheduled by this row.

284closing1 also records timing wall1400/report579/gap821 with711.603ms before
dispatch and refresh skipped.281 owns the scoped phase diagnostics; missing
underlying RPC/scheduling attribution remains explicit. A later passing timing
case does not retrospectively prove host load or repair these intervals. Fold
this exact occurrence into the terminal disposition alongside other retained
281/283 timing records, without changing750ms/>5ms assertions.

## Terminal disposition — evidence cutoff September 25, 2026, 17:51 UTC

This is the finite reconciliation authorized by the plan adaptation in PR272
(merge `c25a80c5c0011e6b895d81137c848422e16b9512`). It supersedes the earlier
instructions to keep this holder parked indefinitely. **No fourth holder will
be created.** Retiring a watch means that this file no longer requests ongoing
monitoring; it does not establish the historical cause or close a separately
assigned implementation. An independent local review accepted this disposition with zero findings.
The supervisor reconciled the still-open selected owners before publication;
required documentation gates and exact-head CI govern its merge. The original thirteen criteria and their `[0/13]` state are
preserved verbatim rather than turning this disposition into historical proof.

The checkout baseline is merged main
`f727318d18d887a59e3db929324630fe0a66f249` (283 / PR277). It contains completed
262 / PR264 `c761c1b1f62b3cd9f6bcf56301e6528b1e70fad2`, 267's scoped blocking-mode
repair, and 284's joined daemon shutdown. Neither that ancestry nor a subsequent
passing sweep retroactively attributes an older EOF, reset, timeout, missing
selector, or navigation status. All older entries above remain historical
records, including their then-current owner/status statements.

### Evidence reused, without another sweep

Three independently required closing sweeps on their own qualified source and
binary sets passed all 349 names: 284 closing2, 281 closing4, and 283 integrated
closing2. Each used both live environment gates, reconciled all six tiers
(338 CLI, 1 frame-target, 3 registry, 3 live61u, 2 Firefox, 2 watcher-protocol),
and reported:

```text
LIVE_SWEEP_SUMMARY executed=349 skipped=0 preexisting=0 vanished=0 launch_timeout=0 timed_out=0 total=349
LIVE_SWEEP_PROFILES leaked=0 unattributed=0
```

The profile line above retains the counts; each original log also records its
own private root. All three logs explicitly report `ok` for the exact names
`live_160_envelope_honesty::live_160_ref_click_asserts_handler_effect`,
`live_104_security_pwa::live_manifest_fetch_canonical`,
`live_145_error_envelope_completeness::live_145_click_element_not_found_unchanged`,
`live_109_throttle_block::live_throttle_slow3g_slows_fetch`,
`live_137_daemon_mode_parity::live_137_consent_accept_via_daemon`, and
`live_169_nav_verb_status_parity::live_169_nav_verbs_report_status_daemon`.
These are three passing observations, not a claim of no recurrence since August
24, continuous raw-browser availability, or a new 203 sweep. Earlier failures
in those iterations remain in their original logs and are dispositioned below.

The saved GitHub query of `toolchain-watch.yml` contains five scheduled runs,
all `completed/success`, on August 24 and 31 and September 7, 14 and 21. The
latest is [run 35582616896](https://github.com/ractive/ff-rdp/actions/runs/35582616896),
created September 21 at 09:19:28 UTC against `c761c1b1`; it completed at
09:20:23 UTC. The separate August 23 manual run is excluded from scheduled
claims. These records establish cron execution and no consecutive red scheduled
pair in the observed history. They do not test September 25 main, prove
notification delivery, or guarantee the next scheduled run.

### Original thirteen criteria: disposition and reopening conditions

| Criterion / condition | Evidence and terminal disposition | Concrete reopening condition / owner |
|---|---|---|
| 1 / ref-click intermittent | Retire this general watch without claiming the old cause. Exact ref-click passed the three qualified sweeps above. Other named160 greeting-wait observations are distinct and retained below. | A new exact ref-click failure: preserve command, route/fallback, target and handler effect, then file a scoped owner before repair. |
| 2 / launch-timeout budget | Retire budget speculation; no larger wait is justified. The old220 failure and four281 startup timeouts remain individually unknown.284 repaired two independently proved launcher diagnostic weaknesses, without extending the30s bound. Three later zero-timeout sweeps do not explain them. | A new named launch timeout: retain the owned child's identity, stderr count/EOF/error and observed status from284's diagnostics; scope one discriminating experiment for what remains unknown. |
| 3 / count reconciliation | Retire the holder row; the three retained six-tier reconciliations conserve349 exact names and all count categories. This does not claim that their zero-count runs exercised vanished/launch-timeout branches. | A nonconserving total, missing/duplicate verdict, or incorrectly classified named outcome: file a sweep-accounting defect with the original log and compiled partition. No deliberately killed browser is scheduled. |
| 4 / unprovoked port6000 death | Retire the indefinite hunt without assigning cause. The old267 raw browser was absent at sweep startup, with exit cause missing; it is not proof of a death during an untouched sweep. Later owned-browser setup/teardown and boundary censuses establish only their recorded observations. | A newly observed unexpected death of the qualified raw browser: retain birth, listener and operator/signal timeline, then run the finite polling investigation from173. No continuous-availability claim is made here. |
| 5 / manifest timeout | The exact manifest test passed the three current sweeps, but this watch previously fired in258 with a greeting-wait Timeout and was recorded in267. Retire the duplicate holder; retain that historical occurrence without attributing it to267's separately demonstrated socket-mode race. | Recurrence on repaired source requires its own connection/handshake/request evidence before assigning267 or268 as cause. |
| 6 / click-not-found145 | The promotion occurrence was assigned to262;262 completed its original five criteria and three qualifying sweeps. The exact named145 control also passed all three current sweeps. Retire this duplicate watch, without equating other145 outcomes with promotion. | New named failure: distinguish target readiness, frame scan, protocol and action stage, then reopen the appropriate scoped owner. |
| 7 / throttle or launch/daemon-start failure | The broader trigger fired in named161 startup cases and was folded into267;267 repaired its attributable165 handshake occurrence. The exact throttle control passed all three current sweeps. Retire the umbrella watch; older161 and other envelopes remain unattributed. | Retain each new startup error and connection identity separately. An EOF/reset belongs to268 only by its actual stage; a displayed “after auth” Timeout alone does not prove server authentication success. |
| 8 /137 promotion | The fork arm is factually satisfied by262 and its preserved137/145 promotion evidence; this is not a tick for a mere pass.262 is merged and complete. The different ready-target/no-CMP/action outcomes remain275's responsibility. | New target1/live0 occurrence reopens promotion investigation; ready-target consent failure follows275. Neither route is closed by retiring203. |
| 10 / merge-red main survives canary | Retire the speculative holder row under the existing DEC-044 cost decision. Five scheduled successes are the bounded observation; there is no proof about main between those runs or the newest merges. | An actual merge-introduced lint failure that survives a full scheduled cycle uncaught reopens the CI-trigger decision with exact heads/run evidence. |
| 11 / failed canary unnoticed | Retire the holder row: no red pair exists among the five observed scheduled conclusions. Notification delivery has not been tested. | Two consecutive scheduled failures on the same unresolved cause require a concrete alerting/follow-up plan; no artificial red run is needed now. |
| 13 / cron never fired | The missing-cron premise is obsolete: five `schedule` successes exist. Retire this row on observed execution, not workflow YAML inspection alone. | A later Monday with no scheduled run, or GitHub reporting a disabled schedule, requires a workflow investigation. No new periodic monitor is installed. |
| 14 / raw browser left by sweep | The installed Codex iteration-close skill explicitly requires ownership-first inspection and stop/wait of the raw browser this run launched. Current closing receipts retain actual raw-child waits and independent cleanup observations. Retire the missing-instruction premise. | A stale raw listener, missing actual wait, or ownership-ambiguous cleanup requires a scoped runner correction; never stop a pre-existing browser merely by its port/name. |
| Final / fourth holder decision | Explicit decision: no fourth holder, no calendar wait and no extra sweep. Historical evidence is retained here; implementation responsibilities stay with their named plans. | A concrete recurrence is handled during its required closing review, with one scoped owner and finite evidence schedule. Do not reopen an indefinite holder. |

### Additional frontmatter/body watches and active owners

| Retained observation | Disposition, limits and reopening evidence |
|---|---|
| 252/253/254/242 named160/161/164/219/240 greeting-wait Timeout envelopes |267 retains the individual observations and its demonstrated165 repair. Retire the duplicated203 watch. Do not generalize that proof to every older occurrence; on recurrence preserve pre-auth connection identity, auth read/write, greeting and request/dispatcher sequence. |
| Historical240 hop27 EOF;224 hop7 reset54; later265 EOF |268 remains open with original0/4 and distinct occurrence records.284's actual joined-shutdown implementation supplies a prerequisite, not missing failed-occurrence attribution. External death is not a worker return. No consumed268 capture is replayed by this disposition. |
| 253 applied-styles missing-p;261 styles failure with missing panic |274 completed its explicitly bounded unresolved investigation and merged. The single passing instrumented control did not diagnose either failure. Retire the duplicated watch; a new attributable recurrence reopens274's exact stage/document/DOM/route/rules evidence requirement. |
| 254 dogfood exit124 after lifecycle checks, before sentinel |Retire the nonactionable watch with cause unresolved: the original suppressed/removed output cannot localize the failed command. The later output-retaining script and passing control are not a product repair. On recurrence retain the exact command/envelope, port/PID and route; route to267 only if the evidence supports that stage. |
| 242 daemon169 reload: complete, null/not_observed after21028ms |Retire this historical watch without a cause claim. Exact169 passed the three current sweeps; those passes cannot recover its missing event/document trace. A new null-status occurrence requires route, actual document/request identity, network status, subscription/event progression and fallback reason before repair. |
| 284 direct166: complete canonical document, null/no_document_request after21125ms |Keep separate from daemon169. The admitted original direct-parity passing control observed both200 legs but did not explain the old result. Retire the duplicate watch with the same evidence requirements scoped to its actual direct route; preserve original assertion/deadline. |
| 284220 initial launch timeout and four281 old-launch timeouts |Retain every PID/port and missing-evidence limitation in the284 input section above. Retire203's duplicate tracking after284's diagnostic repair, not as historical causal closure. The original30s budget remains; new recurrence uses the new failed-child evidence. |
| 281/283/284 elapsed-time gaps |281's merged phase diagnostics and279's region contract identify measured versus excluded work; they do not identify the underlying scheduling/RPC cause of each old gap. In particular284's wall1400/report579/gap821 and711.603ms pre-dispatch remain preserved, alongside283's gap1048 and281's earlier failures. Retire the duplicate watch; new failure requires actual command phases under unchanged750ms/>5ms assertions. |
| 283 historical daemon/direct frame-count mismatch and later159 initial network timeout |283 completed the same-observation-cut contract and controls; sequential observations are not an atomic identity oracle. The old causes remain unknown. Retire any duplicate umbrella inference; a new failure needs target identities and observation windows, or the actual failed navigation stage, respectively. |
| Guardian ready-target detection/action failures; BBC no-CMP |275 remains open at this cutoff;271 remains dependent on its consent contract. Neither262 promotion repair nor203 retirement closes either site-specific occurrence. Preserve the distinct page/banner/action/effect evidence before claiming a shared cause. |
|147 language diagnostics,259 outstanding-reply ownership,266 resource grips |These are independent open implementation owners, not watches to retire here.147's English-pack named proof still failed;259 retains its action-specific execution-filter boundary;266 remains dependent on259. No source work or acceptance evidence is supplied by this documentation audit. |

There is no all-open-complete claim:275,271,147,259,266 and268 remain under the
supervisor's selected queue. At final reconciliation,275 had passed its two separately admitted five-case
native matrices and its own closing sweep was running, so it remained open.
268 had repaired two attribution-check findings pending fresh review;147
had a reviewed private diagnostic pending qualification.259/266 and271 remained
pending. Completion here applies only to the terminal watch disposition, not
to those owners or the original historical criteria. Publication does not
authorize a new capture, replace a criterion, or erase failed results.

Evidence for this audit is private under
`.git/ralph-loop/20260924-all-open/iter203/disposition1/`: the original plan,
exact-name/six-tier extracts and source log hashes, saved scheduled-run records,
final diff and original-criteria conservation receipt. The reused source logs
are `iter284/closing2`, `iter281/closing4`, and
`iter283/integrated-gates1/closing2` under the same run root. This documentation
audit launched no additional Cargo tests, Firefox, sweep or investigation;
normal publication CI remains required.
