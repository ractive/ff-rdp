---
title: "Iteration 271: BBC consent action and live-site test contract"
date: 2026-09-14
type: iteration
status: in-progress
branch: iter-271/bbc-consent-no-cmp-recurrence
tags: [iteration, carry-over, consent, live-tests]
first_call_sites: []
dogfood_path: |
  FF_RDP_LIVE_TESTS=1 FF_RDP_LIVE_NETWORK_TESTS=1 cargo test -p ff-rdp-cli --test live live_144_session_hygiene_followup::live_144_bbc_cmp_dismissed -- --include-ignored --exact --nocapture --test-threads=1
  # Before choosing a fix, retain an attributable failed occurrence's navigate envelope,
  # current URL, document identity/readiness and real banner DOM on an owned browser.
  FF_RDP_LIVE_TESTS=1 FF_RDP_LIVE_NETWORK_TESTS=1 cargo run -p xtask -- live-sweep
depends_on:
  - "275"
---

# Iteration 271: BBC consent action and live-site test contract

Filed from the separately owed242 sweep at mergedmain
`19f4e236a70399c2984e6d46eb15bdac37a55181`. No271 implementation is authorized in the252–257
queue.

## Measured observations

`live_144_session_hygiene_followup::live_144_bbc_cmp_dismissed` failed after
navigation to `https://www.bbc.com/news` returned success, when `consent accept`
returned exit1, `error_type: consent_no_cmp`, `cmp: null`, `action: null`,
`status: no_cmp_detected`. The same exact dual-gate serial test failed again
in5.37s (command wall5.59s), with the same envelope.
Sweep Firefox58160/debug55257 is recorded in the launch log; both tests use
the direct route. Failed-occurrence navigate envelopes, current URL, document
identity/readiness and banner DOM were not retained. Successful navigation
alone therefore does not distinguish a delayed banner, a site/region variation,
a changed adapter contract or a different loaded document. No cause is claimed.

Original144's verified behavior was a real native BBC adapter click on
`#bbccookies-continue-button`, followed by proof that its control was gone.
Preserve that history; neither `--allow-no-cmp` nor accepting a null action
establishes dismissal. This failure is distinct from262's Sourcepoint
`detected_not_actioned` and target-promotion observations and from242's HN
empty-title mechanism, even though each touches a third-party page.

Evidence: `.git/ralph-loop/20260912-validation-efficiency/iter242-owed-sweep/sweep-failures.txt`,
`isolated-live_144_session_hygiene_followup__live_144_bbc_cmp_dismissed.log`,
`sweep-live-launches.log` and `isolation-results.jsonl`.

## Tasks [0/4]

- [ ] Reproduce with an exclusively owned browser and retain the failing navigate,
      URL/document/readiness and banner DOM evidence, including language/region,
      profile/consent state and the actual route; retain passing controls too.
- [ ] Identify whether detection, readiness or the real site's contract explains
      the error, without inferring a cause from test timing or an isolated pass.
- [ ] Implement only the demonstrated scoped correction, or record evidence that
      the current product behavior is correct and explicitly decide the real-site
      test contract; do not silently weaken actual-dismissal assertions.
- [ ] Add a meaningful bounded regression for the chosen contract, verify against
      real Firefox/site evidence, and run the required ordered gates and closing
      dual-gate sweep with every unrelated failure dispositioned.

## Acceptance Criteria [0/4]

- [ ] An attributable failing occurrence explains the BBC no-CMP result with actual
      page/readiness/banner evidence; the two original failures remain recorded.
- [ ] The chosen behavior distinguishes no banner from a failed dismissal and does
      not claim acceptance without an observed action and post-action evidence.
- [ ] A regression fails without the demonstrated correction and passes with it,
      or a no-product-change outcome is justified by measured site/test-contract
      evidence with the original requirement explicitly retained as unmet if needed.
- [ ] Real-Firefox verification and the required ordered gates/sweep are recorded;
      remaining failures have explicit owners, with no retry-only masking.

## Out of scope

Executing this plan in the252–257 takeover; changing unrelated consent adapters;
claiming this is262's Sourcepoint cause; changing the screenshot fix or weakening
BBC assertions simply to make the sweep green.

## Iteration257 additional recurrence, 2026-09-14

The screenshot repair's distinct344-name closing sweep failed the same BBC
test with `consent_no_cmp`, `cmp:null`, `action:null`, `no_cmp_detected` on the
direct route (Firefox54579/debug56165). Exact serial dual-gate isolation failed
again in4.77s (Firefox74544/debug49201) with the same envelope. All seven257
screenshots and its new guard passed; this is not a screenshot regression.
As before, failed-occurrence page/banner DOM and document identity were not
retained, so the original attributable-cause requirement remains unmet. No271
implementation, cause, test weakening or new consent behavior is claimed.
Guardian's concurrent daemon-route no-CMP result is separately preserved under262;
the shared error type does not establish a shared mechanism.
Evidence: `.git/ralph-loop/20260912-validation-efficiency/iter257-implementation/sweep.log`,
the exact isolated144 log and both launch logs. Original tasks/ACs stay untouched.

## Implementation preflight — 2026-09-19

BBC no-CMP recurred in both242/257 sweeps and exact isolation, but attributable failed-page/banner/readiness/region evidence remains missing. Capture it before choosing adapter changes. Keep BBC separate from262's Guardian/Sourcepoint observations unless evidence establishes a connection. The current live144 test also prints 'skipping' and returns success on navigation failure; assess this concrete test-honesty gap in regression scope without conflating it with the observed no-CMP result.

This source/evidence audit adds implementation guidance, not a new execution result.
Original task and acceptance-criterion wording and checkbox states remain unchanged.

## Bounded diagnosis and scoped test repair — 2026-09-19

Baseline `2e604688d226f4e77303468c158f0d059a59cb44`, Firefox 156.0,
direct route, a freshly launched owned profile, no auto-consent extension.
The owned browser on port61271 navigated successfully to the actual BBC News
document. A read-only pre-consent capture found the native
`#bbccookies-continue-button` at195.2×15.6 CSS pixels. `consent accept`
reported `cmp:bbc`, `action:accepted`; the subsequent capture found the
same control at0×0. The separate unchanged exact test passed in5.29s.
These are passing controls, not proof that the242/257 failures were fixed.
The extra diagnostic round trip before consent could affect readiness;
the exact test did not add that round trip.

The diagnostic captures retain the navigate envelope, `location.href`,
`document.URL`, title, readiness, banner HTML/rectangles, cookie/storage state
and profile identity. Browser language was `en-US` (`en-US,en`), timezone
`Europe/Zurich`; those do not establish the site's geolocation decision,
which remains unverified. The native control was followed by a Sourcepoint
acceptance in the same already-used profile, then a third consent call
returned `consent_no_cmp` after dismissal. That is a post-consent control,
not a reproduction of the fresh-profile failure or evidence of262's cause.

No consent-adapter change is justified by these observations. The existing
real-site test contract remains strict: native BBC match, accepted action,
and an absent or zero-size native control afterward. No `--allow-no-cmp`,
null-action acceptance or readiness delay was added.

One independent test-honesty defect was demonstrated and repaired. Replacing
only the BBC navigation URL temporarily with `http://127.0.0.1:1/` made
Firefox report `deniedPortAccess`. The old test printed “skipping” and passed
in2.51s without testing consent. With navigation failure asserted, the same
mutation failed in2.59s and recorded the actual `about:neterror` document.
The mutation is removed; with the BBC URL restored the test passed in4.37s.
The assertion now includes the navigate envelope. On navigation or consent
failure, a subsequent read-only page-context capture preserves URL/document,
readiness, language/timezone, cookies and native/banner DOM. It is explicitly
a later observation, not an atomic snapshot of the failing command, and runs
only on failure so it cannot delay the successful pre-consent path.

Evidence root: `.git/ralph-loop/20260919-queue/iter271/` in the primary
checkout. Each command log has a `.meta` sidecar with command/environment,
start/end and exit status. Diagnostic logs: `launch.log`, `before.log`,
`navigate.log`, `after-navigate.log`, `consent.log`, `after-consent.log`,
`second-consent.log`, `third-consent.log`, `after-third.log`.
Regression evidence: `exact-baseline.log`, `navigation-failure-mutation.patch`,
`navigation-failure-mutation.log`, `navigation-failure-fixed.log`,
`exact-final.log`. Original242/257 evidence above remains authoritative.

The original attributable failing-occurrence requirement remains unmet:
no fresh-profile BBC no-CMP occurrence was reproduced in the bounded initial
diagnosis, and historical failures have no page snapshots. This iteration
must not be marked done or described as a BBC detection fix on that evidence.

## Initial closing validation and carry-over — 2026-09-19

Initial-source dual-gate sweep (Firefox156.0, owned raw Firefox49282 on6000,
readiness verified before the sweep; desktop1112 preserved):

```text
LIVE_SWEEP_SUMMARY executed=346 skipped=0 preexisting=0 vanished=0 launch_timeout=0 timed_out=0 total=346
LIVE_SWEEP_PROFILES leaked=0 unattributed=0 root=/Users/james/Library/Application Support/ff-rdp/profiles
```

All346 compiled ignored-test names have actual verdicts: CLI332 passed/3 failed;
core tiers1+3+3+2+2 passed, including the newer `live_watcher_protocol` target.
No missing, duplicate or unexpected names. BBC passed. Sweep exit1 is retained;
no whole-sweep rerun or unrelated isolation was used to erase its failures.
The344-name totals from older iterations are not reused for this baseline.

All nine enumerated xtask checks passed. `check-dogfood-script` explicitly
skipped execution because this plan has no script; the exact live BBC test
and sweep provide its real-site verification. `rustup update stable`,
`cargo fmt`, strict workspace/all-target clippy, and `cargo test --workspace -q`
passed in that order. Logs: `xtask-gates.log`, per-gate logs,
`ordered-gates.log`, `rustup-update.log`, `fmt.log`, `clippy.log`,
`workspace-tests.log`, `reconciliation.log` and their `.meta` records.

| Observation or unmet requirement | Disposition |
|---|---|
| Original242/257 fresh-profile BBC no-CMP failures lack attributable page evidence; no fresh-profile recurrence here | Retain in271. Original cause AC remains unticked, status in-progress. Passing controls and test-honesty repair do not discharge it. |
| Old live144 passed after failed navigation without attempting dismissal | Closed by this branch's assertion; identical blocked-port mutation passes before and fails after. BBC action/post-action assertions remain intact. |
| Sweep `live_137_daemon_mode_parity::live_137_consent_accept_via_daemon`:15079ms/47 polls, targets1/live0 | Fold into [[iteration-262-daemon-live-target-never-promoted]], dated target-lifecycle observation; no262 investigation here. |
| Sweep `live_140_element_targeting::live_140_frame_filter_count_accurate`:0 available frames instead of5 filtered candidates | Fold separately into262's zero-frame observations; no shared cause asserted. |
| Sweep `live_166_navigate_document_status::live_166_navigate_status_direct_parity`:HTTP200, committed `about:blank` | File [[iteration-277-direct-navigate-committed-about-blank]], diagnostic-only carry-over; not executed. |
| Site geolocation decision unverified; initial and historical failed-page evidence unavailable | Retain in271's attribution requirement. Browser language/timezone are not treated as geographic proof. |

Original tasks and AC wording are unchanged. Their boxes remain unticked for
supervisor reconciliation; the required original-cause evidence is unavailable.
The frozen change is a test-honesty repair and diagnostic improvement, not a
product consent correction or a completed271 claim.

## Repair R271-1 — bounded failure diagnostics, 2026-09-19

The independent review found that `bbc_failure_context` could emit every
matching node's complete `outerHTML` and the complete cookie string. The
failure-only diagnostic now caps native/banner matches at12, records bounded
identifying attributes, rectangles and text, and reports cookie presence,
names, count and raw length without returning cookie values. The final
diagnostic note is UTF-8 safely capped at16KiB with an explicit truncation
marker. BBC navigation, native-CMP matching, `action:accepted` and the
post-dismissal zero-size assertion are unchanged.

The initial repair's string-match assertions did not execute JavaScript and
were insufficient proof of cookie-value omission. Its preliminary logs remain
under `iter271/repair1-*.log`; those checks are not the final verification.
The final Firefox-free test
`bbc_failure_context_preserves_utf8_and_caps_bytes` checks unchanged short
input, a multibyte oversized payload, the16KiB limit, preserved content and
the explicit marker. It is explicitly annotated as Firefox-free for the live
test-layout checker.

Actual JavaScript behavior was measured on an owned Firefox156.0 with a local
HTTP fixture, using the exact `BBC_FAILURE_CONTEXT_JS` extracted from source.
The fixture created36 visible nodes with5000-character text and1000-character
attributes, including16 native-selector matches, and20 actual document cookies
with `COOKIE_VALUE_SENTINEL_` values. Setup verified those values were present
in the cookie store. The diagnostic returned12 native entries/4 omitted,
12 banner entries/24 omitted,12 cookie names/8 omitted, and capped text and
attributes with truncation markers. Assertions over the executed JSON passed;
no cookie-value sentinel was present anywhere in the diagnostic output.
The fixture is an ad-hoc verification artifact, not a new permanent live test.

Evidence: primary checkout `.git/ralph-loop/20260919-queue/iter271/repair1/`:
`fixture.rs`, `setup.js`, `exact-diagnostic.js`, `fixture-setup.log`,
`fixture-diagnostic.log`, `fixture-assertions.log`, `cap-test.log` and
command `.meta` sidecars. This repair addresses only R271-1. The original BBC
cause remains unexplained; all original tasks/ACs stay unticked and status
remains `in-progress`.

### Final repair sweep

R271-1 changed source, so its own dual-gate closing sweep was required; it was
not a retry of the unchanged initial source. On the frozen repair source it
reported:

```text
LIVE_SWEEP_SUMMARY executed=346 skipped=0 preexisting=0 vanished=0 launch_timeout=0 timed_out=0 total=346
LIVE_SWEEP_PROFILES leaked=0 unattributed=0 root=/Users/james/Library/Application Support/ff-rdp/profiles
```

CLI334 passed/1 failed and all11 core tests passed. The failure was again
`live_137_daemon_mode_parity::live_137_consent_accept_via_daemon`:
15104ms/47 polls, daemon78161/proxy56135/debug56031, targets1/live0,
dispatcher alive with89 frames started/finished and none in flight. Folded
as another target-lifecycle observation into262, without a cause claim.
BBC,140 and166 passed; none of those passes discharges the historical BBC
cause requirement, the initial140 zero-frame failure, or plan277's initial166
committed-URL failure. Both sweep records and every original disposition remain.

The owned raw Firefox75058 was verified listening on6000 before the sweep,
survived all core tiers, and was stopped afterward; its profile was removed.
The local fixture server75107 was also stopped; desktop1112 was preserved.
All repair execution logs and metadata are in `iter271/repair1/`, separately
from the initial sweep and incomplete preliminary repair logs.

All346 compiled ignored names reconcile against actual verdicts across all
six targets, with0 missing, unexpected or duplicate names. All nine xtask
checks passed, including the live-layout exemption for the Firefox-free cap
test; dogfood-script execution remains an explicit no-script skip. Stable
update, fmt, strict workspace/all-target clippy and workspace tests passed
in order on the frozen repair source. Evidence: `repair1/reconciliation.log`,
`xtask-gates.log`, `ordered-gates.log` and their per-command logs/metadata.
Fresh scoped independent review of R271-1 remains required before checkpointing.
## Restart plan — 2026-09-19

Follow [[ralph-loop-open-iterations-2026-09-19]]. Resume the existing partial-code
branch at `0e9f978b67f6de74bbd276e04dec4704b5554a61`, currently checked out in
`/Users/james/.cache/ff-rdp/queue-20260919-271`. Integrate current verified main
there; preserve its BBC navigation assertion, bounded failure capture and
branch-only plan277, plus both previous sweep records. Main does not yet contain
this test repair. The original four tasks/four ACs remain unticked.

**Reuse already-reviewed work.** The old test passed after a navigation failure;
the same blocked-port mutation failed after the repair. The independent review's
unbounded-DOM/cookie finding was repaired and freshly reviewed with explicit zero
remaining findings. The exact diagnostic JavaScript was exercised against an
oversized real-Firefox fixture; final output is16KiB-bounded and omits cookie
values. Preserve those contracts and reuse unchanged evidence. Do not repeat the
initial full review or fixture measurements solely for another model's opinion.

**Attribution experiment (Sol for capture; Astra for ambiguous diagnosis).**

1. Inspect integration changes and verify the retained diagnostic code still runs
   only after failure, so a successful test gains no extra pre-consent delay.
   Preserve the exact native-BBC match, real accepted action and post-action
   control absence/zero-size assertions. No `--allow-no-cmp` or null-action pass.
2. Use fresh exclusively owned profiles on the existing network route and record
   installed Firefox identity, profile history, locale/timezone, navigation
   envelope and timing, actual URL/document/title/readiness, observed banner/CMP
   markers and bounded cookie-name/presence metadata. Locale/timezone are not proof
   of the site's geolocation decision. Do not use paid proxies, new accounts or
   assumed regional emulation. Post-consent no-CMP is a control, not reproduction.
3. Run up to three unchanged exact BBC tests serially with failure capture. If
   all pass, one existing contention configuration may be tried, with at most
   three additional fresh-profile BBC attempts. A run must exercise dismissal;
   navigation failure cannot be counted as a BBC no-CMP occurrence. These are
   bounded discovery attempts, not new repeated-measurement ACs or a green streak.
4. On a qualifying failure, preserve immediately available command/page evidence
   before another navigation or consent action. Mark later snapshots as later
   observations, not atomic failure state. Use targeted read-only inspection to
   distinguish wrong document, delayed banner, legitimately absent/already-consented
   banner, regional/site contract, or adapter mismatch. Test only the demonstrated
   distinction. A local regression fixture must come from recorded real Firefox
   evidence; no invented e2e payloads.
5. Implement the demonstrated scoped correction, or document a supported site/test
   contract decision without silently changing original acceptance. Keep the old
   failing observations and their missing data visible. Review only new behavioral
   changes/affected contracts against preserved initial review coverage, then run
   current-source gates and this iteration's own dual-gate closing sweep.

If all bounded fresh-profile attempts pass, retain the test-honesty improvement
on its recoverable branch and report the missing failing-occurrence evidence.
There is no authorization here to merge that partial repair as completed271 or to
rewrite its causal AC. Broader test-contract changes need an explicit, reviewed
scope decision. Plan277 remains filed and unexecuted; a later pass does not close it.

## Restart attribution capture — 2026-09-19

The prepared branch was integrated with verified main/planning while preserving
the reviewed test-honesty repair unchanged (source SHA-256
`f42f2f95bd172574d6c01132b6f2d1231ae24a65251a6e57d1d09faedc422fbe`).
Its diagnostic remains failure-only; successful runs add no pre-consent probe.
The exact assertions still require `cmp:bbc`, `action:accepted`, and native-button
absence or zero size after action.

The worker reported Firefox156.0 (`CFBundleVersion 15626.9.9`,
BuildID20260909172920), but the version command/output was not retained as a
contemporaneous receipt. The contemporaneous launch ledgers and test logs retain
the existing direct network route, fresh test-owned launch policy, no auto-consent
extension, named launch PIDs/ports and the six test verdicts. They do not retain
per-profile paths or a profile-removal receipt. Three serial unchanged exact BBC
attempts used fresh test-owned profiles and passed after exercising dismissal in4.88s,4.36s
and4.70s. The single permitted contention configuration paired each subsequent
serial BBC attempt with five existing local Firefox tests, matching the measured
six-worker setting. All three BBC attempts and all companions passed; BBC times
were9.32s,7.77s and7.29s. No contemporaneous per-profile teardown receipt was
retained, and the worker's cleanup command outputs are unavailable.

The supervisor later captured current process state at 2026-09-19T21:59:18Z in
`../supervisor-current-processes.log`: no test workload, desktop Firefox1112
present and no listener on port6000. This is a later current-state observation,
not reconstructed capture/teardown evidence. The supervisor later captured the
currently installed browser version at 2026-09-19T22:02:01Z in
`../supervisor-current-firefox.log`, with the same reported identity. This is a
later installed-version observation, not contemporaneous capture identity.

The tests use the direct route, fresh per-launch profiles and no auto-consent
extension. Host timezone was Europe/Zurich. Failure-only diagnostics would have
retained actual URL/document/title/readiness, locale/timezone, bounded banner/CMP
markers and cookie names/presence, but no qualifying failure occurred, so those
page fields were deliberately not collected on successful paths. Locale/timezone
would not establish the site's geolocation decision. No proxy, account or region
emulation was used.

Evidence: `.git/ralph-loop/20260919-resume-2349/iter271/`, including six BBC
logs and metadata, launch ledgers, companion logs and the frozen phase report.
The bounded experiment therefore supplies no attributable failing occurrence
and justifies no consent-adapter or broader contract change. The historical
242/257 failures and their missing page evidence remain recorded. All original
tasks and acceptance criteria remain unticked (0/4); this passing capture is not
partial completion, a retry-based green claim or authorization to merge271.

## Additional investigation block — 2026-09-20

The owner authorized one new 60-minute active investigation block with at most six targeted captures. The previous six passing controls and checkpoint `f044e6791683126b9d45e4444982c918d6648e88` remain preserved. The new proposal tested a concrete site-contract distinction: BBC native/Sourcepoint layer order and readiness, with one immediate and one delayed fresh-profile arm. It did not repeat the old unchanged block.

Two instrumentation repair batches preserved the original native JavaScript and strict native BBC acceptance/post-action absence assertions. The independently reviewed fallback records same-command branch choices and later bounded site-state observations; decision-time selector/geometry state is explicitly unavailable, so later snapshots cannot establish a detection bug. Final independent review approved the exact frozen instrumentation with zero findings. Focused Rust, actual page-JavaScript and actual runner failure-path checks passed.

No BBC capture ran (0/6). Preparation reached the proposal's stated 50-minute cutoff for starting new captures; that stopping condition was preserved rather than moved. Active accounting before closeout is 3230/3600 seconds, including the final measured 187-second review; the review exceeded its requested three-minute phase bound by seven seconds, explicitly reported. This is not a claim that all 60 minutes or six slots were used. The exact diagnostic patch, binaries and runner are archived under `.git/ralph-loop/20260920-additional/iter271/final-instrumentation-archive/`; final source diff SHA256 is `4b38d82204029c2250ceca04150ca264c94eedf209a911e63c5c8786c39fd30e`, with approval in `repair2-review.md`. Source was restored to `f545497184087d0793ae7c0c755e4c785a77e37f`, verified against its passing source manifest, and restored CLI/tests rebuilt successfully.

All original tasks and AC0/4 remain unmet: there is still no attributable new failure or measured site-contract outcome. The reviewed partial test repair is not completed271 and is not mergeable as such. Next entry must preserve this prepared tooling, verify exact source/binary/environment identities, and explicitly reconcile a new capture schedule with the recorded stopping condition and remaining allowance. No fresh-session reset, unchanged six-attempt repetition, weakened dismissal assertion or automatic background continuation is authorized.

## Foundation repair checkpoint — 2026-09-20

The owner adopted two additional repair batches and3600 active preparation seconds, and narrowly reopened only the previously reviewed immediate/delayed site-contract pair, at most two captures within370 retained discovery seconds. Historical3230/3600 discovery use, the exhausted six earlier passing BBC controls and all original failures remain unchanged. This run did not repeat those controls.

Verified foundation8666a5324905793538c59532e909e0808beee7f3 was integrated above preserved301f2a9e/f044e679 history. Both feedback-note conflict sides were retained. The exact combined baseline passed ordered stable update, fmt, strict workspace/all-target Clippy and workspace tests:2500passed/0failed/418ignored. This validates the combined baseline, not the temporary diagnostics or a closing live sweep.

Both newly authorized repair batches are now consumed. The first adapted the retained reviewed instrumentation and froze the pair, with diagnostic build/Clippy,23 consent-unit tests, focused compiled checks, actual diagnostic JavaScript and shared runner controls. Fresh review rejected four fatal-path defects: unbounded synchronous helpers, evidence deletion before a fatal desktop check, delayed exceptions leaving pair exit0, and completed navigation/passive observations lost on timeout. The final batch repaired immediate progress flushing and retained all private homes through evidence review; it added real phase-alarm/FIFO, identity, delayed-error and final-write controls. Build/Clippy, six compiled offline tests, actual JavaScript, five existing controls and11 new controls passed. These are offline preparation results, not a Firefox capture.

Final fresh review rejected the exact frozen pair for three remaining defects. A caught one-shot phase alarm can leave later blocking I/O without an armed timer; opening the next file occurs before the deadline check. A command.json write exception can bypass browser cleanup, and killing the test group does not cover Firefox's separate process group. A timeout after success JSON emission can leave pair-end exit0 while the process returns70, because its fallback shares the expired deadline. The existing controls did not cover these exact paths. All three findings and both reviewed freezes remain archived; they do not authorize a third repair.

No capture was launched, no slot was spent and no new BBC site-contract evidence exists. All370 discovery seconds and six numerical historical slots remain unused, with the narrow pair still capped at two. The final runner is not approved. An outer supervisor launch wrapper also requires an exact approval record that was never created. Do not execute these archived inputs or restart the old shell runner on a fresh-session assumption. New work requires authority consistent with the exhausted repair ceiling, corrected fatal paths and fresh review before the bounded pair.

Strict native acceptance JavaScript, cmp:bbc/action:accepted and actual native post-action absence/zero-size assertions remain unchanged. Sourcepoint acceptance and no-CMP cannot satisfy them. Decision-time selector/geometry/timeOrigin remain explicitly unavailable; later samples cannot establish atomic detection failure or explain the original failures. Original tasks and AC0/4 remain untouched. The reviewed partial test repair is not a completed271 implementation and was not merged to main. No product behavior was changed to force a passing outcome; no259 investigation occurred.

The supervisor used the reviewed guarded restoration to restore exactly three diagnostic files. All567 combined-baseline source/config hashes match; restored CLI/live/unit artifacts rebuilt successfully in0.352s, exit0. Matching ordered baseline gates are reused only for the restored documentation checkpoint. The pending foundation integration, original partial repair and all evidence are preserved. New preparation charge2589/3600 includes conservative supervisor handoffs and both reviews; compilation was included in worker active time. Both new repair batches are exhausted despite numerical time remaining. Actual tokens are unavailable.

Evidence is under the primary checkout's .git/ralph-loop/20260920-foundation-repair/iter271/: repair1/, review1.md, repair2/, review2.md and blocked-checkpoint/. Final rejected freeze36c788677928fc2e7cf24ba471176103cd29558730e2593ae58491e177ec5334 pins695 inputs including110 Firefox package files. The reviewer verified every pin and the restoration baseline. Desktop Firefox57827 remained preserved; the earlier disappearance of1112 has no established cause. No full sweep or completion PR was run.

## Prospective scope adaptation — 2026-09-25

This iteration completes the reviewed navigation-failure honesty repair and
defines the supported native-BBC versus current remote-site consent contract.
Native dismissal remains verified by an actual action and post-action effect.
A remote page with no native BBC banner or an earlier Sourcepoint layer is a
different observed state, not proof of native dismissal.

Original historical no-CMP failures remain recorded and unattributed; original
AC1 remains unfulfilled without their missing page evidence. A newly observed
site state may justify a prospective test-contract change, independently
reviewed before implementation, but does not reconstruct those failures.

### Delivery acceptance [0/3]

- [ ] Navigation failure is a real failure, with the reviewed bounded
      failure-only diagnostics; the blocked-navigation regression remains
      sensitive and successful execution gains no diagnostic delay.
- [ ] Native BBC dismissal has real Firefox action/effect proof. Current
      remote-site outcomes are explicitly classified; no-banner, another CMP
      and failed dismissal cannot count as native accepted success.
- [ ] Any changed site/test behavior has meaningful contract coverage and
      independent review, followed by ordered gates and this iteration's own
      dual-gate closing sweep.

The owner’s September25 all-open adaptation request supplies the prospective
delivery scope above. Completion under this scope must say so explicitly and
retain every unfulfilled historical AC; it must not report the original
attribution criterion as passed. Earlier investigation schedules and restrictions
remain historical records, not renewable capture allowances. Use only the
finite controls needed for the new contract and the required closing validation.

The archived immediate/delayed pair runner remains rejected, including its
later process70/receipt0 terminal-publication mismatch in
`.git/ralph-loop/20260920-another-spin/iter271/`. Do not execute that runner or
reuse its consumed repair claim. If a native comparison is needed for the new
contract, use current qualified owned-child execution with actual waits.
