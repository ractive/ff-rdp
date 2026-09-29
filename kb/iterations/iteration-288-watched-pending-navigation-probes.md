---
branch: iter-288/watched-pending-probes
date: 2026-09-28
depends_on: []
dogfood_path: >-
  Prove the watched event-wait path with a scripted live-to-Pending-to-live
  transition, then validate the corrected product with its required dual-gate closing
  sweep. Preserve one navigation and the existing readiness deadlines.
first_call_sites: []
status: done
type: iteration
title: "Iteration 288: Stop navigation probes to invalidated watched consoles"
tags:
  - iteration
  - navigation
  - carry-over
---

# Iteration 288: Stop navigation probes to invalidated watched consoles

Filed carry-over from iteration286 closing1; filing this plan does
not add it to the current execution queue (286, then287, then268).

## Observation and bounded repair

The failed `live_262_target_snapshot::live_262_watched_target_prevention_contract`
occurrence destroyed its outgoing watched target at17:20:51.937272Z on2026-09-28.
The retained trace then contains53 outgoing evaluateJSAsync requests to the old
console and54 noSuchActor replies, including one earlier in-flight request.
These are navigation probes; final eval047 never sent an evaluation request.

The offline audit identifies a separate product defect: refresh_probe_console_actor
retains the previous console on authoritative Pending; the eager event branch
ignores its false return, and the same-document branch also permits the stale
probe. The stricter watched fallback already requires a live snapshot. Repair
both event-phase branches, keeping direct-route behavior separately covered.
Pending must continue bounded event processing/snapshot observation, without
reissuing navigation or invoking legacy getTarget.

This proves futile requests to an invalidated actor. It does not establish why
Firefox delivered no replacement target, or prove that removing the probes fixes
that absence. A delayed stale response spending the deadline is a risk, not a
measured effect of this occurrence. Missing replacement belongs to iteration289.

## Tasks [3/3]

- [x] Add a scripted control through the actual event-wait path: old live target,
      authoritative Pending/destruction, then a fresh live target. Observe every
      evaluation destination, navigation request and legacy getTarget request.
- [x] Admit watched probes only for a current live snapshot in both eager and
      same-document branches; retire invalidated probe identity consistently.
      Preserve atomic epoch/href/readyState semantics and existing deadlines.
- [x] Obtain independent scoped repair/proof review, ordered workspace gates,
      relevant native navigation coverage and this product iteration's own
      reconciled dual-gate closing sweep; retain every failed occurrence.

## Acceptance Criteria [3/3]

- [x] A before-failing control observes stale evaluation on the original path;
      the corrected path sends none after Pending, finishes on the fresh atomic
      sample, sends exactly one navigateTo and sends zero legacy getTarget.
- [x] Pending-until-deadline returns the original bounded failure with no stale
      eval. Still-live same-document and direct-route controls retain correct
      completion. Removing each Pending guard separately fails on an observed
      forbidden request, rather than only on a harness timeout.
- [x] Independent review and required final-source validation pass without
      weakened timing, readiness or navigation assertions; missing replacement
      and historical262/267/268 causes are not claimed as repaired.

## Finite validation and evidence

Start with offline controls, using scripted protocol behavior rather than
invented recorded-Firefox fixtures. One baseline/corrected proof and one mutation
per separately required gate suffice; no Guardian replay or focused timing
capture is needed to establish this repair. A changed protocol/spec method, if
proposed, additionally needs the repository's qualified native proof; none is
proposed here. The closing sweep is validation, not discovery by repetition.

Primary private evidence is `.git/ralph-loop/20260924-all-open/iter286/`:
`audit-missing-target-offline1/report.md`, `audit-closing1-failures1/report.md`,
and `closing-preparation1/failure-bodies.txt`. Navigation trace SHA256:
`5ed73a7be9493b75c39e7efffcbb76789afb030bf324e86137642f58d9f5549e`.
Iteration286 owns its own tab-readiness and262 phase-attribution harness repairs.
Original268 tasks and AC0/4 remain unchanged; this plan supplies no original
224/240 failure attribution. Windows259/PR281 and266 remain deferred.
## Additional preserved closing2 occurrence — 2026-09-28

Closing2 separately records one post-destruction atomic readiness send and one noSuchActor, not closing1’s53/54. The final eval never sent. This strengthens the existing stale-probe observation without establishing why the replacement is missing.

Evidence: private iter286/closing2-preparation1/failed262-proof.tar.gz and
iter286/audit-closing2-target1/report.md. No execution or acceptance credit.

## Focused implementation and proof — 2026-09-28

The owner admitted 288 as a new finite queue after 287 merged and 268 stopped;
the original filing-only scope statement above remains historical. The candidate
is based on merged main `c696f09f98ff17c4c8a18bccd1630f00076d9a06` and imports no
unmerged 259, 268 or 286 product code.

`ReadyStateProbe` now represents an absent console explicitly. Watched Pending
and failed watched resolution retire its cached identity. Both scheduled
same-document checks (including plain navigate) and eager atomic checks require
current live resolution. Direct-route lookup failure keeps its existing
best-effort behavior. Retired identity is also unavailable to event URL fallbacks.
No protocol method, navigation retry, legacy watched getTarget, readiness sample
expression or product deadline changed.

The scripted controls use the real `wait_for_doc_complete` event loop with raw
navigation/destruction packets and an authenticated snapshot endpoint. They are
protocol scripts, not Firefox recordings. Every main-socket request is recorded.
The original code failed on observed old-console evaluations in both probe
branches; surviving watched and direct same-document controls passed. The first
harness run failed because accepted snapshot sockets inherited nonblocking mode;
that failed setup is retained separately and earns no baseline evidence.

The corrected focused navigation suite passed **70/70**. Controls cover two
Pending snapshots followed by fresh live identity, eager completion from the
exact atomic epoch/href/readyState expression, plain-navigation and history
same-document probes, Pending to the original bounded timeout, and still-live
watched/direct completion. Each control observes exactly one navigateTo and zero
legacy getTarget; Pending-only controls send no evaluation and retire identity.

One mutation per branch bypassed its current-live revalidation gate while leaving
the other branch intact. Each failed on an observed `evaluateJSAsync` to
`old/console` after the scripted destruction/Pending transition. The server closes
on that forbidden request, so the negative oracle is not a harness timeout.
The candidate source was restored byte-for-byte after each mutation, and its test
binary was rebuilt without replaying passed experiments.

Private evidence: `.git/ralph-loop/20260924-all-open/iter288/implementation1/`.
`baseline.log` is the rejected harness setup, `baseline-qualified.log` is the
before-failing proof, `corrected-navigation.log` is the final focused suite,
`mutant-same-document.*` and `mutant-eager.*` retain both negative controls and
restoration hashes. `report.md` and `final-manifest.json` bind the candidate and
execution receipts. Actual token usage is unavailable.

At this focused-proof checkpoint, independent review, ordered workspace gates,
native navigation coverage and the own dual-gate closing sweep remained open;
the closing results below record their later execution. These controls do not
establish or repair why Firefox sometimes emits no replacement target, and do
not reinterpret historical262/267/268 outcomes.

## Closing validation — 2026-09-29

Independent product review accepted the navigation repair and focused proof with
zero actionable findings. A separate scoped review accepted the test-style repair.
After the comment-only stderr annotation correction and validation-environment
repair, the ordered final-source gates passed: `cargo fmt`, strict workspace
Clippy, then workspace tests **2723 passed / 0 failed / 429 ignored**. The outer
N4 evidence-extraction driver subsequently returned1; its actual successful
command receipts were preserved and independently reviewed, not rewritten.

This iteration's own six-tier dual-gate sweep C1 returned1. Actual verdicts were
**348 passed / 3 failed**: CLI337/3 plus core1/0,3/0,3/0,2/0,2/0.

```text
LIVE_SWEEP_SUMMARY executed=348 skipped=0 preexisting=0 vanished=0 launch_timeout=3 timed_out=0 total=351
LIVE_SWEEP_PROFILES leaked=0 unattributed=0
```

Both `FF_RDP_LIVE_TESTS=1` and `FF_RDP_LIVE_NETWORK_TESTS=1` were set. The original
startup and sweep watchdog budgets remained unchanged. All351 planned names and
341 paired launch attempts across17 ledgers were reconciled. Six retained
fixture profiles were attributed; prior profiles and protected/real state were
preserved. The watched-target prevention case passed. Three other cases failed
in their initial Firefox constructor before any flag or navigation assertion.
They remain three distinct startup failures, with unknown causes, in
[iteration294](iteration-294-managed-startup-timeout-attribution.md).

The normal-build and integration-build CLI bytes differed. Independent audit and
review accepted the matching1211 source inputs,131 direct CLI inputs, actual
Cargo-driven execution and dependency/configuration correspondence as sufficient
for reusing the348 unchanged passes. Contemporaneous C1 compiler JSON/rustc argv
and per-test runtime executable attestations are absent; later cache observations
do not manufacture them. Neither binary equality nor startup causation is claimed.

The three exact startup-blocked cases subsequently passed once each, in isolation,
using the same frozen integration test and verified compiled-path CLI, both live
gates, original assertions and unchanged product timeouts:

| Separate supplemental case | Result | Test process duration |
|---|---|---|
| `live_161_eval_and_flag_strictness::live_161_fields_and_sort_reject_unknown_names` | 1 passed / 0 failed | 4.777s |
| `live_224_with_page_connection_reset::live_repeated_hop_never_loses_the_connection` | 1 passed / 0 failed; 12 hops,0 reconnects | 18.005s |
| `live_92_navigate_epoch::live_index_navigate_parity` | 1 passed / 0 failed | 2.164s |

The supplemental runner returned0 after33.033s, with actual child waits and
qualified per-case and final cleanup. It used separate private HOME/FF_RDP_HOME
directories and an external non-Git TMPDIR, without a port6000 fixture or rebuild.
These isolation differences are recorded; no historical startup cause follows
from the passes. Full-run profiles went566→567, with one dead-owned retained
directory exactly attributed to the92 case; there were no unexpected survivors.
The existing LiveFirefox Drop ends its browser without deleting that directory;
it is preserved as evidence, not counted as a live-owned profile leak. Real and
protected state and all prior profiles were conserved.

All351 planned names therefore have successful final-product-source coverage
across C1 and the three supplements. This is not a successful351/351 sweep: C1's
failed summary, exit1 and three startup failures remain unchanged. Independent
workflow/evidence reviews accepted this recovery and bounded source correspondence;
no missing-replacement or historical262/267/268 repair is claimed.

## Carry-over

| Occurrence | Disposition |
|---|---|
| First scripted baseline failed at inherited nonblocking snapshot socket setup | Closed in this PR: blocking accepted socket repair; separate qualified before-failing proof and corrected70/70 suite. Failed setup retained, never counted as the baseline. |
| Two deliberately removed Pending guards each sent a forbidden old-console request | Closed in this PR: negative mutation evidence, restored source verified. These are expected controls, not unresolved product failures. |
| N1 strict Clippy rejected a test-only redundant closure | Closed in this PR: scoped style correction, independently reviewed; final N4 Clippy passed. |
| N2 driver initially rejected formatter-only hash changes | Closed in validation tooling: exact formatting delta preserved and reconciled; no behavior changed or successful fmt repeated. |
| N2 workspace stderr-policy invariant failed | Closed in this PR: required test-only stderr annotation, with focused invariant proof and final full workspace pass. |
| N2 post-boundary detected another Claude Firefox session and changed real state | Closed as unqualified historical environment: retain N2 result; N4 and C1 used a fresh verified baseline after the owner released that session. No external state was reset. |
| N3 outside-repository fixture found `.git` through its private TMPDIR | Closed in validation setup: external non-Git TMPDIR verified before N4; all workspace tests then passed. |
| N4 extraction assumed the normal CLI and test artifact were hardlinked | Closed in validation tooling: preserved driver failure and actual successful ordered commands; independent actual-gates review accepted corrected provenance. |
| N4 extraction mishandled absolute include paths | Closed in validation tooling: offline extractor repaired without rerunning successful Cargo gates; independent provenance review accepted its result. |
| First static dogfood gate omitted its required live environment gate | Closed in gate invocation: corrected invocation explicitly reported SKIP because this plan has no dogfood script. That skip is not counted as live proof; C1 and the separately required case coverage supply it. |
| C1 normal/integration CLI byte mismatch | Closed in evidence assessment: independent dependency audit and bounded reuse review; absent historical compiler/per-test attestations remain explicit. |
| C1 `live_161_fields_and_sort_reject_unknown_names` startup timeout | Filed in iteration294: PID54312/port59458/30511ms, cause unresolved; keep any supplemental pass separate. |
| C1 `live_repeated_hop_never_loses_the_connection` startup timeout | Filed in iteration294: PID75826/port63303/30507ms, cause unresolved; this is not268's original connection-reset attribution. |
| C1 `live_index_navigate_parity` startup timeout | Filed in iteration294: PID97345/port51217/30512ms, cause unresolved; keep any supplemental pass separate. |
| Startup diagnostic opened in710ms; fixed5/20s observations were not reached | Filed with iteration294 as a non-reproduction. One-of-one schedule consumed; no historical cause or repair credit. |
| Diagnostic observer review found false listener absence on collection failure and a stale launcher-state check | Closed in private tooling before execution: unknown collection cannot permit sampling, and launcher/root identity is rechecked immediately before sampling; fresh independent review accepted both repairs and offline negative controls. |
| Supplemental runner review found its intermediate census blocked the fallback cleanup | Closed in private tooling before execution: phase distinction repaired and independently accepted with29 synthetic controls; admission and final boundaries still reject leftovers. No runtime was attempted with the rejected version. |
| Diagnostic cleanup packet returned failure when two new group members appeared | No product plan: the guard correctly refused an unqualified group signal; only the qualified root received TERM. Fresh qualified full-run census found no survivors, with prior profiles/real/protected state preserved. Keep failure and unknown transitions intact. Any survivor or need to reuse this private helper requires a new ownership/cleanup assessment before execution. |
| Supplemental92 case retained its attributable profile directory | No plan: existing LiveFirefox Drop terminates the browser but leaves the dead-owned directory; qualified census found no survivor. Preserve it as evidence. A live owner or missing attribution would require investigation and fail closing. |
| Earlier missing replacement and historical262/267/268 causes | Existing `iteration-289-missing-watched-target-replacement.md` (preserved in the286 checkout) and `iteration-268-daemon-pre-auth-connection-loss.md` checkpoints remain unresolved; removing stale navigation probes establishes no missing-replacement cause or original224/240 completion. |
| Windows259 and dependent266 | Owner-deferred Windows investigation and dependency remain binding; no validation or merge credit from288. |

Private evidence is under `.git/ralph-loop/20260924-all-open/iter288/`: product
and style reviews, `nonlive1` through `nonlive4`, `review-actual-gates1`,
`closing-preparation1`, `review-failed-inventory1`, `audit-binary-difference1`,
`review-c1-evidence-reuse1`, `review-closing-requirements1`,
`startup-c1-offline1`, `startup-unified-log1`, and
`startup-diagnostic-preparation1/root-release1/receipt.json`, and
`isolated-closing-preparation1/` with its actual per-case results, root release
and retained-profile classification. All failed records remain intact. Actual
token usage is unavailable.

Final scoped closing/documentation review accepted these results and dispositions
with zero actionable findings. Applicable final plan, Firefox-reference, Hyalo
and whitespace checks passed; unchanged static-gate evidence is reused. The
post-commit actor/KB check and exact-head CI remain publication gates.

## Publication gate reopened — 2026-09-29

PR284 head `d3bc6a3a6331fd3e7215428eaba77dfae7379115` failed its macOS unit
job in workflow36550437216. The existing long-string watched-terminal mock
panicked while receiving a snapshot connection (`UnexpectedEof`); the stale
caller separately returned its expected803ms timeout. Cause and repair are
under investigation. Closing task3/AC3 are reopened pending repair and final
validation; prior passing live evidence and the failed CI output are retained.

## Publication fixture repair — 2026-09-29

The CI log cannot distinguish an empty query from a partial frame, so its exact
EOF cause remains unknown. A separate deterministic control established a fixture
contract mismatch: authentication and greeting do not commit a snapshot query;
the client's actual100ms sub-deadline may expire before query submission. The
cfg(test)-only observer now binds the successful auth write, accepted peer and
actual deadline. The byte-aware fixture accepts only a zero-byte close observed
after that matched deadline, recording it separately from completed queries.
Early closes, partial frames, malformed requests and unrelated errors still fail.
All original readiness assertions, real worker joins and product budgets remain.

One forced baseline returned101 with the old fixture; the single corrected module
run passed13/13. The forced boundary is a counterfactual, not attribution of the
historical CI EOF. Independent scoped review accepted the repair with zero
findings. N5 retained successful fmt0 followed by an outer formatting-reconciliation
failure, then Clippy101 for similar local names; workspace tests were not started.
A three-reference alpha rename resolved the name finding and passed a fresh
independent review. No suppression, timing change or additional experiment was used.

Independent cfg(test)-erasure comparison found unchanged non-test code and all429
core/integration inputs unchanged. Consequently C1 and its three supplemental
passes remain applicable with their existing provenance limitations; no full
live sweep or answered focused experiment was repeated. Evidence: private
`iter288/ci-repair1/implementation1`, `review-ci-fixture-repair1`, `nonlive5`,
`ci-style1` and `review-ci-style1`. Every failed result remains preserved.

Final N6 gates then passed in order: fmt0, strict workspace Clippy0, workspace
**2725 passed / 0 failed / 429 ignored**, across38 summaries. Actual child waits
and enclosing exit0 are recorded; qualified final census preserved prior profiles,
real and protected state with no unexpected survivors. Four dead-owned fixture
profiles remain in the sole private workspace home (567→571); an initial offline
all-profiles equality assertion rejected those additions and was reconciled without
rerunning any gate. All four carry owner35152, absent from the post census; the
directories remain preserved. This is not proof of native worker return.
Source-invariant and live-layout gates passed. The fixture mismatch and Clippy
finding are closed; the historical CI EOF remains unclassified. Original task3/AC3
are complete based on reviewed repair, final gates and applicable closing evidence.
Exact-head green CI and GitHub merge remain publication requirements.
