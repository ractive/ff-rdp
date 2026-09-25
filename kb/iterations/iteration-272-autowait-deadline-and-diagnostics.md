---
title: "Iteration 272: Bound auto-wait while preserving timeout diagnostics"
type: iteration
date: 2026-09-19
status: done
branch: iter-272/deadline-final-20260925
depends_on: [258]
first_call_sites: []
dogfood_path: |
  On an owned Firefox, compare click/type readiness timeouts on a blocked page
  with --wait-timeout smaller than --timeout. Retain the error's selector,
  match count and hidden/unstable classification alongside wall time.
  Exercise a responsive absent selector and a moving element too.
tags: [iteration, carry-over, latency]
---

# Iteration 272: Auto-wait needs a deadline-aware diagnostic contract

## Evidence

Iteration 258's source survey found `commands/js_helpers.rs::autowait_element`
uses the socket timeout inside readiness, settle installation, idle detection,
stability and selector diagnostics. The diagnostic is deliberately evaluated
after expiry to distinguish missing, hidden and unstable elements. Wrapping
each probe with the new read deadline unchanged would suppress that diagnostic
at precisely the point it is required. The same function has a separate 500 ms
stability allowance and an overall readiness budget. This requires an explicit
diagnostic policy and multi-stage regression coverage, beyond applying the
unchanged navigation-poll mechanism. This is source evidence, not a new live
failure measurement.

## Tasks [3/3]

- [x] Capture blocked-page and responsive selector baselines on an owned browser.
- [x] Bound all auto-wait stages with an absolute budget; retain useful diagnostics
      from observed evidence and define fallback wording when no probe answered.
- [x] Cover blocked reads, push traffic, late replies and each diagnostic category
      without weakening the iteration 237 idle-page short-circuit.

## Acceptance Criteria [3/3]

- [x] A scripted blocked console cannot extend auto-wait to the larger socket timeout.
- [x] Missing, hidden and unstable selectors remain distinguishable when evidence
      is available; unavailable evidence is reported honestly.
- [x] Ordered quality gates and a dual-gate live sweep pass, with measured owned-browser
      timing and preserved navigation/auto-wait behavior.

## Out of scope

Changing the global timeout, unrelated polling loops, or skipping diagnostics silently.

## Execution clarification — 2026-09-23

All five auto-wait evaluation paths share one absolute readiness budget;
stability retains its 500 ms sub-budget inside that deadline. Diagnostics derive
from validated observations collected before expiry and explicitly report
unavailable or last-observed evidence. No diagnostic read starts after expiry.
Preserve iteration 237's idle signal and observation floor, and iteration 160's
actual exception reasons. Caller setup, match resolution and click's cross-frame
salvage are separate measured phases, not evidence that auto-wait spent its own
budget. This source preflight does not establish live results; original tasks and
acceptance criteria remain unchanged.

The first fixture qualification also corrected the dogfood premise: click/type
currently expose `--timeout`, not a separate `--wait-timeout`. Their internal
options accept a distinct readiness budget, which the scripted-console tests can
exercise against a larger socket timeout. Live CLI measurements use the actual
`--timeout` and separately exercise the existing 500 ms stability sub-budget.
No new CLI flag is required. The original dogfood text is retained as historical
planning context, not a claim that the unsupported flag ran.

## Implementation and focused evidence — 2026-09-23

Auto-wait now scopes readiness, selector diagnosis, settle installation, idle
checks and rect stability through iteration 258's existing absolute read deadline.
The stability phase, including its diagnostic, retains its 500 ms sub-budget;
all sleeps are clamped to the remaining budget. No final probe starts after
expiry. Successful diagnostics collected inside the budget are retained and
labeled as the last observation when a later read fails. A blocked first probe
reports unavailable evidence, not a guessed missing/hidden/unstable cause.
Malformed diagnostic fields cannot fabricate a zero match count. Actual changing
rects report instability; a blocked rect read only reports unconfirmed stability.

The implementation deliberately retains the existing readiness/stability wire
reply shapes. It adds a bounded selector diagnostic after each readiness reply,
including the successful readiness reply before stability. This costs one extra
round-trip per readiness poll but avoids any unbounded post-timeout diagnostic.
The ready path still installs no settle instrumentation. The iteration 237 idle
predicate and its observation floor, iteration 160 exception reasons, and
iteration 258 reply correlation and timeout restoration remain intact. Click's
existing subsequent frame search is unchanged and remains outside auto-wait.

A finite local fixture blocks exactly one probe for 2500 ms. Twelve CLI cases
(absent, hidden single/multiple, moving, blocked readiness and blocked stability,
on both direct/daemon routes) were measured before and after, on owned Firefox
156.0 with `--timeout 4000`. The blocked-readiness case still succeeds inside the
overall budget. The blocked-stability case improved as follows:

| Route | Base wall time | Changed wall time | Result |
|---|---:|---:|---|
| Direct | 2.57 s | 0.51 s | Timeout; no typing action |
| Daemon | 2.74 s | 0.67 s | Timeout; no typing action |

The blocked second rect call is now encountered during the bounded diagnostic
inside the stability phase. Its error reports that stage and the earlier
readiness observation; it does not invent motion or a match count. The separate
moving case retains the actual instability diagnosis. Missing and hidden cases
retain their match counts and recovery advice. Full timings, envelopes, commands,
source/binary identities and ownership records are private under the primary
checkout's `.git/ralph-loop/20260923-remaining272-278/iter272/`.

Four new non-live tests cover every blocked stage, zero-budget/no-send behavior,
finite timeout restoration, missing/hidden/malformed/moving diagnostics, continuous
push traffic and late ACK/result correlation. All 24 helper tests passed. Extending
the auto-wait deadline and removing cached diagnostic evidence each made the
intended regression assertion fail; both mutations were restored byte-for-byte.
The new consolidated live test passed its twelve direct/daemon cases on the final
product source, and all eight selected existing 237/140/160 live tests passed.

Stable was updated (Rust 1.98.1, Clippy 0.1.98). Ordered fmt, strict workspace
all-target Clippy and normal parallel workspace tests passed: 2533 passed,
0 failed, 420 ignored. The ignored count includes the new live test; it is not
claimed as executed Firefox coverage. Closing sweep and enumerated gate results
are recorded separately below once complete.

## Closing sweep and remaining acceptance — 2026-09-23

One final-source dual-gate sweep executed all 348 compiled ignored names, exactly
once each across six tiers. Name reconciliation found no missing, unexpected or
duplicate verdicts. CLI: 336 pass / 1 fail; core tiers: 1+3+3+2+2 pass. The runner
exited 1. Its actual summaries are:

```text
FF_RDP_LIVE_TESTS=1 FF_RDP_LIVE_NETWORK_TESTS=1 cargo run -p xtask -- live-sweep
LIVE_SWEEP_SUMMARY executed=348 skipped=0 preexisting=0 vanished=0 launch_timeout=0 timed_out=0 total=348
LIVE_SWEEP_PROFILES leaked=0 unattributed=0 root=/Users/james/Library/Application Support/ff-rdp/profiles
```

`live_navigate_default_fast::live_navigate_elapsed_matches_wall` failed with
wall1669ms, reported860ms, delta809ms against its unchanged750ms assertion;
load averages were150.59/109.97/59.86. One bounded isolated control passed with
wall545ms, reported291ms, delta254ms at55.07/93.08/59.23. This matches the historical
load-shaped signature recorded in [[iteration-246-sweep-load-misclassification]]:
external wall time includes work outside the internal measurement. It does not
establish a timing regression, and an isolated pass does not satisfy the original
clean-sweep criterion. No wait/assertion was widened and no second unchanged
sweep was used to overwrite this occurrence. Original AC3 remains unticked.

The new272 test passed in the sweep, as did every selected compatibility test.
All per-target profile scans were clean. Owned raw Firefox29235 was actually
waited after SIGTERM (status143); its unmanaged profile was removed and port6000
was free. Desktop Firefox14298 retained its original birth. Product hashes were
unchanged throughout final gates, focused live tests and the sweep.

## Historical carry-over — September23

| Observation | Disposition |
|---|---|
| Auto-wait stages and final diagnostics could exceed their readiness/stability budgets. | Closed in this candidate by deadline scopes, cached evidence, scripted mutations and owned-browser proof. |
| Initial baseline setup attempts rejected a data URL, encountered an initial zero-tab snapshot, hit Bash3 empty-array handling, or used the plan's unsupported click/type `--wait-timeout`. | Closed in the measurement harness and dated clarification; all rejected attempts retained. Baseline5 is the first admitted12-case series. No claim that wrapper exit0 made usage-error cases pass. |
| First Clippy pass and a later daemon-parity source guard failed. | Closed here with mechanical option-reference/semicolon fixes and the truthful both-route annotation. Final ordered gates passed2533/0/420. |
| Navigation wall-versus-reported delta809ms in this sweep; isolated control254ms. | Filed [[iteration-279-navigation-timing-regions-and-load]], planning only outside272–278. Historical246/264 context retained; cause unproven, unchanged assertions remain binding. |
| Contended launch test timed out in the second closing sweep after 300 seconds of silence. | Fold into existing [[iteration-273-contended-launch-output-hang]] as a new occurrence, with exact watchdog stacks and process/descriptor snapshots retained. The matching test name is not proof of the same cause; no273 implementation ran here. |
| Original clean dual-gate sweep acceptance. | Unmet, AC2/3. Neither sweep qualified; a passing isolated control cannot replace the first red sweep. No completion/PR/merge claim. |

The full private failure ledger also retains both expected mutation failures and
minor discovery/bookkeeping command errors. Actual token usage is unavailable.

All nine enumerated xtask checks passed after the sweep, including the full281-plan
inventory (0failed,93warnings-only) and actor/KB sync against the verified base
`c761c1b1f62b3cd9f6bcf56301e6528b1e70fad2`. The Firefox-reference check had no
`firefox_refs` key; the dogfood-script check explicitly skipped because no
`dogfood_script` was declared. Neither is counted as live evidence. HYALO005
checked475documents with0errors/0warnings. No source changed after the final
ordered gates. The candidate remains uncommitted, with original tasks3/3 and
AC2/3, awaiting the supervisor's independent review and closing disposition.

## Second closing attempt — 2026-09-23

After an independent initial review returned zero findings, the supervisor
authorized one further finite closing attempt on unchanged reviewed source.
The same dual gates, default six jobs, default 300-second quiet watchdog, all348
names and unchanged timing assertions applied. No load threshold was used for
admission: start load8.56/27.51/38.39, finish5.72/46.02/58.06.

There were347 actual passing verdicts (CLI336, core1+3+3+2+2) and one named
timeout: `live_158_launch_lifecycle::live_158_launch_survives_contended_bind`.
The watchdog captured stacks, killed the CLI tier and reaped four orphaned test
Firefox processes. A slow-test notice also named240, but240 subsequently reported
a passing verdict and is not classified as timed out. The runner exited1:

```text
LIVE_SWEEP_SUMMARY executed=347 skipped=0 preexisting=0 vanished=0 launch_timeout=0 timed_out=1 total=348
LIVE_SWEEP_PROFILES leaked=0 unattributed=0 root=/Users/james/Library/Application Support/ff-rdp/profiles
```

The navigation timing assertion and new272 test both passed in this attempt;
this does not erase the first809ms occurrence. That timing investigation is filed
as279, planning only outside the272–278 execution queue. The contended-launch
occurrence belongs to existing273; its cause remains unproven. The exact logs,
watchdog stacks and a finite process/descriptor snapshot are retained in private
`iter272/closing2/` evidence. No third sweep or273/279 execution followed.

Every per-tier profile scan reported zero leaked/unattributed profiles. Owned
raw Firefox53017 was actually joined after SIGTERM (status143); its profile was
removed, port6000 was free and desktop Firefox14298 retained its September22
birth. All three reviewed source hashes remained unchanged. Original tasks3/3,
AC2/3 and in-progress status remain honest; the required clean sweep is still
unmet.

Actual-verdict reconciliation intentionally retained its exit1 for the absent
watchdog verdict. Separate classification reconciliation then matched347passing
verdicts plus the one explicitly named timeout to all348compiled names, with no
missing, unexpected or duplicate classifications. All nine enumerated xtask gates
passed again; plan272, new279 and the full282-plan inventory validated with0failed
and93warnings-only. HYALO005 checked476documents with0errors/0warnings.
The no-reference/no-dogfood-script skips remain distinct from live evidence.
Final ordered workspace validation receipts are retained alongside the closing2
report; no passing workspace gate substitutes for the unmet clean-sweep AC.

## Current-base recovery — 2026-09-24

The owner authorized the all-open iteration run with a new grant; earlier stopped
capture and repair allowances remain historical expenditure, not reset evidence.
This candidate recovers exactly the three previously reviewed source files onto
merged main `9aff4a66116783aae7bcf4a63d5a7187763f8ed9`, on
`iter-272/autowait-deadline-20260924`. The original helper and live-registration
inputs were identical between the old base and this base, so no source adaptation
was required. The prior review and two sensitive mutations remain applicable to
those unchanged inputs. New non-live validation and current-runtime native
qualification are separate evidence; the old Firefox156.0 measurements are not
represented as Firefox156.0.1 measurements.

The current base retains262 startup/reply ownership and282 launch-child cleanup,
output ownership, attempt accounting, inherited-descriptor exclusion and genuine
owned Firefox fallback contracts. Those changes justify fresh finite qualification
and a new closing sweep on changed inputs. They do not explain the old272/273
hangs. Both historical closing attempts above remain failed.273 is now completed;
its old carry-over row records the historical disposition, not a new pending task.
279 remains open with the original809ms occurrence and unchanged assertion.

The current qualification proposal is one execution of the new twelve-case
both-route live test and one execution of each of the eight original237/140/160
compatibility tests. It requires current Firefox package verification, private
run homes, exact launch-attempt accounting, before/after ownership records and
fixture read-back. No unchanged baseline recreation, widened wait, assertion,
retry, or full-sweep discovery is proposed. Original tasks3/3 and AC2/3 remain;
AC3 requires this candidate's actual successful closing sweep and final gates.

Current-base focused validation passed24/24 helper tests. After stable update
(Rust1.98.1, Clippy0.1.98), ordered fmt, strict workspace all-target Clippy and
normal parallel workspace tests passed2555/0/421. The nested282 isolated child
emitted one additional1/0/0 summary; it is excluded from the outer workspace
count. Live gates and RUST_TEST_THREADS were unset, HOME stayed/Users/james and
each invocation used a fresh0700 private FF_RDP_HOME. A subsequent CLI/xtask
build, this plan's validation and HYALO005 (478files, no issues) also passed.
Exact command/environment/start/end/exit receipts, source recovery/freeze and
finite native proposal are private under
`.git/ralph-loop/20260924-all-open/iter272/implementation/` in the primary checkout.
No current-runtime native test, closing sweep, remote action or completion was
performed during this recovery phase. The two historical failed sweeps and all
original criterion wording remain intact.

## Exact-action route evidence repair — 2026-09-24

The fresh current-base review found a qualification gap: the old live272 route
labels came from requested flags. A later daemon lookup could fall back to direct;
five expected-error cases per route returned before successful-envelope route
metadata was added. Setup/readback/daemon presence could not establish the route
of that different failed action. No historical fallback is asserted, but old
both-route labels and passes alone are not proof of actual per-action routing.

Click and type now emit an opt-in debug record immediately after obtaining their
actual connected context, before auto-wait/action work that can fail. The record
uses that context's resolved route. It changes neither routing nor the JSON error
schema, waits or lifecycle. Live272 enables only this tracing target and requires
exactly one matching action/route record from the action subprocess's own stderr.
Missing, wrong, malformed or duplicate records reject qualification. A neighboring
setup/readback record cannot satisfy the action check. The original12fixture
semantics and all original acceptance wording remain unchanged.

The finite Firefox-free proof runs click/type expected-error actions over direct,
real mock-backed daemon and deliberately corrupt-registry fallback connections.
It requires actual evaluation/error output, accepts the resolved route and rejects
the opposite route, including the newly demonstrated daemon-requested/direct
fallback. A separate parser control rejects absent/mismatched/duplicate evidence.
This new proof is separate from historical native passes and deadline mutations;
current-runtime native qualification and a clean closing sweep remain owed.

Repair validation: the two route tests passed, including all six mock action
cases and opposite-route rejection. The first workspace run then failed the179
source-scan guard because an assertion used an indirect combined-output variable;
inlining the same output-note helper repaired the static guard without changing
route assertions. That failure remains retained. The guard's four tests passed,
then fresh ordered stable/fmt/strict all-target Clippy/normal parallel workspace
gates passed2557/0/421 (excluding the nested282child1/0/0). CLI/xtask build,
plan validation and HYALO005 (479files/no issues) passed. The unchanged helper
retains its prior24/24and mutation evidence. Repair logs, failed source, final
source/binaries and revised native proposal are private in
`.git/ralph-loop/20260924-all-open/iter272/route-repair1/`. No native capture ran.

## First current-runtime qualification failure and fixture host repair — 2026-09-24

The first of nine admitted occurrences failed on Firefox156.0.1. All six direct
cases completed their original assertions. The first requested-daemon absent
click emitted actual `via_daemon=false`, and the new route check rejected it
before publishing that case. Libtest exited101, outer runner1, without
interruption. The occurrence remains spent and failed; eight later tests remain
unspent. Strict before/after boundaries preserved the12baseline profiles plus
one attributable new launch profile. Absence after cleanup is not worker return.

Source diagnosis found a fixture target mismatch: `with_daemon_using` sets up
`127.0.0.1`, while272's command builder omitted `--host` and selected the CLI's
`localhost` default. The registry resolver compares host strings exactly; a
mismatching existing registry is not adopted. The retained success envelope
explicitly reports `host: localhost`; the private daemon log records three
listeners. No timestamped intermediate registry survives, so the exact ordering
of every later spawn/lookup remains unknown. This establishes a bad fixture
setup, not loss of a correctly matching daemon or any historical268/282cause.

The fixture now explicitly uses127.0.0.1 for every command, matching setup without
reordering cases, holding/restarting a daemon, changing waits or weakening route
assertions. Its exact command builder is shared with the six mock route controls.
A before control rejected its missing explicit host against the actual mock
daemon registry; it reaped its owned daemon and joined the mock before failing,
so the control did not launch an uncontrolled duplicate. The fixed builder must
pass those same actual route/error/evaluation checks. This changes only test
setup; product routing and deadline source are unchanged. Old labels/pass claims
retain their attribution limitation. Fresh review and a separately admitted
finite qualification on the changed fixture remain required; no capture ran as
part of this repair.

The before mock invocation failed1/2 at the explicit-host preflight, preserving
that evidence; the fixed builder passed2/2, including all six real mock route
cases. Final stable/fmt/strict all-target Clippy/normal parallel workspace gates
passed2557/0/421 (nested282child1/0/0 excluded). CLI/xtask build, plan validation
and HYALO005 (479files/no issues) passed. Exact before/after receipts, source and
four built executables are frozen privately under
`.git/ralph-loop/20260924-all-open/iter272/route-repair2/`. Tasks3/3 and AC2/3
remain unchanged, with one separately proposed changed-fixture qualification and
the original eight unspent compatibility occurrences awaiting root admission.


## Current closing1 failures and passive attribution — 2026-09-25

Current-runtime native2 qualification passed all nine original occurrences,
including all twelve actual direct/daemon272 action routes; immutable evidence
is under `20260924-all-open/iter272/native2/summary.json`. This does not erase
native1's failed route selection or either historical closing failure.

This candidate's own closing1 FAILED: 347 passed, 2 failed of349 across all six
tiers (CLI336/338 and core11/11). New272 passed. Failed159 frame parity observed
daemon1/direct2. Failed navigation timing observed wall1386ms, reported489ms,
delta897ms against unchanged750ms; reported load315.60/198.23/97.71 is observation,
not established cause. The sweep summary had executed349, skipped0, preexisting0,
vanished0, launch_timeout0, timed_out0, and profiles leaked0/unattributed0. Root
owns separate strict ownership reconciliation; those summaries are not proof of
worker return. Complete original logs and post-sweep binaries remain immutable
under `20260924-all-open/iter272/closing1/`.

The bounded source-only investigation (21:59:33–22:07:07Z September24) found
insufficient occurrence evidence to assign either cause. Both159 routes already
use explicit127.0.0.1; this is not the route-repair2 host mismatch. Frame commands
sample successive enumeration windows and previously discarded their outputs.
The public navigation timer begins at dispatch; connection, predispatch and
postcommit/drop/output/process work occupy other regions. Neither source fact
explains this occurrence without measurement.

Closing-repair1 adds opt-in passive target snapshot/result/daemon event records,
retains outputs and monotonic test-relative windows for159's same four operations,
and records navigation core/run monotonic boundaries from the exact measured
command. Actor IDs remain connection-local; target/document readiness beyond
observed packets remains unknown. Timing offsets only share origins within one
PID and scope. Startup/exit and other outside-run time remain a combined residual;
trace overhead can perturb measurement. No assertion, public elapsed meaning,
wait, retry, lifecycle hold, command ordering or fixture changed.

Two new CLI/mock tests verify target identity/result evidence and ordered timing
records from the actual subprocess PID. Existing route controls and unchanged
helper proofs remain relevant. The new private packet is
`20260924-all-open/iter272/closing-repair1/`; its receipts and freeze record the
actual focused and ordered nonlive gates, including every failure if any.

Proposed, not executed here: one original159 occurrence and one original timing
occurrence, with fresh review and root native admission, exact private homes,
launch accounting and strict before/after ownership. Stop each hypothesis after
its occurrence; an isolated pass does not explain a failed sweep or replace
closing. No discovery sweep or unchanged retry is authorized by this note. If
navigation needs a behavioral correction, original279 owns its criteria; its
exact carried-over plan remains untouched. Original tasks3/3 and AC2/3 remain;
AC3 is still unmet and no PR/merge or completion is claimed.


## Observed-snapshot correction — 2026-09-25

The two admitted diagnostic-native1 slots are spent. Original159 passed2/2
with15daemon replies and4target lifecycle records; timing passed wall489ms,
reported240ms/delta249ms with complete selected region records. Actual child
and outer waits were0. Root's qualification preserves the initial new-profile
refusal and later exact attribution for timing. Those passing isolated samples
do not explain old frame1/2 or timing897, and do not authorize repeat isolation.

The next finite source investigation (22:28:29–22:31:56Z September24) identified
a separate concrete defect in the affected frame path: largest-snapshot retention
can undo lifecycle updates already received from the daemon. The daemon's stored
forms and the direct event replay both remove destroyed actors and replace
updated forms; the CLI ignored a newer snapshot unless its count increased.
It could also hide a later unavailable watcher behind a prior ready snapshot.

A deterministic production-helper control preserved the old selection rule:
observed removal returned2instead of1, an equal-count form update retained
about:blank, and withdrawn readiness returned success. Result1pass/3fail,
actualexit101. The initial incorrect --lib invocation also exited101 because
ff-rdp-cli is binary-only; both failures remain in closing-repair2. After changing
selection to the newest observed snapshot, all4controls passed. The fourth
control shows that an early daemon event cut can honestly contain1target while
a later direct cut contains2; same-cut sets agree. This proves a temporal limit
of the parity premise, not that late arrival caused the historical mismatch.

The repair changes only frame snapshot selection/finalization and adds those
four tests. It retains the original polling/deadline, current diagnostic records,
readiness refusal and all live assertions/fixtures. No lifecycle hold, delay,
extra request, wait or retry was introduced. Original137/159 same-target intent
requires respecting observed removals/replacements, but the unresolved159test
still compares different windows. No criterion-preserving correction to that
live temporal assumption has yet been established, so it is not waived.

Timing remains under original279. The recorded connection45.583ms,
predispatch159.125ms, dispatch→commit240.255ms and postcore29.830ms quantify
only the new passing sample. The source excludes these outside-dispatch regions
from publicelapsed; it does not bound their sum by750ms. A scoped279mock control
could delay only existing predispatch replies and measure the exact regions,
without redefining publicelapsed or weakening750/>5. No279behavioral change or
such timing control ran within272, and its exact carryover plan is untouched.

Private `20260924-all-open/iter272/closing-repair2/` contains before-source,
failed/passing controls, actual ordered gates, immutable source/binary freeze and
handoff. No new native occurrence ran. Prior diagnostic captures remain spent;
any further capture needs a distinct reviewed hypothesis and root admission.
Original tasks3/3, AC2/3 remain; old/current failed closings remain failures.

## Integration after completed279 — 2026-09-25

PR270 merged279 at `ffa4ef2e6fdc211d3e0bd40a9e2f95cec4dd33e6`.
This candidate starts from that merge on `iter-272/deadline-final-20260925`.
Fourteen272 source/test/plan paths were imported byte-for-byte from the reviewed
D4 checkout before updating this plan's branch and appending this section. The
old checkout, including its historical279 plan and new283 plan, remains intact.
The merged279 navigation implementation, three consumer guards, timing-record
test and live timing diagnostics are retained; the superseded272-named timing
record test is not duplicated. No deadline, route, snapshot, fixture, assertion
or wait was changed during integration.

The accepted272 deadline, actual-route, host, diagnostic and snapshot reviews
remain applicable to their unchanged inputs. Native2's nine successful finite
occurrences, including twelve actual action routes, remain recorded evidence;
they are not repeated or represented as new execution. The latest272 closing2
remains FAILED348passed/1failed of349, with the attributable886ms timing gap
against the unchanged750ms bound. Completed279 owns its demonstrated correction
and limitations; its successful sweep does not satisfy272's own closing.
Original159's unexplained observation-window mismatch remains separately filed
in [[iteration-283-frame-parity-observation-windows]], with its original scope.

Integrated ordered gates, source/binary attribution and subsequent closing are
recorded separately when actually complete. Original tasks3/3 and AC2/3 remain;
no272 completion, successful closing or merge is claimed by this integration.
Exact imports, retained279 hashes and old-checkout preservation are recorded in
`.git/ralph-loop/20260924-all-open/iter272/integration279/` in the primary checkout.

## Completion — 2026-09-25

Original tasks3/3 and acceptance3/3 are fulfilled on the integrated279 baseline.
The dated states above remain historical. Fresh independent integration review
accepted the unchanged imported repairs and their interaction with279, with zero
findings. One integrated stable-update/fmt/strict workspace all-target Clippy/
normal parallel workspace sequence passed2566/0/421 (nested282 child excluded).
No successful focused/native cases were repeated: native2's nine occurrences and
twelve actual direct/daemon action routes remain applicable to unchanged inputs.

This iteration's own closing3 finished at08:30CEST with349passed/0failed across
all six exact tiers (CLI338 and core1/3/3/2/2), with both live gates enabled:

```text
LIVE_SWEEP_SUMMARY executed=349 skipped=0 preexisting=0 vanished=0 launch_timeout=0 timed_out=0 total=349
LIVE_SWEEP_PROFILES leaked=0 unattributed=0
```

The primary checkout's private
`.git/ralph-loop/20260924-all-open/iter272/closing3/` retains the full profile-root
summary, actual sweep/outer exit0, raw browser actual wait and cleanup, all-tier
names,574unchanged source inputs and archived current executable hashes. All145
baseline profiles were conserved and all ten added profiles attributed;345
attempt records have matching start/output pairs. Passing test output and process
absence are not treated as complete worker-return evidence. Protected desktop,
Firefox package and real profiles remain unchanged. The integration archive
preserves the original checkout's18dirty paths and all previous failed results.

Final offered xtask checks and plan/frontmatter checks are recorded under
`iter272/final-gates/`; absent dogfood_script remains an explicit skip, not live
proof. No source change or repeated workspace/live run accompanies completion
bookkeeping. Iteration283 retains the independent observation-window question.

## Carry-over

| Observation | Disposition |
| --- | --- |
| Auto-wait overran readiness/stability budgets or lost useful diagnostics. | Closed in this PR: absolute deadline scopes, cached validated observations, no post-expiry probes, scripted controls and actual owned-browser timing/action proof. |
| Initial baseline setup/usage errors, first lint/source-guard failures and expected mutation failures. | Closed as recorded in the historical sections; repaired setup and meaningful final-source evidence, with every failed result retained. |
| Historical contended launch/watchdog occurrence. | Fold: completed iteration273 retains that occurrence and its attribution limits; no assumption that the matching test name proves an identical cause. |
| Historical navigation timing809/897ms and attributable886ms occurrences. | Fold: merged iteration279 (PR270) supplies the demonstrated correction, unchanged assertions and explicit historical attribution limits. Earlier sweeps remain failed; closing3 supplies this iteration's own passing result. |
| Native1 requested-daemon case actually used direct routing. | Closed in this PR's fixture: explicit127.0.0.1 matches setup, before/fixed route controls and native2 verify actual routing. No product routing change or historical268cause is claimed. |
| Latest snapshot removal, equal-count replacement and readiness withdrawal were discarded. | Closed in this PR: newest observed snapshot wins; three before-failing controls and four passing corrected controls, unchanged deadlines and assertions. |
| Historical159daemon1/direct2 mismatch across separate observation windows. | Filed [[iteration-283-frame-parity-observation-windows]] with reviewed scope; later passes do not explain the historical mismatch. |
| Integrated build freeze parser initially expected wrong Cargo output punctuation. | Closed offline using retained output; failed parser preserved and no test/build repeated. |
| Required clean closing acceptance. | Closed by closing3:349/349, zero leaked/unattributed profiles, exact baseline conservation and owned-browser cleanup. |
