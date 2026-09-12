---
title: "Iteration 253: --with-page cannot tell a fast outgoing-document answer from the real destination"
type: iteration
date: 2026-08-31
status: done
branch: iter-253/with-page-outgoing-doc-race-detection
depends_on: [220]
first_call_sites: []
dogfood_path: >-
  FF_RDP_LIVE_TESTS=1 FF_RDP_LIVE_NETWORK_TESTS=1 cargo run -p xtask --
  check-dogfood-script kb/iterations/iteration-253-with-page-outgoing-doc-race-detection.md.
  Exercises a non-navigating control and full-payload 50/700/3200/6000 ms
  destination matrix on direct and daemon routes. Destinations that have not handed over
  their document must not return an outgoing view with meta.page_ready=true.
tags: [iteration, act-and-see, page-view, carry-over, defect]
dogfood_script: iteration-253-with-page-outgoing-doc-race-detection.dogfood.sh
implementation_evidence_2026_09_12: "Baseline09cfee1c: full-payload click --no-wait --with-page, fresh Readability injection on each document, Firefox155.0.1. Delays50/700ms returned destination ready=true on both routes; delays3200/6000ms returned outgoing ready=true on both routes (daemon3160/3144ms; direct3099/3079ms; parse1–4ms). The residual is reachable after the existing3s settle cap, not during a concurrent settle poll: collection follows the poll. Conservative labeling closes false confidence without making the action hostage to the destination: the settle outcome now gates page_ready. No broader deadline fix or unbounded navigation wait. Pre-fix2test failures and post-fix results retained in the assigned iter253 artifacts. Original AC wording remains unchanged; final closure pending."
outcome_2026_09_12: |-
  ## Outcome — 2026-09-12
  The outgoing-document answer is reproducible on post-220 main
  `09cfee1c6464896394f0e5ee1877b3894431cd06`, with the normal full collection and
  cold Readability injection, on Firefox 155.0.1. The actual opening is after the
  existing three-second settlement bound. Collection follows settlement; it does
  not run concurrently with the next settle poll. No reduced payload or artificial
  teardown event was used. Delaying the destination's first byte delays its commit
  and leaves the outgoing document able to answer.
  | Route | Destination delay | Before: heading / readiness / action wall time | After: heading / readiness / action wall time |
  |---|---:|---|---|
  | daemon | 3200 ms | outgoing / true / 3160 ms | outgoing / false / 3128 ms |
  | daemon | 6000 ms | outgoing / true / 3144 ms | outgoing / false / 3150 ms |
  | direct | 3200 ms | outgoing / true / 3099 ms | outgoing / false / 3092 ms |
  | direct | 6000 ms | outgoing / true / 3079 ms | outgoing / false / 3089 ms |
  The 50/700 ms controls returned the destination, ready, on both routes before
  and after. Every sample freshly injected Readability, ran the headings and
  interactive scan and reader pass, and reported parse time 1–4 ms before the fix.
  The two vendored reader inputs alone are 36,819 bytes; the injection wrapper and
  collection program are additional. Parse time is not whole-collection latency,
  and these measurements do not establish the historical Wikipedia 55 ms teardown
  timing for this fixture.
  The fix closes the confidently-wrong answer through the plan's labeling
  mechanism: `settle_after_navigation` returns whether it observed document
  handover, and `collect_settled` intersects that with `page.ready`. A complete
  outgoing DOM therefore remains useful but explicitly unready when the bounded
  wait expires. Checking a fresh target only *after* collection would not prove
  that the already-collected view belongs to that target. The navigation latch,
  teardown guard, bounded retries and no-navigation fast path remain intact; no
  iteration 258 deadline work is included. The no-signal branch still returns
  immediately and now labels uncertainty. A missing new ID is not positive evidence
  of a changed document. README documents the semantics.
  Two live tests fail on main and pass after the fix. Reversing the readiness
  intersection makes the direct live test fail again. A non-live test drives the
  real settlement loop against the recorded target shape, covering unchanged,
  missing, changed, fragment and no-signal identity cases. Mutating the expired
  poll to report success makes that test fail. Both mutations were restored.
  The conditional unreproducibility task remains unticked because its premise is
  false; no unreproducibility claim or extra carry-over work is appropriate.
  Validation: stable updated to Rust 1.98.1; ordered formatting, strict Clippy
  0.1.98 and workspace tests passed (2445 passed, 0 failed, 404 ignored across
  36 result summaries). An initial unit-mock handshake error and missing
  daemon-parity annotation were repaired before the passing run. The original
  dogfood control used a data URL without its required opt-in, and correctly
  received the existing User error; the corrected control populates the owned
  blank document with `eval`. Its gate then passed, including both live routes and
  a fresh sentinel. These were authoring/setup errors, not deferred product bugs.
  The closing sweep used both `FF_RDP_LIVE_TESTS=1` and
  `FF_RDP_LIVE_NETWORK_TESTS=1`, with a raw owned Firefox on port 6000:
  ```text
  LIVE_SWEEP_SUMMARY executed=332 skipped=0 preexisting=0 vanished=0 launch_timeout=0 timed_out=0 total=332
  LIVE_SWEEP_PROFILES leaked=0 unattributed=0 root=/Users/james/Library/Application Support/ff-rdp/profiles
  ```
  CLI: 315 passed, 8 failed. Core tiers: 1 + 3 + 3 + 2 passed, zero failed.
  Total 324 + 8 = 332. All 332 compiled qualified names match actual verdicts;
  there are no missing, extra or reclassified results. All five real-root scans
  and the final profile summary are preserved. Both new regressions and all five
  iteration 220 live tests passed in the sweep. All nine actual xtask checks ran
  afterward and passed after the dogfood correction. Firefox refs had no declared
  references to validate; actor/KB sync had no changed actor source to check.
  ## Carry-over — 2026-09-12
  | Observation | Disposition |
  |---|---|
  | `live_135_screenshot_ff153::live_135_screenshot_full_page_taller` failed in sweep and exact isolation with fourth-argument dictionary TypeError | Fold: [[iteration-257-firefox-155-drawsnapshot-dictionary-arg]], already explicitly covers this test |
  | `live_144_session_hygiene_followup::live_144_full_page_no_duplicate_header` same error in sweep and exact isolation | Fold: [[iteration-257-firefox-155-drawsnapshot-dictionary-arg]] |
  | `live_61l::live_screenshot_full_page` same error in sweep and exact isolation | Fold: [[iteration-257-firefox-155-drawsnapshot-dictionary-arg]] |
  | `live_61r_screenshot::live_screenshot_full_page` same error in sweep and exact isolation | Fold: [[iteration-257-firefox-155-drawsnapshot-dictionary-arg]] |
  | `live_92_screenshot_full_page::pre_fix_repro_screenshot_full_page_taller_than_viewport` same error in sweep and exact isolation | Fold: [[iteration-257-firefox-155-drawsnapshot-dictionary-arg]] |
  | `live_92_screenshot_full_page::live_screenshot_full_page_md5_differs_from_viewport` same error in sweep and exact isolation | Fold: [[iteration-257-firefox-155-drawsnapshot-dictionary-arg]] |
  | `live_screenshot_shim::live_screenshot_unchanged_after_shim` same error in sweep and exact isolation | Fold: [[iteration-257-firefox-155-drawsnapshot-dictionary-arg]] |
  | `live_137_daemon_mode_parity::live_137_consent_accept_via_daemon` failed after 15038 ms / 47 polls; daemon PID 80419, uptime 16 s, target_count 1 / live_target_count 0, dispatcher alive and 90/90 frames, network buffer 464; isolation passed in 5.60 s (target ready in 29 ms / one poll) | Fold: [[iteration-262-daemon-live-target-never-promoted]] and record condition 8 recurrence in [[iteration-203-live-sweep-watch-conditions-third-holder]]. A successful isolation is not a fix or one of 262's required three green sweeps |
  | Conditional task to explain inability to reproduce remains unticked | No plan: race reproduced with the full payload; this alternative premise is false |
  | Failed authoring/setup attempts: unit mock handshake, missing daemon-parity annotation, dogfood data URL opt-in | Closed in this PR: raw mock connection, actual parity annotation and eval control; passing unit/workspace/dogfood evidence retained |
  | Plan body editor unavailable in installed Hyalo | No product plan: substantive current outcome/carry-over evidence is stored in dated Hyalo metadata; this exact body addendum is supplied as an unapplied patch, and original AC wording is preserved |
  No post-auth timeout, pre-auth EOF, timing-bound, omitted-name, watchdog or leak
  trigger occurred in this sweep. Their existing plans and unticked ACs remain
  open. Root owns independent review, final status and reconciliation of every
  upcoming pending plan before publication.
precheckpoint_outcome_2026_09_12: |-
  # Iteration 253 pre-checkpoint edge verification
  This followup supersedes the initial worker handoff for current source and closure evidence. Initial reports, sweeps, and failed setup attempts are preserved as historical artifacts, not reused as the final sweep.
  ## Both supervisor cases
  1. **Late navigation starts: confirmed and closed.** The existing transport permits a start before a successful `getTarget` reply, and permits a full collection reply after a start when the target omitted its optional document ID. A scripted transport regression sends these messages in a deterministic order, using recorded target/result shapes and the actual complete Readability injection. It also covers a new start after a previous navigation settled. Baseline failed all three originally tested cases; the expanded four-case test fails all four when the final latch intersection is removed. `collect_settled` now consumes the latch after every collection, including successful ones, and labels that collected view unready when a new start was observed. It does not try to bind an old view to a later target probe. A reconnect retains the latest latch. Existing navigation latch and target guard remain in place.
  2. **Same-URL cross-document replacement: confirmed and closed.** A localhost fixture serves generation 1 immediately and delays generation 2 at the identical `/same` URL for six seconds. Full collection on real Firefox155.0.1 returned generation 1 confidently ready before the fix: daemon135ms, direct49ms. Actual before/after evals show the identical URL and generations1/2; both route envelopes are asserted. The fix permits URL-based settlement only when a known pre-navigation URL differs from the announced destination. Same document ID plus already-equal URL no longer proves handover. Corrected live tests return generation1 unready after the bounded wait, then observe generation2 at the identical URL. Final fragment controls show the URL changing to `#here` while generation2 remains, page_ready=true, daemon142ms/direct30ms. Unit controls retain changed-ID redirects, fragment URL changes, missing-ID behavior, and the no-signal single-probe fast path. Removing the URL distinction fails the same-ID/same-URL unit regression.
  The first same-URL test attempt exposed fixture setup errors (nonblocking accepted sockets and JSON string parsing) and a daemon-host mismatch that selected direct fallback. These were fixed in the fixture/command setup, and all route assertions passed in the corrected before/after tests. They are not counted as product regressions. No broader architecture, timeout-budget258, screenshot257, or daemon-promotion262 work was performed.
  ## Validation completed before sweep
  - `before-late-event-all.log`: regression failed before the late-latch fix.
  - `before-same-url-corrected.log`: two meaningful pre-fix live failures with actual route and before-URL evidence.
  - `after-live.log`: four tests passed, full delayed-destination matrix plus same-URL replacements on both routes.
  - `mutation-late-latch.log` and `mutation-same-url.log`: exit101 at the intended assertions; exact correct source restored after each.
  - `final-fragment-live.log`: two tests passed, 23.53s; both same-URL and fragment controls.
  - `rustup.log`, `fmt.log`, `clippy.log`, `workspace-test.log`: ordered gates exit0 on final source. Workspace2446passed,0failed,406ignored,36summaries.
  - Sweep input frozen in `sweep.tree` and `sweep.patch`: tree95eedae226ee976937c55bda15a83fd35306d845. Real Git index remains byte-identical to baseline; no commits or publication.
  Final dual-gate sweep16:36:24–16:41:14CEST:334executed=322passed+12failed; CLI313+12 and core1+3+3+2. Exact334 names reconcile, all reclassification/skip counts zero, all five profile-root checks clean, final leaked0/unattributed0. Every failed test received an exact isolated rerun. Detailed individual dispositions are in carryover.md. All nine xtask gates ran after the sweep and exited0, including actual dogfood non-navigating control and all four live tests48.61s. Owned rawFirefox56568 stopped/reaped143, port6000 free; unrelated desktop37270 preserved. Final status/review/checkpoint are supervisor responsibilities.
precheckpoint_carryover_2026_09_12: |-
  ## Carry-over from the final pre-checkpoint sweep
  The closing sweep ran with both live gates enabled. It executed334=322passed+12failed (CLI313passed+12failed; core tiers1+3+3+2passed). Exactly334 qualified names match334 actual verdicts, with no missing/extra names. All skips, preexisting, vanished, launch_timeout and timed_out counts are zero. Each of five actual managed-profile-root checks reports no live-owned profile left behind; final leaked=0/unattributed=0. This supersedes the initial253 sweep332=324+8 as current evidence; that initial sweep remains historical.
  | Exact failed test or diagnostic | Disposition and evidence |
  |---|---|
  | live_135_screenshot_ff153::live_135_screenshot_full_page_taller | Fold257: Firefox155 fourth-argument dictionary TypeError; exact isolation also failed2.47s. |
  | live_144_session_hygiene_followup::live_144_full_page_no_duplicate_header | Fold257: same explicit dictionary TypeError; isolation failed2.29s. |
  | live_61l::live_screenshot_full_page | Fold257: same explicit dictionary TypeError; isolation failed2.39s. |
  | live_61r_screenshot::live_screenshot_full_page | Fold257: same explicit dictionary TypeError; isolation failed2.70s. |
  | live_92_screenshot_full_page::pre_fix_repro_screenshot_full_page_taller_than_viewport | Fold257: same explicit dictionary TypeError; isolation failed2.44s. |
  | live_92_screenshot_full_page::live_screenshot_full_page_md5_differs_from_viewport | Fold257: same explicit dictionary TypeError; isolation failed2.46s. |
  | live_screenshot_shim::live_screenshot_unchanged_after_shim | Fold257: same explicit dictionary TypeError; isolation failed2.80s. |
  | live_137_daemon_mode_parity::live_137_consent_accept_via_daemon, sweep | Fold262 and203 condition8: promotion failed15108ms/47polls. DaemonPID60752 uptime16s, target1/live0, dispatcher99started/99finished, in_flight0, no RPCowner, network buffer438. This is the original promotion signature. |
  | Same consent test, exact isolation | Fold262 separately: live targets ready37ms/1poll, then Sourcepoint consent_not_actioned/detected_not_actioned. Testfailed5.81s. This is the previously documented consent-action observation; it does not reproduce promotion failure or imply a common cause. |
  | live_161_eval_and_flag_strictness::live_161_fields_and_sort_reject_unknown_names | Fold267: autostart eval1 exited124 with post-auth Timeout before flag assertions. Firefoxport58109. Isolation passed4.99s. Failed-occurrence auth/request/dispatcher timing remains uncaptured and required; no cause established. |
  | live_219_reader_view::live_219_collection_leaves_the_dom_byte_identical | Fold267: eval of DOM length/ref count returned post-auth Timeout; daemon proxy60330. Isolation passed5.13s. No causal or DOM-mutation conclusion follows; preserve missing failed-occurrence timing. |
  | live_240_daemon_frame_desync_and_wedge::live_240_sustained_hops_never_desynchronise | Fold267: hop28 of40 failed with post-auth Timeout on origin page, reconnects0, daemon proxy61263. Isolation passed all40hops28.71s. This is DISTINCT from prior pre-auth EOF at hop27 carried by203; neither frame desynchronization nor common cause is proved. |
  | live_styles_applied::live_styles_applied_returns_real_rules | Fold203 as a new explicit watch: readystate navigation to a data fixture exited successfully, then styles p --applied returned User/no element matching selector p. Isolation passed2.21s. The test never requests --with-page, so it does not execute253's changed collection path; no cause or environmental explanation established. On another unpaused sweep or exact-isolation recurrence, capture navigation envelope, before/after URL, document identity/readyState/DOM, route and styles actor target, then file a scoped diagnostic plan. Do not raise waits or infer a style-filter defect from a missing element. |
  | Initial same-URL fixture setup and route errors | Closed here: accept sockets changed to blocking with bounded read; JSON string result parsed; commands explicitly match daemon registry host127.0.0.1 and assert actual route. Corrected meaningful pre-fix failures and post-fix passes retained. No product issue assigned from invalid setup. |
  | Conditional unreproducibility task | No plan, false premise: the realistic complete-payload race was reproduced on real Firefox and receives detection/labeling. Original conditional task remains unticked and wording unchanged. |
  | General Markdown body editing unavailable through Hyalo | Metadata contains current dated Outcome/Carry-over. Original task/AC wording preserved; old heading counts remain honest tooling debt. Unapplied body patch supplied, never raw-applied. Root owns final status and disposition before publication. |
  | Raw profile cleanup | Owned Firefox56568 stopped and waited143; port6000 free, desktop37270 preserved. Fresh raw profile retained as an optional artifact. Earlier rawprofile removal was automatically rejected; no retry or approval requested. No running browser is retained. |
  Plans264's three named timing tests passed this sweep; no new timing-bound trigger or distribution evidence is claimed. No pre-auth EOF recurred. Existing203/262/267 ACs remain unmet and untouched. Canary/locale watch checks and every-upcoming-plan reconciliation remain supervisor responsibilities before checkpoint/merge.
precheckpoint_final_validation_2026_09_12: >-
  Final source tree95eedae226ee976937c55bda15a83fd35306d845 completed
  stable-update/fmt/clippy/workspace tests in order:2446passed,0failed,406ignored,36summaries.
  Final dual-gate sweep16:36:24–16:41:14CEST:334executed=322passed+12failed;
  CLI313+12 and core1+3+3+2; exact334names, no
  skips/preexisting/vanished/launch_timeout/timed_out or profile leaks. All actual9xtask checks ran afterward and exited0;
  real dogfood executed non-navigating control plus all4 live regression tests48.61s
  and wrote a fresh sentinel. Firefox-reference check has no declared refs;
  actor-sync compares base...HEAD and the entire working diff separately confirms no
  actors changed. Current carried failures are explicit in
  precheckpoint_carryover_2026_09_12; historical332sweep is not final closure evidence. Owned rawFirefox56568
  stopped/reaped143, port6000 free, unrelated desktop37270 preserved. No commit,
  publication or independent review performed; supervisor owns final
  status/checkpoint/review and all-upcoming-plan reconciliation. Evidence directory
  .git/ralph-loop/20260912-validation-efficiency/iter253/precheckpoint-edge-verification.
supervisor_closure_2026_09_12: "2026-09-12 latest supervisor closure after formal review repair 1: verified frozen implementation tree d31f3255abdde513cf3fc541230ff9c6b7a3428b against swept source 30f6d6c957610fb16369c501b826f03025bc0c57. Product, tests, README and dogfood match. All 336 distinct expected names equal 336 observed verdicts: 328 passed and 8 failed across five tiers; all five profile checks clean, no missing/extra names or reclassifications. Ordered stable-update/fmt/strict-Clippy/workspace gates passed (2447 passed, 0 failed, 408 ignored), and all nine actual xtask gates passed including actual dogfood. Seven screenshot failures reproduce in isolation and remain assigned to257; daemon target promotion failed this sweep and passed isolation, with the distinct earlier consent-action failure retained under262. Prior post-auth Timeout and styles observations passed this sweep without establishing causes or satisfying267/203 diagnostic requirements. Complete named dispositions and historical sweeps remain in dated metadata. Both original ACs are fulfilled; the conditional unreproducibility task remains unticked because the realistic race reproduced. Status done describes implementation and closure, with fresh independent exact-head review, CI and authorized merge still owned by the supervisor. All17 pending plans were reread and reconciled before finalization; no broader backlog executed. Next254, then256 ThemeA verified/exported prerequisite before255 measurement and merge, followed by256 final measurement/single PR, the separate242 sweep,246 KB-only correction and257 compatibility work. Hyalo-only KB operations preserved: correct heading counts A1/2, B-or-C1/1, AC2/2 are disclosed here; original stale body counters and AC/task wording remain unchanged. First-head closure is historical under supervisor_closure_first_head_2026_09_12."
review_repair_1_started_2026_09_12: "First formal independent-review repair on PR249 head f32ca468402b2fcb983c0354759d68cfc2b41ff4: preserve originating document identity across type caller refreshes and collection reconnects. Prior334 sweep is historical after source edits. Supervisor metadata/history and original ACs remain intact; current repair evidence will supersede readiness claims."
review_repair_1_outcome_2026_09_12: |-
  ## First formal review repair — originating document identity
  The independent review of PR249 headf32ca468402b2fcb983c0354759d68cfc2b41ff4 identified one P2: target refreshes in type submission and collection reconnect replaced the origin with the destination, making settlement compare the destination to itself. This repair addresses both paths. Earlier253 sweeps, including334=322passed+12failed, are historical after these source changes.
  `NavigationOrigin` is a small command-scoped ID/URL snapshot. Type captures it before the action and passes it to page collection after either submission refresh path; other attachment callers keep the existing entry point. Collection preserves the origin while navigation is pending, across polls and reconnects. Positive handover retires that pending navigation; a later connection failure does not prove the same transition again. A new start during collection uses that collection target as the next origin and keeps the existing conservative labeling. The existing navigation latch, scoped teardown guard, attempt/reconnect limits, settle bound and overall deadline remain intact. No transport deadline258 or broader architecture change is included.
  ### Meaningful before/after evidence
  - RealFirefox155.0.1 `type input origin --submit --with-page`, fresh full Readability injection: before daemon3928ms/direct3842ms, committed destination heading and navigated=true but page_ready=false. After daemon907ms/direct796ms, the same committed destination is ready. Both actual routes and before/after URLs are asserted and logged.
  - Deterministic recorded-shape RDP protocol: mode0 caller already refreshed to destination; mode1 confirmed handover then collection EOF; mode2 unconfirmed handover then collection EOF followed by a committed destination on reconnect. All three failed before repair. Reconnect uses actual TCP EOF and a fresh greeting/listTabs/getTarget handshake, not an injected replacement.
  - Correct budgeted test: mode0=5ms, mode1=6ms, mode2=3016ms; all ready. Mode2 retains the original3000ms settle allowance inside a3500ms overall budget, then needs exactly one target probe after reconnect. Modes1/2 assert attempts2/reconnects1. All recovered collections execute the readiness probe and full reader injection. A subsequent start is received successfully after the scoped target guard is disarmed.
  - Caller-origin mutation re-captures the destination after submission: both live tests fail again. Reconnect-origin mutation replaces the saved origin with the fresh target: exactly mode2 fails on an extra target probe at3511ms. Both mutations restored byte-for-byte before final gates.
  - Six live regression tests passed54.23s, preserving delayed outgoing-page labeling, same-URL cross-document replacement, and positive fragments alongside committed submissions. All three253 unit tests passed with missing-ID/no-signal, changed-ID redirects, fragment, same-URL and late-start controls.
  Test-authoring errors are retained separately: an initial debug-print compile failure, and the first budgeted mutation fixture omitted the readiness probe and therefore did not constitute mutation evidence. Both were corrected before the meaningful mutation and final ordered gates. No authoring error remains deferred.
  Final source: tree30f6d6c957610fb16369c501b826f03025bc0c57. Ordered stable-update/fmt/clippy/workspace gates passed2447tests/0failed/408ignored/36summaries. Actual closing dual-gate sweep336=328passed+8failed, all five tiers/exact336names reconciled, all skip/reclassification/profile-leak counts zero. The eight failures received exact isolated reruns and explicit dispositions in carryover.md. All nine actual xtask checks ran after the sweep and passed, including non-navigating dogfood control plus all six live regressions59.76s and a fresh sentinel. Source remains identical to the swept tree. Status is active repair until the supervisor's final closure/review; original AC wording and prior supervisor history are preserved through Hyalo.
review_repair_1_carryover_2026_09_12: |-
  ## Carry-over — first formal review repair
  Current closing sweep used both live gates on source30f6d6c957610fb16369c501b826f03025bc0c57, from17:19:16 to17:24:17CEST. LIVE_SWEEP_SUMMARY executed=336 skipped=0 preexisting=0 vanished=0 launch_timeout=0 timed_out=0 total=336. CLI319passed+8failed; all four core tiers1+3+3+2passed. Thus328+8=336. All336 qualified compiled names match336 actual verdicts exactly; no missing, extra or reclassified names. All five real managed-profile-root checks are clean, final leaked0/unattributed0 at /Users/james/Library/Application Support/ff-rdp/profiles. Older334/332sweeps remain historical.
  | Observation | Disposition |
  |---|---|
  | live_135_screenshot_ff153::live_135_screenshot_full_page_taller | Fold257: explicit Firefox155 drawSnapshot argument4 dictionary TypeError, also failed exact isolation2.50s. |
  | live_144_session_hygiene_followup::live_144_full_page_no_duplicate_header | Fold257: same explicit TypeError, isolationfailed2.36s. |
  | live_61l::live_screenshot_full_page | Fold257: same explicit TypeError, isolationfailed2.47s. |
  | live_61r_screenshot::live_screenshot_full_page | Fold257: same explicit TypeError, isolationfailed2.69s. |
  | live_92_screenshot_full_page::live_screenshot_full_page_md5_differs_from_viewport | Fold257: same explicit TypeError, isolationfailed2.41s. |
  | live_92_screenshot_full_page::pre_fix_repro_screenshot_full_page_taller_than_viewport | Fold257: same explicit TypeError, isolationfailed2.46s. |
  | live_screenshot_shim::live_screenshot_unchanged_after_shim | Fold257: same explicit TypeError, isolationfailed2.85s. |
  | live_137_daemon_mode_parity::live_137_consent_accept_via_daemon | Fold262/203 condition8: sweep promotionfailed15005ms/47polls, Firefoxport53180, daemonPID43194/proxy53309, uptime16s, target1/live0, buffer451 networkevents, dispatcher78started/78finished, no inflight work or RPCowner. Isolation reached live targets113ms/1poll and passed5.61s. Passing isolation does not fix promotion or supply a green sweep. Historical ready-target Sourcepoint consent_not_actioned remains a distinct unmet262 observation. |
  | Prior post-auth Timeout recurrences | Fold267 unchanged: all three prior named161/219/240 rows passed this sweep. This does not establish a cause or supply missing attributable failed-occurrence auth/request/dispatcher timing. All original267 ACs remain unticked. No new post-auth Timeout or pre-auth EOF occurred. Prior pre-auth EOF remains a distinct203 watch. |
  | Styles selector missing after readystate navigation | Fold203 unchanged: live_styles_applied_returns_real_rules passed this sweep. Its recorded recurrence trigger did not fire; capture-and-file requirement remains intact for a future recurrence. No cause or closure claimed. |
  | Three264 timing-bound cases | Fold264 unchanged: all three passed this sweep; no required isolated/loaded distribution was measured, and no bound was changed. |
  | Formal review P2, caller refresh and reconnect origin loss | Closed by this repair: command-scoped originating identity, confirmed pending-navigation retirement, deterministic actual reconnect tests and live committed submissions. Before/after/mutation evidence in repair-outcome.md and named logs. |
  | Test-authoring failures | Closed by this repair: initial Debug-print compile error and missing readiness-probe fixture response repaired before meaningful mutation/final gates. Earlier invalid logs retained and not counted as regression evidence. |
  | Conditional unreproducibility task | No plan, false premise: original realistic full-payload outgoing race reproduced, so the conditional task remains unticked. No original task or AC wording changed. |
  | Body heading counts | Hyalo still has no general body editor; correct counts remain TasksA1/2, B-or-C1/1, AC2/2. Current Outcome/Carry-over is dated metadata, root's historical disclosure preserved. No raw body patch applied; supervisor owns final status and count disposition. |
  | Raw Firefox lifecycle | Owned rawFirefox39150 stopped/reaped143,6000free; unrelated desktop37270 preserved. /tmp/ff-rdp-253-repair1-raw.BklieZ retained as optional artifact, no removal requested/retried. |
  All original watcher/CMP/timing acceptance criteria remain unchanged. Supervisor's earlier17-pending-plan and canary reconciliation remains historical; supervisor owns the next exact-head independent review, current all-pending-plan reconciliation, final status/checkpoint and authorized merge. No source changes or broader backlog implementation were made for the carried failures.
supervisor_closure_first_head_2026_09_12: "2026-09-12 supervisor verified frozen implementation 41f4de9a69a637232e405ff77e09814605492c2b against swept source 95eedae226ee976937c55bda15a83fd35306d845: product, tests, README and dogfood are identical. All 334 distinct expected names equal 334 observed verdicts: 322 passed and 12 failed across five tiers, with all five profile checks clean. Ordered gates passed (2446 passed, 0 failed, 406 ignored), as did all nine actual xtask gates. Original AC/task wording is preserved. The conditional unreproducibility task remains unticked because the realistic race reproduced. Both actual ACs are fulfilled; status done describes implementation and closure, while the supervisor still owns exact-head PR review, CI and authorized merge. All 17 pending iterations were read and reconciled, including metadata-only plans 265/266/267; outside-range work remains pending. Next: 254, then prepare/verify/export the 256 Theme A harness before 255 measurement, then final 256 measurement and its single PR; the separate 242 sweep, 246 KB-only correction and 257 compatibility work follow. The architecture audit does not authorize broader implementation. Hyalo has no general body editor: correct heading counts are Tasks A 1/2, B-or-C 1/1, AC 2/2. Original body heading counters remain stale; the exact unapplied patch is retained. The user's Hyalo-only KB rule is preserved; no raw body patch was applied."
---

# Iteration 253: `--with-page` cannot tell a fast outgoing-document answer from the real destination

> **Renumbered 223 → 253 on 2026-09-06** so the pending queue runs as one contiguous sweep (DEC-051). Older PRs, commits and sweep logs cite it as iteration 223.

## Why

Carry-over from [[iteration-220-with-page-after-navigating-click]]'s closing sweep. iter-220
fixed the case where the outgoing document's collection eval is still in flight when Firefox
tears its docshell down — `set_target_guard` now catches that and retries.

It does **not** cover the narrower case iter-220's Outcome names as a residual, deliberately
left unfixed:

> When a navigation starts *and* the outgoing document answers the whole collection before its
> docshell is torn down, the view describes the outgoing page and nothing detects it.

Concretely: `click --ref <link>` fires `tabNavigated{state:"start"}`, and `collect_settled`'s
`settle_after_navigation` polls `getTarget` for up to 3s waiting for the `innerWindowId` (or
URL) to change. If Firefox answers the full collection eval against the **outgoing** docshell
before the settle loop's *next* poll observes the change — a small/cached outgoing page,
racing a `target-destroyed-form` that has not arrived yet — `collect_settled` returns a page
view of the page the action left, with no error and no signal that anything is wrong.

iter-220's own AC-2 evidence produced exactly this failure shape (`headings[0] == "Ada
Lovelace"`, the page the click left) when the fix was reverted — so the shape is real and
reachable; iter-220 fixed the reachable trajectory (Wikipedia: destination too slow for the
outgoing page to still be answerable) but not the adjacent one (outgoing page answers fast
enough to beat detection).

iter-220 judged the window "small" and chose not to file this at the time; filing it now
because the closing-sweep rule for this run requires every residual finding to get its own
plan rather than a comment.

## Themes

- **A — Reproduce it on purpose.** Build a fixture trajectory where the outgoing document is
  small enough to answer a full collection (headings + interactive scan + Readability run)
  before `target-destroyed-form` or the settle loop's own poll would catch it. If this cannot
  be reproduced against a realistic collection payload (the Readability pass alone is tens of
  KB of JS to inject and run — Theme A should measure whether that alone pushes every real
  collection past the observed 55ms teardown latency), say so with numbers and downgrade this
  iteration's scope to detection-only (Theme C) rather than a race fix.
- **B — If reproducible: close the window.** The cheapest fix is *not* "wait for every
  announced navigation to resolve before collecting" (iter-220 rejected that — it makes
  `--with-page` hostage to a redirect that never lands). Candidates worth costing out instead:
  re-check `getTarget`'s `innerWindowId`/`url` *after* collection completes, before returning,
  and retry if it still names the pre-navigation document; or have the collection eval itself
  report the `innerWindowId` it ran against (already visible to content-process JS) so the CLI
  can compare without a second round-trip.
- **C — If not affordably reproducible: detect and label it instead.** Failing that, at minimum
  make the returned page view distinguishable — e.g. compare the post-collection
  `innerWindowId`/`url` against what a navigation was announced for, and set
  `meta.page_ready = false` (or a new field) rather than reporting a confident wrong answer
  silently, matching how `page_ready` already reports the unrelated "collection wait timed
  out" case.

## Tasks

### A. Reproduce or bound the window [0/2]
- [x] Fixture route(s) that isolate the race (fast outgoing page, slow-to-commit destination
      with a `target-destroyed-form` delay) and a live test that fails on `main`
      (post-iter-220) if the race is real
- [ ] If unreproducible against a realistic collection payload, measure and record why
      (timing numbers), and note the theme this narrows the iteration to

### B or C. Close the window or label it [0/1]
- [x] Implement whichever of Theme B or Theme C the Task A finding points at, with a live test

## Acceptance Criteria [0/2]

- [x] Either: the reproduction test from Task A fails on `main` and passes after the fix — OR:
      Task A's write-up in Outcome explains, with measured numbers, why no such test exists,
      and Task B instead ships detection/labeling with its own live test
- [x] `cargo fmt && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace -q`
      clean; live sweep reconciles

## Out of scope

- Everything iter-220 already fixed (the in-flight-collection race, the navigation-latch, the
  target guard). This iteration is only the narrower "outgoing page answers before teardown is
  observed" gap iter-220's Outcome left as residual.

## References

- [[iteration-220-with-page-after-navigating-click]] — Outcome § "Residual, deliberately not
  fixed here"; PR #234's `## Carry-over` table
- `crates/ff-rdp-cli/src/commands/page_view.rs` — `collect_settled`, `settle_after_navigation`
- `crates/ff-rdp-core/src/transport.rs` — `set_target_guard`, `take_navigation_started`
