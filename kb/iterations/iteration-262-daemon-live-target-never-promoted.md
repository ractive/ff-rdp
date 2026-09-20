---
title: "Iteration 262: the daemon counts a frame target and never promotes it to live"
type: iteration
date: 2026-09-07
status: in-progress
branch: iter-262/daemon-live-target-never-promoted
depends_on:
  - 246
first_call_sites:
  - primitive: (to be decided — a repair path on the frame-target subscription)
    site: crates/ff-rdp-cli/src/daemon/server.rs
dogfood_path: |-
  # Reproduce under sweep contention; isolation also failed in the recorded255 repair.
  firefox -no-remote --start-debugger-server 6000 --headless   # raw browser, NOT ff-rdp launch
  FF_RDP_LIVE_TESTS=1 FF_RDP_LIVE_NETWORK_TESTS=1 cargo run -p xtask -- live-sweep 2>&1 | tee /tmp/sweep.log
  grep "LIVE_TARGET_WAIT" /tmp/sweep.log
  # expected TODAY: some runs show `reached=false elapsed_ms≈15000` with the daemon
  # reporting target_count=1, live_target_count=0 and a healthy dispatcher.
  # expected AFTER: every LIVE_TARGET_WAIT line reports reached=true, or a
  # reached=false line is accompanied by live_target_count>0 having been observed
  # and lost — i.e. the bookkeeping is repaired rather than latched.
  #
  # Serial control: retain either result; isolation reproduced promotion in255:
  FF_RDP_LIVE_TESTS=1 FF_RDP_LIVE_NETWORK_TESTS=1 cargo test -p ff-rdp-cli --test live \
    live_137_consent_accept_via_daemon -- --include-ignored --test-threads=1
tags: [iteration, daemon, frame-targets, race, live-tests, carry-over]
iteration_252_followup_observations: "2026-09-09 final dual-gate sweep: live_137_consent_accept_via_daemon failed AFTER LIVE_TARGET_WAIT reached=true in 116 ms, with Sourcepoint detected_not_actioned/consent_not_actioned. The exact isolated test failed again after readiness in 113 ms. An untouched origin/main checkout at 05634cbd42f02995f50e06c35d68b868cd39d0ba independently reproduced the same consent error, establishing this predates iteration 252. This is DISTINCT from the original target_count>0/live_target_count=0 failure; do not attribute it to promotion without evidence. Folded here because this plan already requires three full sweeps with this named consent test green: retain both failure shapes and resolve or file the separate consent-action failure when addressing that original AC. The earlier diagnostic sweep also saw live_140_frame_filter_count_accurate report zero frames; it passed the final sweep and isolated rerun, and no target-count instrumentation was captured for that row. Evidence: iteration252 PR closure report and .git/ralph-loop/20260909-takeover/iter252/resume-1 logs. Original scope and acceptance criteria remain unchanged."
iteration_252_review_repair: "The final repair sweep again failed live_137_consent_accept_via_daemon before consent: LIVE_TARGET_WAIT reached=false after15252ms and49 polls; target_count1/live_target_count0; dispatcher alive with88 started/finished frames and no RPC owner. Exact isolated rerun promoted in67ms then separately failed Sourcepoint consent_not_actioned as untouched main already did in resume-1/baseline-consent.log. Keep both signatures distinct; original three-green-sweeps AC remains unmet. Frame-filter140 again reported0 available frames in this sweep and passed exact isolation; target counts were not captured for that failure so shared cause remains unproved. Evidence: .git/ralph-loop/20260909-takeover/iter252/review-repair-1."
iteration_252_windows_sweep: "2026-09-12 final Windows compatibility correction sweep: live_137_daemon_mode_parity::live_137_consent_accept_via_daemon recurred with the original target-promotion signature: reached=false after15094ms/47polls, target_count1/live_target_count0, dispatcher healthy with85started/85finished and no inflight work. Exact isolated rerun reached a live target in13ms/1poll and accepted Sourcepoint consent. This sweep did not reproduce the separate ready-target consent-action failure; that historical observation and all original ACs remain open. Do not count the isolation pass as a green sweep or claim the required consecutive sweeps. Full current sweep330=321pass+9fail, zero skips/reclassifications/profile leaks. Evidence: .git/ralph-loop/20260912-validation-efficiency/pr247-windows-abort/sweep.log and isolated-live_137_daemon_mode_parity.log."
iteration_252_windows_receive_sweep: "2026-09-12: preserve three distinct observations from final Windows-receive correction validation. (1) live_137_consent_accept_via_daemon sweep failed promotion: reached=false15134ms/47polls, target_count1/live_target_count0, healthy dispatcher98started/98finished, no inflight work or RPC owner. (2) Exact isolation reached readiness49ms/1poll, then failed consent_not_actioned/detected_not_actioned for Sourcepoint; this is the separately known ready-target action failure, previously reproduced on untouched main, not a passed rerun or a reproduced promotion failure. (3) live_129_sourcepoint_consent direct navigation detected Sourcepoint but action=null/detected_not_actioned; its exact isolation passed accepted in4.72s. No direct-route readiness instrumentation or common cause is established. Original three-green-sweep and all other ACs remain unmet. Evidence: .git/ralph-loop/20260912-validation-efficiency/pr247-windows-recv/sweep.log and the two named isolated consent logs."
iteration_253_observation_2026_09_12: "Closing dual-gate sweep: live_137_daemon_mode_parity::live_137_consent_accept_via_daemon failed after 15038 ms and 47 polls. Last status: PID 80419, uptime 16 s, target_count 1, live_target_count 0, dispatcher alive, frames_started=frames_finished=90, in_flight=0, network-event buffer 464. Exact isolated rerun passed in 5.60 s, target readiness 29 ms / one poll. This is another measured target-promotion recurrence, not a fixed defect; the three consecutive green-sweep AC remains unticked. The two live_145 target tests and live_network_watcher_source_after_navigate_with_network passed this sweep. Evidence: iter253/sweep.log and isolated-live_137_daemon_mode_parity::live_137_consent_accept_via_daemon.log."
iteration_253_precheckpoint_2026_09_12: "2026-09-12 final253 pre-checkpoint sweep: live_137_daemon_mode_parity::live_137_consent_accept_via_daemon failed target promotion15108ms/47polls. DaemonPID60752, proxy54734, uptime16s, target_count1/live_target_count0, network buffer438, dispatcher alive99started/99finished, in_flight0 and no RPCowner. Exact isolation reached live targets37ms/1poll, then separately failed Sourcepoint consent_not_actioned/detected_not_actioned in5.81s. Preserve both signatures; isolation is neither a pass nor reproduction of promotion failure, and no common cause is proved. The two145 promotion tests and watcher-source test passed in this sweep, but original consecutive-green-sweep AC remains unmet. Full current sweep334=322passed+12failed, zero skips/reclassifications/leaks, all names reconciled; earlier253 sweep332 remains historical. Evidence: .git/ralph-loop/20260912-validation-efficiency/iter253/precheckpoint-edge-verification/sweep.log and isolated-live_137_daemon_mode_parity::live_137_consent_accept_via_daemon.log."
iteration_253_review_repair_1_2026_09_12: "Current final repair sweep336=328passed+8failed, all names/five tiers reconciled, no skips/reclassifications/profile leaks. Consent promotion recurred: live_137_daemon_mode_parity::live_137_consent_accept_via_daemon reached=false15005ms/47polls, Firefoxport53180, daemonPID43194/proxy53309/uptime16s, target_count1/live_target_count0, network-event buffer451, dispatcher alive78started/78finished, in_flight0, no RPCowner. Exact isolation reached live targets113ms/1poll and passed5.61s. This does not fix promotion or supply a green sweep. Historical ready-target Sourcepoint consent_not_actioned remains a distinct unmet observation; no common cause asserted. Original tasks and ACs unchanged. Evidence: .git/ralph-loop/20260912-validation-efficiency/iter253/review-repair-1/sweep.log, sweep-failures.txt and isolated-live_137_daemon_mode_parity::live_137_consent_accept_via_daemon.log."
iteration_253_review_repair_2_2026_09_12: "Final253 repair2 sweep338=330passed+8failed recurs in live_137_consent_accept_via_daemon: live-target promotion failed15095ms/47polls, Firefoxport52945, daemonPID40471/proxy53055, uptime16s, target1/live0,451networkevents, healthy103started/103finished dispatcher,inflight0,noRPCowner. Exact isolation separately reached targets78ms/1poll on Firefoxport62938/proxy62966, then failed5.50s with Sourcepoint consent_not_actioned/detected_not_actioned. These are DISTINCT promotion and ready-target consent-action observations, not a demonstrated common cause. Both remain assigned262 with all original tasks/ACs untouched and unmet. All five tiers/exact338names reconcile, zero skips/reclassifications/profile leaks. Evidence: .git/ralph-loop/20260912-validation-efficiency/iter253/review-repair-2/sweep.log, isolated-live_137_daemon_mode_parity-live_137_consent_accept_via_daemon.log, observed-results.tsv and profile-accounting.txt. Earlier336/334sweeps remain historical."
iteration_254_review_repair_2_2026_09_13: "2026-09-13 iteration 254 repair 2 final sweep: 338 = 329 passed + 9 failed; all five tiers and exact names reconciled, with zero profile leaks. Consent promotion recurred: live_137_daemon_mode_parity::live_137_consent_accept_via_daemon reached=false after 15042 ms / 47 polls on Firefox port 61142; daemon PID 70011 / proxy 61216 / uptime 17 s, target_count=1 / live_target_count=0, network buffer 441, dispatcher alive, 83 started / 83 finished / in_flight=0, no RPC owner. Exact isolation reached=true after 35 ms / 1 poll on Firefox port 55464 / proxy 55486 and accepted Sourcepoint (libtest 6.00 s, command wall time 6.46 s). This does not supply a green sweep or fix promotion; historical ready-target Sourcepoint consent_not_actioned remains distinct. Separately live_140_element_targeting::live_140_frame_error_bounded failed its more-frames assertion: click missing selector reported 0 of 0 frames tried / of 0 total on daemon proxy 62089. Exact isolation passed 4.43 s (command wall time 4.73 s); no failing-occurrence target counters or common cause were captured. Fold this new named zero-frame observation with the existing unattributed 140 frame-filter family here; preserve it separately from measured promotion and split into its own plan if diagnostics establish a different unresolved cause. Original tasks and all ACs unchanged/unmet. Evidence .git/ralph-loop/20260912-validation-efficiency/iter254/review-repair-2/sweep.log and both exact named isolated logs."
iteration_255_observation_2026_09_13: "Closing dual-gate sweep341=331passed+10failed, all five tiers/exact names reconciled and all profile scans clean. Consent promotion recurred: live_137_consent_accept_via_daemon reached=false after15125ms/48polls on Firefoxport52303, daemonPID86661/proxy52444/uptime18s, target1/live0, network buffer431, healthy84started/84finished dispatcher,inflight0,noRPCowner. Exact isolation reached=true50ms/1poll and accepted Sourcepoint in5.75s; this is not a green sweep or a fix. Separately live_140_frame_error_bounded again reported0of0frames tried/of0total; exact isolation passed. No failed-occurrence target counters for140 or common cause captured; keep separate from measured promotion. Historical ready-target Sourcepoint consent-action failure and original three-green-sweep AC remain unmet. Evidence: .git/ralph-loop/20260912-validation-efficiency/iter255-implementation/sweep.log, sweep-reconciliation.json, isolation-results.tsv and named logs."
---

# Iteration 262: the daemon counts a frame target and never promotes it to live

> Filed by [[iteration-246-sweep-load-misclassification]] Part D, which instrumented the wait,
> got one honest failure, and reached verdict (1) — a product race — on the evidence below.
> Iteration 246 deliberately did not attempt the fix: it is daemon bookkeeping, it needs its own
> live test that fails before and passes after, and 246 already carried four merged plans.

## The evidence

From iteration 246's closing sweep (`FF_RDP_LIVE_TESTS=1 FF_RDP_LIVE_NETWORK_TESTS=1`,
default `--jobs`, macOS, 2026-09-07, `executed=328 … total=328`, 308 passed / 11 failed):

```
---- live_137_daemon_mode_parity::live_137_consent_accept_via_daemon stdout ----
LIVE_TARGET_WAIT port=55418 reached=false elapsed_ms=15301 polls=49 bound_ms=15000
… last daemon status: status=Some(0) stdout={
  "running": true, "pid": 57215, "port": 55556, "uptime_seconds": 20,
  "connections": 0, "buffer_sizes": { "network-event": 446 },
  "target_count": 1, "live_target_count": 0,
  "dispatcher": { "alive": true, "frames_started": 130, "frames_finished": 130,
                  "in_flight": 0, "last_frame_kind": "resources-updated-array" }, … }

---- live_145_…::live_145_click_frame_scan_js_exception_envelope stdout ----
LIVE_TARGET_WAIT port=57307 reached=false elapsed_ms=15163 polls=48 bound_ms=15000

---- live_145_…::live_145_click_element_not_found_unchanged stdout ----
LIVE_TARGET_WAIT port=57200 reached=false elapsed_ms=15048 polls=47 bound_ms=15000
```

Three things in that status rule out the two non-product explanations:

1. **`target_count: 1, live_target_count: 0` after 20 s of uptime.** The daemon saw a frame
   target and never promoted it. A machine too slow to answer within 15 s would show the count
   arriving late, not a target counted and permanently not-live.
2. **`dispatcher.alive: true`, `frames_started: 130 == frames_finished: 130`, `in_flight: 0`.**
   The daemon is neither wedged nor behind; it answered 47–49 `daemon status` polls inside the
   window, each well under the loop's 300 ms spacing.
3. **All three failures exhaust the bound within 300 ms of each other** (15048 / 15163 /
   15301 ms). A bound that is merely too tight produces a distribution straddling it. This
   produces a cliff, which is what a latched state looks like.

Same shape as [[iteration-179-live-62-runner-sees-no-network-events]]: a subscription armed after
the event it needs, so the arming is missed and never repaired.

## A second, deliberately-unattributed observation from the same sweep

```
live_network_default_watcher::live_network_watcher_source_after_navigate_with_network
  at least one network entry should have source='watcher' after navigate --with-network.
  Got 0 entries, all with sources: []
```

Another daemon-side subscription reporting **nothing** in the same run — and the daemon status
quoted above shows `buffer_sizes: { "network-event": 446 }`, i.e. the network buffer was far from
empty on a *different* daemon in the same sweep. It is folded in here rather than given its own
plan because it is the same *family* (a subscription that produced no observations under sweep
load) and it has never been filed anywhere.

**Do not assume it shares a cause with the frame-target rows.** It is a different resource type
and a different code path. If the frame-target fix lands and this still fails, it earns its own
plan at that point, with whatever the fix's instrumentation says about it.

## Themes

Iteration 255 repair sweep (2026-09-13) reproduced the promotion failure in
`live_137_consent_accept_via_daemon`: 15,210 ms / 49 polls; Firefox PID 57127,
debug port 52758; daemon PID 57257, proxy 52906; target 1/live 0; healthy
dispatcher 102 started/102 finished, no in-flight frame or RPC owner, 439 network
events. Evidence: `.git/ralph-loop/20260912-validation-efficiency/iter255-repair1/sweep.log`.
The separate `live_140_frame_error_bounded` zero-frame case passed in this sweep;
its previous failure and unproved relationship to promotion remain open. This
new observation does not supply the three-green-sweeps acceptance requirement.
Exact isolated rerun also failed (19.92 s): 15,252 ms / 49 polls, debug port
63649, daemon PID 77820 / proxy 63669, target 1/live 0, healthy dispatcher
79/79 with no in-flight work or RPC owner, 508 network events. Its full status
is in `iter255-repair1/isolated-live_137_daemon_mode_parity-live_137_consent_accept_via_daemon.log`
under the same run directory. The failure is therefore not confined to this
sweep's parallel execution; no new cause or timeout change is claimed.

- **A — Find where `live_target_count` is set and why it can stay 0 with `target_count` at 1.**
  The two counters disagree, so the promotion step is the suspect, not the enumeration.
- **B — Decide whether the fix is ordering (arm before the event) or repair (re-derive liveness
  when a target is observed).** 179/181's precedent is ordering; a latched counter may need both.
- **C — A live test that fails before the fix and passes after.** Iteration 246 could not write
  one because the trigger is contention; find a deterministic trigger, or drive the daemon's
  subscription directly.

## Tasks

### A. Locate [2/2]
- [x] Identify every write to the live-target bookkeeping in `daemon/server.rs`
- [x] Explain, in writing, how `target_count: 1` and `live_target_count: 0` coexist

### B. Fix [1/2]
- [ ] Land the fix the explanation points at
- [x] State whether it is ordering, repair, or both

### C. Test [1/1]
- [x] A live Firefox test that fails before the fix and passes after — not only a unit test

## Acceptance Criteria [2/5]

- [x] The coexistence of `target_count > 0` and `live_target_count == 0` is explained in writing
- [x] A live test fails on the pre-fix build and passes on the post-fix one
- [ ] Three consecutive full live sweeps with `live_137_consent_accept_via_daemon`,
      `live_145_click_frame_scan_js_exception_envelope` and
      `live_145_click_element_not_found_unchanged` green
- [ ] `live_network_watcher_source_after_navigate_with_network` is either green in those same
      three sweeps or filed as its own plan with the fix's evidence
- [ ] `cargo fmt && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace -q` clean

## Out of scope

- Raising the 15 s bound in `common::wait_for_live_targets`. Iteration 246 established that the
  bound is not the defect and that raising it would have hidden this; `FF_RDP_TEST_LIVE_TARGET_WAIT_S`
  exists for a deliberate one-run measurement, not for a new default.
- The seven `--full-page` screenshot failures in the same sweep — those are
  [[iteration-257-firefox-155-drawsnapshot-dictionary-arg]].

## References

- [[iteration-246-sweep-load-misclassification]] — the instrumentation and the verdict
- [[iteration-179-live-62-runner-sees-no-network-events]] — the precedent for this shape
- [[iteration-181-playbook-scoped-network-subscription]] — its fix, on the daemon/direct split
- `crates/ff-rdp-cli/tests/common/mod.rs` — `wait_for_live_targets`, `LIVE_TARGET_WAIT`


## Owed242 observations, 2026-09-14

Two distinct cases recurred at `19f4e236a70399c2984e6d46eb15bdac37a55181`:

- `live_145_error_envelope_completeness::live_145_click_element_not_found_unchanged`
  failed promotion after15008ms/48polls. Firefox58291/debug55380;
  daemon58392/proxy55550, uptime17s; target1/live0, network buffer12;
  dispatcher alive56started/56finished/in_flight0, noRPCowner;
  clients_dropped_on_write1. The write-drop counter is evidence, not a proved cause.
  Exact isolation passed5.46s (command wall5.64s).
- `live_140_element_targeting::live_140_frame_filter_count_accurate` reported
  zero frames for the leaf1 filter, where five candidates were required.
  Firefox56876/debug54498, proxy54633. Exact isolation passed5.57s
  (command wall6.01s). No failed-occurrence target counters establish a shared cause.

The137 consent, other145 and watcher-source tests passed this sweep, which does
not satisfy the three-consecutive-green-sweeps AC. Historical ready-target
Sourcepoint failures remain distinct and unresolved. The metadata dogfood claim
that promotion does not occur in isolation is superseded by the recorded255
repair isolation failure; treat isolation as a measured control, not expected
success. This run made no promotion/consent fix and changed no deadline.
Evidence: `.git/ralph-loop/20260912-validation-efficiency/iter242-owed-sweep/sweep-failures.txt`, exact isolated logs and
`sweep-reconciliation.json` (343=331pass+12fail, no missing names/reclassifications/leaks).

## Iteration257 reconciliation, 2026-09-14

The screenshot fix's closing sweep344=342pass+2fail produced a **new distinct
Guardian detection outcome** in `live_137_consent_accept_via_daemon`:
Firefox51777/debug54644, proxy54746; live targets were ready in106ms/1poll,
then consent returned `consent_no_cmp`, `cmp:null`, `action:null`,
`status:no_cmp_detected`. This is neither the original promotion failure nor
the previous detected-but-not-actioned Sourcepoint failure. Failed-occurrence
page/banner DOM and document identity were not retained; no cause is claimed,
and the identical error type on BBC's separate direct-route test does not prove
a common mechanism. Fold this named137 outcome here because the original
three-green-sweeps requirement already owns that scenario. Preserve all three
shapes; resolve or separately file the detection/action outcomes when pursuing
that requirement, without weakening acceptance or changing waits.

Exact serial dual-gate isolation passed5.09s: Firefox74320/debug65519,
proxy49157, readiness18ms/1poll, Sourcepoint accepted through the daemon.
This does not diagnose or close the sweep failure. Both145 tests, both140
frame controls and watcher-source passed; no promotion fix or three-green-sweep
claim is made. All original tasks and ACs remain unchanged and unmet.

The metadata dogfood comments now agree with the already-recorded255 repair
isolation failure: isolation is a measured control, not guaranteed green.
This corrects validation guidance without executing this plan. The seven
full-page screenshot regressions owned by257 passed together; their prior
negative history remains intact. Evidence:
`.git/ralph-loop/20260912-validation-efficiency/iter257-implementation/`,
`sweep.log`, the exact isolated137 log and both launch logs.

## Implementation preflight — 2026-09-19

Correct the historical interpretation before selecting a fix: daemon target_count is cumulative available events, while live_target_count counts currently retained forms. An available event immediately records its form; destruction removes it. Therefore 1/0 can mean arrival followed by destruction, not a failed separate promotion step. Existing initialization code documents a placeholder about:blank available/destroyed/no-replacement sequence and a 350ms settling mitigation. This is an investigation lead, not proof of the recorded failures' cause; repeated expiration of a bound alone does not prove a permanently latched state. Preserve distinct target-lifecycle, ready-target Sourcepoint action, Guardian detection, zero-frame and network-source observations unless evidence connects them. Original repeated-sweep acceptance requirements remain unchanged.

This source/evidence audit adds implementation guidance, not a new execution result.
Original task and acceptance-criterion wording and checkbox states remain unchanged.

## Iteration258 sweep observation — 2026-09-19

`live_145_error_envelope_completeness::live_145_click_element_not_found_unchanged`
failed its readiness precondition before invoking click: debug64018/proxy64126,
daemon70185, reached=false after15.213s/47polls, target_count1/live_target_count0,
dispatcher alive with52started/52finished, no in-flight frame or RPC owner.
This repeats the original target-promotion signature; no common cause with
consent-action failures is inferred. No isolation rerun was used to replace it.
The344-name sweep had342pass/2fail and zero skips, reclassifications or leaks;
its other failure was a greeting timeout owned by267. Original ACs remain unmet.
Evidence: primary checkout `.git/ralph-loop/20260919-queue/iter258/sweep.log`.
## Iteration263 reconciliation — 2026-09-19

Firefox156.0 final263 repair sweep executed344=342passed+2failed with exact
all-tier names and zero profile leaks. Named137 failed target readiness at
15,257ms/47polls, debug51281/proxy51370/daemon17257, uptime16s, target_count1 /
live_target_count0, healthy dispatcher90started/90finished/in_flight0, no RPC
owner and no dropped client writes. This does not identify a lifecycle cause.
Earlier263 sweeps separately failed ready-target Sourcepoint action; the first
reached targets in21ms. Preserve these distinct shapes. An intermediate344/344
pass fixed neither and does not supply this plan's three consecutive sweeps.
Original tasks/ACs remain unchanged. Evidence:
`.git/ralph-loop/20260919-queue/iter263-repair2/live-sweep.log`,
`reconciliation.json`, and the prior `iter263/logs/` and `iter263-repair1/` logs.

The final263 security-dependency sweep separately repeated137's target-readiness
failure:15,194ms/47polls, debug64969/proxy65086/daemon42087. It is not a new
Sourcepoint-action observation. Full344=341pass/3fail, no missing names or profile
leaks; original cause and three-sweep requirements remain unmet. Evidence:
`.git/ralph-loop/20260919-queue/iter263-security/sweep.log`.

## Iteration261 recurrence — 2026-09-19

The corrected 261 dual-gate sweep failed
`live_137_daemon_mode_parity::live_137_consent_accept_via_daemon` at target readiness:
15,077ms / 47 polls, debug port55205, proxy55274, daemon8566. The last status had
uptime17s, target_count1/live_target_count0, network-event buffer455, dispatcher
100 frames started/finished, no in-flight frame and no RPC owner. This is another
failed occurrence to explain, not proof of a promotion phase or a particular cause.
343 other tests passed, all344 names accounted for, zero profile leaks.
Evidence: `.git/ralph-loop/20260919-queue/iter261/logs/live-sweep-rerun.log`.

## Bounded implementation investigation — 2026-09-19

Source `38add406f711434f520fa06494b2e02ab3f5e0b2`, Firefox156. No product
repair was selected. Twelve named137 probes (six serial, then six concurrent)
produced two passes, three target-readiness failures, six ready-target
Sourcepoint action failures, and one ready-target Guardian `consent_no_cmp`.
The concurrent probes used separate owned homes/browsers, not a full sweep.
Probes2–12 added failure-only page diagnostics without changing assertions;
that temporary test edit was saved as `diagnostic-only.patch` and restored.

The three readiness failures are probes8/11/12, debug52138/52139/52141,
15206/15211/15241ms, all47polls. Their daemon wire traces capture the missing
lifecycle directly: one available `about:blank` target, destruction during
Guardian navigation, and no replacement available event before cleanup.
In probe8, watcher `server1.conn5.watcher3` received `watchTargets` at
11:40:45.571107Z; available target
`server1.conn5.watcher2.process4//windowGlobalTarget2`, innerWindowId8589934593,
arrived at11:40:45.594172Z. Destruction arrived at11:40:47.635639Z.
The client's `unwatchTargets` at11:40:48.240798Z was received by the proxy but
**not sent to Firefox**. The watcher remained daemon-owned; its initial
`getWatcher` explicitly enabled server target switching.

At failure, `getTarget` still returned the loaded Guardian `/europe` document,
innerWindowId17179869185, browsingContextID11, via a legacy child target on
the same connection. The daemon-routed DOM snapshot reported `complete`.
Thus `target_count=1/live_target_count=0` is concretely one availability plus
destruction, not a failed promotion phase. The last wait status had453 network
events,95 dispatcher frames started/finished, no in-flight work/RPC owner and
no dropped client writes. Later diagnostic requests increased the frame count;
those later counts must not replace the failure-time status.

All writes are in `daemon/server.rs`: availability increments `target_count`
and `record_frame_target` inserts/replaces the form; a top-level switch clears
old forms before insertion; `forget_frame_target` removes a destroyed actor.
Initialization starts both counters/storage empty. Status snapshots the retained
forms. The counter coexistence is explained, but **why Firefox delivers no
replacement on this watcher is not established**. This sequence is later than
startup: it does not prove the existing350ms placeholder-settling explanation.
Local Firefox watcher code was inspected at0088392ab4ccab730743ed188ddec62d04e578b7;
that checkout's provenance is separate from the installed Firefox156 binary.

The bounded investigation stops before speculative retry/re-subscription changes.
Resume with the retained failing wire sequence and Firefox process/window-global
watcher instrumentation sufficient to identify why replacement notification is
missing, then demonstrate a specific live before/after repair. All original
tasks/AC checkboxes remain unchanged for supervisor review. No closing sweep,
three-consecutive-sweep claim, repair gate pass or implementation-complete claim
is made. The original15s bound is unchanged.

Ready-target Sourcepoint action and Guardian-no-CMP outcomes are now filed in
[[iteration-275-guardian-consent-ready-target-failures]], with failure-time top
DOM, iframe attributes, document identity and route. CMP-frame DOM remains
missing; filing does not satisfy this plan's named137 consecutive-sweep AC.
Zero-frame140 and watcher-network observations were not exercised or resolved;
they remain separately held here, with their historical evidence intact.

Raw evidence and recovery files:
`.git/ralph-loop/20260919-queue/iter262/` (`probe-N.log`, `probe-N.meta`,
`home-N/.ff-rdp/daemon.log`, `derived/`, `diagnostic-only.patch`, implementation
report and phase result). All probe sessions finished and their owned browsers
were cleaned up; desktop Firefox PID1112 was preserved and port6000 stayed free.
Three failed-readiness probes left dead-owner profile directories in their
private roots after process exit. Scoped `profiles prune --all` subsequently
removed all three (`owner_liveness=dead`, `failed={}`); cleanup logs preserve
this anomaly without attributing it to the target lifecycle defect. The
supervisor must reconcile it with the selected cleanup work; it is not a
zero-profile-leak claim or a full-sweep profile summary.

## Installed-revision controlled comparison — 2026-09-19

Attempt2 preserved attempt1, fetched narrow official Mozilla source files at
the installed binary's `a80bd15ddee3b4bf3679aeba340e9d2db933c467` revision
(Firefox156.0, BuildID20260909172920), and rebuilt restored test inputs first.
Fetched files, source URLs/commands, hashes and comparison logs are retained in
`.git/ralph-loop/20260919-queue/iter262/attempt2/`.

The `watcher3`/`watcher2` names are **not a mismatched subscription**:
`WatcherActor` separately allocates `watcherConnectionPrefix` with `allocID`;
the managed actor receives its own actor ID. Parent registry session data uses
the former as `connectionPrefix`; content registry adds `.process<childID>/`
for target routing. These are two namespaces belonging to the same watcher.

One controlled comparison used two arms, each six concurrent owned browsers,
with no consent execution. Every browser had its normal daemon watcher plus
a direct RDP watcher on the same tab, both requesting server target switching
and the same three resource types. Navigation still ran through the daemon.

| Arm | Observation |
|---|---|
| Idle direct watcher, probes1–6 | All six direct connections retained3 targets; five daemon connections timed out at15s with no targets, one reached readiness. |
| Direct watcher also issues legacy `getTarget` every250ms, probes7–12 | Direct retained0 targets in probes9/10/11,3 in7/8,2 in12. Daemon failed readiness in7/8/9/10 and succeeded in11/12. All navigations succeeded. |

Direct probe10 received one watcher `about:blank` availability, a legacy
child-target destruction and the watcher-target destruction, then no replacement
over the daemon's full15s wait. Therefore daemon bookkeeping is **not required**
to reproduce target loss when legacy and watcher actors share an RDP connection.
Probe11 ended when daemon readiness succeeded, so its zero direct-target count
is a shorter observation, not a15s persistence claim. Probe8 had one recorded
`getTarget` `tabDestroyed` error; its later3-target result does not erase that.
These diagnostic tests print outcomes and do not assert product success; their
libtest `ok` verdicts must not be counted as passed acceptance tests.

Installed-source evidence gives a specific candidate mechanism:
`window-global.sys.mjs` `findTargetActor` first searches watcher-owned actors,
then queries `TargetActorRegistry` using the same RDP connection prefix and
browser context. `onWindowGlobalCreated` skips creation when that match is an
existing non-JSWindowActor legacy target. Daemon traces show `getTarget` creating
legacy `child*` actors around navigation, including an interim `about:blank`
document in the destination process. The direct legacy arm changes that
connection's outcomes without involving daemon bookkeeping.

This still does **not identify the executed server branch** in a failed
occurrence. The exact diagnostic dependency is to observe, for the replacement
innerWindowId and watcherActorID, (1) the content-process watcher data and
`findTargetActor` result/`createdFromJsWindowActor` at `onWindowGlobalCreated`,
then (2) whether `onNewTargetActor` sends `targetAvailable`, and (3) whether
`DevToolsProcessParent.#onTargetAvailable` accepts it. The installed parent
source adds WindowGlobal/process-identity checks absent from the older local
checkout; an exception there remains a distinct unmeasured alternative.
Use these checkpoints to distinguish legacy suppression, missing watcher
registration and parent delivery rejection before choosing a repair.

A naive cached `getTarget` substitution is not yet a safe selected repair:
it must preserve tab identity, wait/cancellation/reply ownership while the old
target is destroyed, and existing navigation refresh behavior. This bounded
pass makes no architecture change, retry/reset or deadline change. Product
source and original assertions are restored; a final `cargo test -p ff-rdp-cli
--test live --no-run` succeeded at11:58:08Z, so no diagnostic test binary is left
as the build corresponding to the restored source. Before/after and all original
three-sweep obligations remain unmet; plan275 was not executed.

Cleanup attribution is now established from the actual paths: readiness panic
occurs before137's explicit `stop_daemon`, so only `LiveFirefox::Drop` runs;
that guard calls `kill_pid_and_wait` and intentionally does not remove profiles.
The explicit daemon-stop path in `daemon/client.rs` additionally attempts
managed-profile cleanup. Attempt1's three dead-owner remnants therefore match
the documented panic/orphan-prune behavior, not evidence of a false daemon-stop
removal or a new260/261 defect. Attempt2 explicitly stopped each browser;
the final census found no owner-marker remnants and only preserved desktop
Firefox PID1112. No full-sweep profile summary is claimed.

## Supervisor checkpoint — 2026-09-19

Independent review returned zero findings on the bounded diagnostic record.
The two Locate tasks and counter-explanation AC are fulfilled by the retained
source/trace evidence. All repair, before/after, consecutive-sweep and remaining
acceptance requirements stay unticked. Status is in-progress; no product repair
or completed iteration is claimed. New275 is an unselected diagnostic follow-up.

Before checkpoint the branch fast-forwarded to verified260 merge
`010059c632da4ce1344b0516a05a7c911b4cfe15` (documentation only). Source/test/dependency
inputs remain identical to verified261 source; its ordered fmt/clippy/workspace-test
passes and unaffected static checks are reused for this documentation-only recovery
commit. Both262/275 plan checks, HYALO005 and diff check apply to the final docs.
## Iteration265 sweep recurrence — 2026-09-19

The final265 sweep failed `live_137_daemon_mode_parity::live_137_consent_accept_via_daemon`: target_count1/live_target_count0 after the readiness bound; dispatcher95/95, no frame in flight. These counters preserve the observation without proving a promotion mechanism. The separate blocked262 investigation branch retains its watcher-lifecycle diagnosis and unmet requirements. No265 change repairs this finding. Full346=344pass2fail, zero profile leaks. Evidence: `.git/ralph-loop/20260919-queue/iter265/live-sweep.log` and `supervisor-accounting.json`. Original262 ACs remain unchanged.

## Iteration270 sweep recurrence — 2026-09-19

The initial iteration270 dual-gate sweep again failed
`live_137_daemon_mode_parity::live_137_consent_accept_via_daemon` before consent:
readiness stayed false for15,135ms/47polls with daemon PID64449/proxy57180,
uptime17s, target_count1/live_target_count0, network buffer484, dispatcher alive
with83frames started/finished, no in-flight frame or RPC owner, and zero dropped
client writes. Exact serial dual-gate isolation reached a live target in110ms/one
poll and accepted Sourcepoint in5.75s. This is another target-lifecycle recurrence,
not a new consent-action result; the isolation does not repair it or count as a
green full sweep. All345 other names passed, all346 names/six tiers reconciled,
and every profile scan was clean. Iteration270 changed only hook guidance and its
live fixture, so no cause or repair is claimed. Original262 tasks, acceptance
criteria and distinct observed shapes remain unchanged. Evidence:
`.git/ralph-loop/20260919-queue/iter270/logs/final-live-sweep.log` and
`live-137-consent-rerun.log`.

After the iteration270 clippy-only style repair, a second complete dual-gate
sweep passed all346 names, including this test, with zero skipped/precondition/
timeout classifications and zero profile leaks or unattributed profiles. That
green sweep is final iteration270 closure evidence; it does not erase the first
recurrence or satisfy this plan's three-consecutive-sweep requirement. Evidence:
`.git/ralph-loop/20260919-queue/iter270/logs/final-live-sweep-after-style-repair.log`.

## Restart plan — 2026-09-19

This is preparation for a new session, not another execution result. Follow
[[ralph-loop-open-iterations-2026-09-19]] for queue, ownership and validation.
Recover checkpoint `e9c9e88c355782d9f645f4ca7a766b5d827e9be3` on this plan's
branch, integrate verified current main there, and preserve both sets of dated
observations plus branch-only plan275. The checkpoint has Locate2/2 and AC1/5;
main's older unchecked state must not erase that reviewed evidence. Restore only
evidence-backed tick/status differences, using Hyalo, after inspecting the merge.

**Question to answer.** Which exact Firefox branch prevents the replacement
window target from reaching this watcher? `target_count` is cumulative discovery,
not proof that a current live target exists. The daemon observed available →
destroyed without replacement. Idle direct watchers retained targets6/6 while
daemons lost5/6; adding legacy `getTarget` could induce direct loss. This supports
an experiment, not a proved suppression diagnosis or an actor-ID mismatch.

**First experiment (Astra implementation and review).**

1. Verify installed Firefox version, BuildID and source revision. The retained
   comparison used156.0 / BuildID20260909172920; local Firefox checkout HEAD was
   different. Use the retained exact-revision modules as locators, then verify
   against the browser actually running. Reuse the Rust comparison harness and
   ownership scripts under `iter262/attempt2/` in the previous run store.
2. Establish a supported way to observe the required Firefox process/module.
   In an exclusively owned browser/profile, trace each replacement window's
   browsing-context/window identity, watcher actor ID and connection prefix,
   watcher-registry membership, `findTargetActor` result and
   `createdFromJsWindowActor`, the selected suppression/creation branch,
   `onNewTargetActor` send, and parent acceptance or exception. Join these with
   the client's target-available/destroyed stream and existing daemon counters.
   Locate these checkpoints in retained `window-global.sys.mjs`,
   `target-actor-registry.sys.mjs` and registry/parent modules; verify line locations.
   Do not assume a parent-console evaluator can inspect a content-process registry.
3. First prove the probes execute on an ordinary passing replacement. Then use
   paired idle-watcher / watcher-plus-legacy-lookup controls with identical
   navigation and fresh owned profiles. Keep daemon and direct connection IDs
   separate; include a daemon observation. Record full15-second windows when
   claiming persistent loss. Begin with two paired trials; extend to at most six
   only if probes are complete but the failure has not recurred. Existing six-arm
   results are retained evidence, not a required repetition baseline.
4. Select a scoped correction only after a failing occurrence identifies whether
   the loss is legacy-target reuse/suppression, registry omission, delivery
   rejection or client bookkeeping. Verify the other boundaries explicitly before
   concluding. No global timeout increase, blind reconnect/reset or broad target
   redesign. Preserve resource subscription and concurrent consumer behavior.

The initial probe-feasibility budget is45minutes. A supported debugger or an
isolated instrumented Firefox build may be used if available; never replace the
installed browser or edit the shared Firefox source checkout in place. If a build
is needed, first record exact prerequisites and an honest bounded build plan; do
not silently begin an unbounded build. Missing tooling is a concrete blocker.
Needing instrumentation alone is remaining engineering work, not evidence of an
external blocker. Stop a repeated unchanged experiment when it adds no distinction.

**Proof and closing decision.** Retain a real-Firefox failing-before/passing-after
regression for the identified branch, plus focused protocol/ordering coverage.
Run the original three consecutive full dual-gate sweeps on stable final inputs;
all three named137/145 tests must be green. Apply the original separate rule for
`live_network_watcher_source_after_navigate_with_network`. Count every target,
qualified name and profile summary. A failure breaks the qualifying streak;
preserve it and diagnose before any new justified sequence. No retry loop merely
collecting three green samples. Preserve all original AC text and requirements.

The independent ready-target Guardian/Sourcepoint defects remain owned by275.
If those prevent the named137 criterion, leave it unmet: filing275 does not satisfy
it, and this five-iteration queue does not authorize implementing275. Record that
precise prerequisite instead of widening this iteration or weakening consent.

## Restart experiment — 2026-09-19, 22:18–22:27 CEST

Probe feasibility succeeded within the45-minute initial bound, without a native
Firefox build. An isolated copy of installed Firefox156.0 / BuildID20260909172920 /
SourceStamp `a80bd15ddee3b4bf3679aeba340e9d2db933c467` loaded three instrumented
DevTools modules from its own repacked browser `omni.ja`. The installed application
and shared Firefox checkout were not edited. The four relevant pristine packaged
modules matched the prior exact-revision source byte-for-byte. The probes record
content-process/window/browsing-context identity, watcher registry enumeration and
membership, connection prefix, `findTargetActor` source/result and
`createdFromJsWindowActor`, suppression/creation, send-before/send-after, and parent
receive/acceptance/exception. A passing direct control demonstrated every successful
replacement boundary before the paired experiment. No parent-console inference
about content registries was used.

The reused Rust comparison helper was temporarily adapted to connect to explicitly
owned raw browsers and to retain a full15-second post-navigation observation even
when daemon readiness returned early. It logs final daemon counters separately
from each direct connection. The first control lacked its home directory; the
first attempted pair block also mixed localhost and127.0.0.1 and fell back to direct
navigation. Those setup errors and their logs are retained, excluded from the valid
daemon comparison, and corrected before the six valid paired trials. The first
four valid pairs used minimal raw-profile preferences; the last two used the
product's USER_JS preferences plus dump logging. Each pair used identical profile
preferences/navigation in its idle and legacy arms. Browsers ran in bounded blocks
of four with independent homes/profiles; this is not a full sweep.

| Valid arm | Final observation after full15-second window |
|---|---|
| Idle direct watcher, pairs1–6 | All six retained3targets. |
| Direct watcher plus legacy `getTarget` every250ms | Pairs1/3/5 retained0targets;2/4/6 retained3. |
| Daemon connection in each of the12browsers | All12retained3targets, healthy dispatcher, no frame in flight. |

For the three failed **direct** connections, the Firefox branch is now observed,
not inferred. In valid-pair1-legacy, replacement innerWindowId17179869185 / BC13
was present in the content registry for watcher `server1.conn3.watcher3`.
`findTargetActor` found same-connection legacy actor
`server1.conn3.child13/windowGlobalTarget2`, with `createdFromJsWindowActor=false`;
`onWindowGlobalCreated` executed `suppress-legacy`, then the subsequent document
insertion took `ignoreIfExisting`. No replacement target was created/sent/accepted
for that watcher. Its client received availability for the old document followed
by destruction and no replacement during the full observation. On the **separate
daemon connection**, watcher `server1.conn2.watcher3` for that same replacement
window found no legacy match, created a target, sent it, and was accepted by the
parent. Pairs3/5 independently recorded the same direct suppression branch. No
parent exception or trace-helper error occurred in the valid pair logs.

This attributes the induced direct loss to legacy suppression and excludes registry
omission/parent rejection for those occurrences. It does **not yet attribute a
failed daemon occurrence**: that failure did not recur in these12 instrumented
browsers. The earlier uninstrumented daemon losses and their evidence remain open;
probe timing can perturb a race. The six-pair limit is exhausted, so no additional
unchanged sampling or green-chasing was performed. No repair was selected from the
all-green daemon observations. A shared-connection cached-target substitution would
still require precise tab identity, destruction-gap/cancellation/reply ownership,
resource-subscription and concurrent-consumer proof; the induced direct result
alone does not establish that implementation.

This is a bounded diagnostic stop, **not a missing-tooling/external blocker**.
Resume with a reviewed, bounded experiment that captures the original daemon loss
using these now-working probes (or a deterministic equivalent), then select the
scoped repair. Locate2/2 and AC1/5 remain unchanged. No before/after product proof,
three qualifying consecutive sweeps, network-source disposition from a fix, or
iteration completion is claimed;275 remains unexecuted.

Evidence root: primary checkout
`.git/ralph-loop/20260919-remaining-2211/iter262/`. Retained artifacts include
`instrument.rs`, `pristine/`, the private instrumented app/modules,
`comparison-diagnostic.rs`, `diagnostic.patch`, runner scripts, `control*`,
`pair*` setup-error logs, `valid-pair*/{meta,comparison.log,firefox.log,trace.jsonl,
final-status.json}`, and per-failure `attribution.json`. Diagnostic sources were
restored byte-for-byte and rebuilt; all source hashes match verified main's
`iter258-resume/frozen-source.sha256`, allowing reuse of its ordered passing commit
gates for this documentation-only recovery. Actual process cleanup retained all
run-owned profiles as evidence, terminated/reaped only recorded browser PIDs and
stopped owned daemons; desktop Firefox1112 remained untouched. No full-sweep profile
summary is asserted. Detailed commands, phase times, checks and stopping decision
are in the implementation report; actual token usage is unavailable.

## Original managed-path follow-up — 2026-09-19, 22:37–22:44 CEST

Review finding R1's distinct managed sequence was run within its fixed bounds:
the original `LiveFirefox::headless_on_random_port` launch sequence and tab-ready poll,
`with_daemon`, immediate Guardian navigation, the existing15-second live-target
wait, failure-time daemon status, then stop before consent. No direct watcher,
periodic `getTarget`, synthetic delay, consent call or iteration275 work was added.
One serial run and the permitted fixed block of three independently-owned
concurrent runs exhausted the four-run cap. The serial run and concurrent run2
reproduced the daemon loss; concurrent runs1/3 retained replacement targets.

The two failures have the same bounded wire observation. Daemon watcher
`server1.conn5.watcher3` received the initial top-level availability for
innerWindowId8589934593 / BC11 and then its destruction. Network resources prove
replacement innerWindowId17179869185 / BC11 existed, but no replacement
top-level `target-available-form` reached that watcher. At the end of15seconds the
daemon reported target_count1/live_target_count0 with an alive dispatcher, equal
started/finished frame counters, zero in-flight frame and no RPC-slot owner
(serial93/93; concurrent run2 94/94). The passing concurrent controls received the
replacement top-level availability and ended target_count3/live_target_count2.

The requested Firefox-side branch discrimination was not captured. The managed
launcher selected the private instrumented app and fresh managed profiles, and its
temporary hook retained both browser stdout and stderr, but each `firefox.log`
contained only headless/GFX startup lines and no `TRACE262` records. Supervisor
inspection found a concrete instrumentation error: the patch added the dump
preference inside `ensure_devtools_prefs`, called only for an explicitly supplied
profile. These fresh managed launches instead write `USER_JS` directly, which the
patch did not change. The worker's claim that dump was enabled in these fresh
profiles is therefore unsupported and corrected here. This is a setup defect,
not evidence that managed Firefox cannot emit the traces. These failures cannot be
assigned from this capture to legacy suppression, membership omission or parent
rejection. No causal product repair is
selected. The earlier six-pair stop remains in force, the original acceptance
criteria remain unchanged, and no further unchanged sampling is authorized by
this result. The four managed captures also exhausted their declared cap, including
the two failures with incomplete tracing. The precise resume action is to enable
dump on the actual fresh-profile creation path, retain proof of the effective
preference and an ordinary replacement's full trace before sampling, and use a
new explicitly bounded investigation allowance; do not silently reset this cap.
No further live run was performed to correct the instrumentation in this attempt.

Evidence is in
`.git/ralph-loop/20260919-remaining-2211/iter262-managed/`, including the exact
diagnostic patch, original source bytes, redacted client/daemon logs, launch
PID/profile/endpoints for the three fully-instrumented runs, restoration/rebuild
proof and the mechanical implementation report. The first serial launch predates
the launch-metadata print hook, so its browser PID/profile are unavailable; its
Firefox/daemon endpoints and wire trace are retained. All temporary Rust changes
were restored byte-for-byte, the relevant product-source diff against
`origin/main` is empty, owned browsers/daemons/listeners are gone, and desktop
Firefox PID1112 remained running. Actual token usage is unavailable.

## Additional authorized capture — 2026-09-20

The owner's new allowance preserves the exhausted earlier blocks: at most60minutes
active discovery and six targeted captures including controls. Independent design
and completed-instrumentation review preceded this block. Fresh managed USER_JS
now enabled dump logging in the temporary diagnostic build; saved preference bytes
and effective parent/content dump=true were observed. Private Firefox156.0 /
BuildID20260909172920 / SourceStamp a80bd15ddee3b4bf3679aeba340e9d2db933c467
was used without changing the installed app or shared Firefox source.

Capture stopped after2/6attempts. Attempt1 reproduced the original managed-daemon
loss: same daemon watcher server1.conn5.watcher3, replacement inner17179869185/BC11,
legacy actor server1.conn5.child16/windowGlobalTarget2. Explicit watcher membership,
context=true and fromJS=false lookup preceded suppress-legacy, then
suppress-ignore-existing. The initial top-level form was destroyed, with no
replacement top-level creation/send/parent/wire chain; readiness exhausted15.178s
with target1/live0 and healthy90/90dispatcher. The shared daemon connection's
getTarget request/reply and that actor's later replacement-window registry identity
were joined; the older reply's about:blank document was not misidentified as the
replacement. This attributes this occurrence, not every historical symptom.

Attempt2 supplied the mandatory complete ordinary replacement control before the
causal verdict: getWatcher(true), ordered watchTargets(frame), initial availability
and destruction, replacement membership/context, empty legacy lookup, creation,
send, parent acceptance, identical top-level wire form and live retained state.
Independent Astra evidence review returned zero findings. External workspace tests
in another session's Hyalo checkout overlapped attempt1; no uncontended timing or
host-wide exclusivity claim is made. Explicit actor/branch evidence supports the
attribution without inferring cause from timing.

All temporary Rust instrumentation was restored byte-identical and CLI/live artifacts
rebuilt. Both owned browsers/daemons and their four recorded ports were absent at
cleanup; desktopFirefox1112 was preserved. Generated preference copies, exact argv,
profile paths, command times/exits, browser metadata and cleanup observations were
retained contemporaneously. Raw daemon logs contain ephemeral local authentication
material and remain private; publish only sanitized derived evidence. This is not a
full-sweep profile verdict.

No product correction or new acceptance checkbox is claimed. Locate2/2, Fix0/2,
Test0/1 and AC1/5 remain. The precise demonstrated caller is navigation's timer
refresh, which invokes shared legacygetTarget while the watcher target is absent.
A proposed daemon-local disposable target-snapshot query is under separate design
review; it is not implemented or validated. Original live pre/post proof, three
qualifying full sweeps, watcher-source requirement and ordered gates remain owed.
Ready-target consent behavior remains275-owned and unexecuted.

Evidence: primary checkout `.git/ralph-loop/20260920-additional/iter262/`, notably
`capture/report.md`, `capture/phase-result.json`, `capture-review.md`,
`capture/attempt-ledger.tsv`, `external-cargo-note.md`, private `instrumentation/attempt-1/`
and `attempt-2/`, restored-source/build receipts and `fix-proposal.md`. The archived
diagnostic binary differs from the restored runner alias; do not rerun the old
capture command without reestablishing its reviewed inputs. This records the
accepted diagnosis while the authorized selected queue remains active.

## Additional block closeout — 2026-09-20, 03:00 CEST

The accepted capture1 diagnosis and complete ordinary capture2 remain valid. A
17-file correction was implemented, independently reviewed and repaired in two
product repair batches. Final source patch
`118e0fe4abe13fa92020336d283813f398faecc7647c36577eff79500bb08c11`
passed ordered fmt, strict workspace Clippy and workspace tests (2500 passed,
0 failed,419 ignored). The final scoped review returned zero findings and approved
the exact pre/post proof inputs. These checks did not establish live correctness.

Attempt3 launched one owned Firefox successfully, then the proof called unsupported
`daemon start`. The CLI returned exit2 and the test stopped before navigation or
the prevention assertion. The outer runner correctly failed for the missing daemon
receipt and failed command. This is a harness setup failure, not the required
pre-fix failing regression, and provides no new causal evidence. The original
review missed this command-contract defect; retain its verdict together with this
subsequent contrary execution evidence. Attempt4 was not run; no retry or sweep
was started. The browser/profile/port cleanup succeeded, and desktop Firefox1112
was preserved. Proof failure and runtime cleanup are separate results.

Both product repair batches are exhausted. The exact17 source files, full patch,
reviewed runner/manifests and existing frozen binaries are preserved under
`.git/ralph-loop/20260920-additional/iter262/final-correction-archive/` and
`repair-correction2/`. The worktree was restored to checkpoint72243fc source; the
failing live regression was not committed. Restored source matches the retained
565-file passing baseline manifest. This is a blocked documentation checkpoint,
not a partial implementation merge.

The new block used3/6 captures and3048/3600 active discovery seconds (3015 before
proof;33 rounded seconds for its launch, interpretation and stop decision).
Compilation and subsequent administrative preservation are separate.552 seconds
and three capture slots remain numerically; the exhausted repair ceiling blocks
further execution in this run. Earlier exhausted blocks are not reset.

Next entry requires an explicit continuation consistent with that repair ceiling
and retained discovery limits; recover the archived correction, replace the
unsupported command with the actual daemon-establishing CLI path, validate command
contracts before launch, and obtain fresh scoped review/rebuilt frozen identities.
Do not blindly reuse the old attempt3/4 script or overwrite consumed attempt3.
Then establish a valid before/after proof before the original real253 navigation
and same-document checks, three consecutive qualifying full sweeps with137/both145
green, watcher-source disposition, closing gates, independent review and exact-head
CI. Original Locate2/2,Fix0/2,Test0/1,AC1/5 remain unchanged.275 stays unexecuted.

Exact proof command/environment/timestamps/exits are in
`proof-supervisor/attempt-3-command.json`; raw command and runtime receipts are in
`correction-proof-3/`. Raw logs remain private. The stop/debit, immutable archive
manifest, restore result and later cleanup verification are retained alongside
the reviewed design and all earlier failures.


## Foundation integration preparation — 2026-09-20

The new explicitly authorized repair grant is separate from the preserved exhausted
historical batches. The archived correction is adapted beneath the foundation's
private `ConnectedTab` target installation: authenticated disposable primary-watcher
snapshots acquire metadata, and the existing installation boundary preserves same-
target handles and invalidates displaced consoles. Pending/errors never fall back
to shared legacy lookup; direct and explicitly unmanaged descriptors retain legacy
lookup. Navigation requeries under its existing deadline. Watcher document-start
translation and fresh same-document URL queries are retained.

The new proof uses supported `eval 1` autostart, retains its setup wire prefix
separately, and counts prevention only in the subsequent navigation/evaluation
phase. Fresh product-managed profiles remain mandatory; no explicit-profile helper
substitution was made. Matched foundation-only/pre and foundation-plus-correction/
post products share one rebuilt proof executable. New immutable runner inputs and
offline receipts live in `.git/ralph-loop/20260920-foundation-repair/iter262/`
of the primary checkout. Historical attempt3 and archived inputs are unchanged.

This is preparation for independent review, not a new live result. No capture or
sweep was launched in this phase. Original task/AC states remain Locate2/2,
Fix0/2, Test0/1 and AC1/5. A valid pre/post proof, real253 delayed/same-URL and
same-document regressions, three qualifying consecutive sweeps, watcher-source
disposition and final gates/review/CI remain owed.275 and259/266 remain outside
this correction's execution scope.


### Foundation adaptation review repair — 2026-09-20

Independent review found that best-effort refresh polled Pending for the full CLI
timeout before page-view settlement could inspect its own budget. Final new repair
batch2 restores one-snapshot best-effort semantics while retaining centralized
installation. Pending/errors leave metadata untouched and never trigger legacy
lookup; settlement owns retries. Fallible acquisition retains its explicit deadline.
Caller regressions cover persistent Pending, Pending then replacement, short/zero
budgets and missing identity. Initial frozen evidence and the rejected review remain
under batch1-freeze; new final inputs are under repair2. No new capture, acceptance
checkbox or live completion claim is made; fresh scoped review remains required.

## Foundation correction proof and blocked regression —2026-09-20

PR263 foundation merged through GitHub at8666a5324905793538c59532e909e0808beee7f3. Two newly authorized repair batches adapted the archived correction beneath central target installation and repaired the independently identified settlement refresh polling issue. This is prevention through acquisition ownership: daemon-managed callers read the primary watcher's retained snapshot over a disposable authenticated connection rather than issuing a shared legacy getTarget that suppresses replacement watching. It is not a counter-promotion repair or timeout increase. Independent final scoped review approved the frozen implementation and exact pre/post attempts with zero new findings.

One identical real-Firefox regression executable ran against matched foundation-only/pre and foundation-plus-correction/post binaries. Attempt4 failed the intended zero-legacy assertion (4 calls after the setup boundary;setup1), with all54 command receipts successful. Navigation/evaluation exited0; readiness was false and no live replacement form remained. Attempt5 passed:zero legacy calls in both phases, readiness true, initial/replacement forms and final evaluation through the replacement's console actor. All8 command receipts and both attempts' inner/outer cleanup passed. The count covers navigation/readiness/evaluation, not exclusively navigation; historical reviewed Firefox-side causal capture supplies branch attribution. Attempts3 and all older evidence remain intact.

Required real253 follow-up ran once, serially:7passed/1failed. live_253_committed_submission_daemon returned the committed destination page but took4.248194792s and failed the original elapsed<2s settlement-time assertion at live_253_outgoing_page.rs:357. No unsupported cause or passing-rerun substitution is claimed. Both new repair batches are exhausted, so no additional repair or closing sweep was started. This failure remains owned by262's required regression validation; filing a carry-over would not discharge it.

Ordered fmt/strictClippy/full workspace tests passed with RUST_TEST_THREADS=1 (2516pass/0fail/419ignored). Default-parallel workspace tests twice failed an unchanged foundation timeout-receipt test; exact isolation passed but did not explain the failures. [[iteration-278-parallel-launch-timeout-missing-receipt]] preserves that distinct issue, outside execution scope. The serial pass does not tick the original default-validation gate.

Current tasks:Locate2/2,Fix1/2,Test1/1,AC2/5. Land-fix task, three qualifying consecutive full sweeps, watcher-source disposition and original ordered clean gate remain unticked. No262PR/merge or product commit occurred. The initial checkpoint retained an uncommitted foundation merge/correction; failing code was not committed. Exact source bytes, staged/unstaged patches and file manifest are recoverable under primary .git/ralph-loop/20260920-foundation-repair/iter262/blocked-checkpoint/. Final frozen inputs/reviews/proof/253 logs are under repair2/. A prepared sweep runner exists but was never executed.

Historical discovery3048s/3captures and old2repair batches remain consumed. This block spent2new repair batches,2additional captures (total5/6) and270s retained discovery (282s numerically remain); no allowance reset. New preparation debit is recorded separately in allowance-ledger.json. DesktopFirefox57827 was preserved by both proof identity guards; the earlier disappearance of desktop1112 has no established cause. Further262 work requires authority consistent with the exhausted new repair cap and must preserve these failed validations and original requirements.

### Documentation checkpoint restoration

At15:48:51CEST the supervisor verified all569 frozen correction inputs against both the worktree and the retained full-byte archive, then restored the checkout's product source and actor documentation to the merged foundation. All567 restored build inputs match each recorded passing PR263 fmt→strictClippy→workspace-test manifest (2499pass/0fail/418ignored). This authorizes reuse only for the restored documentation checkpoint; it does not erase the correction's253/parallel failures or satisfy its remaining ACs. Both correction binaries, source patch, all source bytes and proof evidence remain recoverable. Restored CLI/live/e2e/unit artifacts rebuilt successfully in15.720s, exit0; command and Cargo JSON receipts are in blocked-checkpoint/restored-build-* before the checkpoint commit.


## Focused caller repair — 2026-09-20, stopped

The owner adopted a separate 90-active-minute preparation/implementation/review
grant for 262 with two independently reviewed implementation/repair batches. Both
were used. The first repaired submission handover and interrupted console replies;
independent review found outgoing-target loss during metadata acquisition. The
second repaired typed lifecycle retry and late metadata reply retirement under
the same absolute deadline. A fresh final review resolved that finding and returned
zero new actionable findings. Twenty-one actual-caller scenarios cover settle,
predicates, page output, outgoing/Pending/replacement states, bounded exhaustion,
interrupted acknowledgments/results, same-document URLs and protocol-error fail-fast.
Before regressions failed and after regressions passed; no assertion was weakened.

Final candidate ordered stable/fmt/strict workspace Clippy/default-parallel tests
passed 2519/0/419. The original live253 suite passed 8/8; committed submission was
1083ms daemon and 738ms direct, within the unchanged <2s bound. The historical
4.248194792s failure remains preserved without retroactive causal attribution.
Targeted required137, both145 and watcher-source cases each passed.

One full dual-gate closing sweep then finished 346passed/1failed across all347
exact compiled names in six tiers. Summary: executed=347, skipped=0, preexisting=0,
vanished=0, launch_timeout=0, timed_out=0, total=347; profile summary leaked=0,
unattributed=0. Required137, both145 and watcher-source passed within this sweep.
The sole failure was live_262_watched_target_prevention_contract at supported
eval1 daemon-autostart setup, before navigation/prevention assertions: product
exit124, Timeout waiting for watched tab target, outer timed_out=false. The trace
records one about:blank top-level availability (inner21), then destruction and no
replacement before failure; zero shared getTarget and zero listFrames sends.
Cause is unproven; this does not establish recurrence of legacy suppression.

No second sweep, fresh capture, third repair, product commit, PR or merge ran.
The required clean consecutive-sweep sequence remains unmet. Original task/AC
wording and checkbox state remain unchanged (AC2/5); the passing candidate gates
and live253 results are retained without claiming iteration delivery. Scope was
not widened to another iteration. The failed setup remains owned by262.

All569 candidate source inputs, four actor docs, full patch and CLI/live/xtask
binaries are archived under primary .git/ralph-loop/20260920-focused262/batch2/
and blocked-checkpoint/. Final review2 permits scoped historical proof4/5 reuse
for the unchanged prevention contract, not exact-function-byte reuse or live
measurement of the new metadata retry behavior. The source was restored to the
foundation and rebuilt; all567 restored inputs match each historical ordered
passing PR263 gate manifest, reused only for this documentation checkpoint.
No failing product source is committed. Runtime cleanup passed, including the
sweep's owned raw browser, port6000 and failed-proof cleanup; desktop57827 remained.

Preparation through final review approval was conservatively charged2209/5400s,
without compile/idle deductions;3191s numerically remain but no repair batch.
Final acceptance validation and preservation are separately recorded. Historical
ledgers remain unchanged:282discovery seconds, one unused capture slot (5/6used),
and the old85s preparation remainder. Actual token totals and a comparable
supervision baseline are unavailable. Evidence: focused run state.json,
review1/review.md, review2/review.md, validation/live253/, sweep-1/reconciliation.json,
sweep-1/exact-name-reconciliation.json, sweep-1/failure-derived.json and private
failed-proof receipts, plus blocked-checkpoint/restore-recipe.md. Primary handoff:
research/rdp-262-focused-results-2026-09-20.md. Verify final cleanup/lock-release
and checkpoint receipts before any authorized resume; no background continuation.
