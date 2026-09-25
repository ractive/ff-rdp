---
title: "Iteration 276: Keep readystate completion bound to one fresh document"
type: iteration
date: 2026-09-19
status: done
branch: iter-276/cascade-external-css-missing-fixture
depends_on: ["277"]
first_call_sites: []
dogfood_path: |
  FF_RDP_LIVE_TESTS=1 FF_RDP_LIVE_NETWORK_TESTS=1 cargo test -p ff-rdp-cli --test live live_cascade::live_cascade_returns_matched_rules_external_css -- --include-ignored --exact --nocapture --test-threads=1
  # Capture failing URL/document/DOM/route before teardown; an isolated pass is only a control.
tags: [iteration, carry-over, cascade, testing]
---

# Iteration 276: Keep readystate completion bound to one fresh document

Iteration267's final-source dual-gate sweep on Firefox156.0 failed
`live_cascade::live_cascade_returns_matched_rules_external_css` at its cascade
success assertion (`tests/live/live_cascade.rs:177`). Navigate had returned success,
but `cascade h1 --prop color` returned exit1 and
`{"error":"no element matching selector 'h1'","error_type":"User"}`.
The expected data-URL fixture contains an h1 and an imported data-URL stylesheet.
No failed-occurrence URL, current document identity, DOM or route was captured.
Neither an absent element's cause nor a relationship to267's socket-mode repair
is established. Do not infer a timing or stylesheet-loading cause from the
existing300ms sleep, or weaken the selector assertion to make this pass.

Evidence: primary checkout `.git/ralph-loop/20260919-queue/iter267/sweep.log`
lines361–370, source freeze in `source-freeze.sha256`, launch ledger in
`sweep-launches.log`. This sweep has334CLI passes/1failure, nine core tests
initially unexecuted because port6000 was absent at sweep startup, and zero profile
leaks. The core tiers were executed separately; that does not change this failure.
This is distinct from the applied-styles observation in
[[iteration-274-styles-applied-unattributed-recurrence]].

The exact serial dual-gate isolation passed in2.66s with computed color `red`;
`isolated-cascade.log` preserves it. No source changed between the failure and
that control. The pass does not provide the missing failed-document attribution
or establish a repair.

## Tasks [2/3]

- [x] Retain the failed occurrence and exact isolated control without treating a
      later pass as a repair.
- [ ] Capture URL/document/DOM and route at a failing occurrence in a bounded
      reproduction or independently required sweep; identify the mechanism.
- [x] Repair only the demonstrated cause with meaningful regression coverage and
      the required closing sweep, or state the precise remaining evidence gap.

## Acceptance Criteria [0/3]

- [ ] The failing cascade result is attributable to a concrete document/route and
      the missing h1 has an evidence-backed explanation.
- [ ] Any repair preserves the real imported-CSS assertion and has a regression
      that fails without the repair and passes with it.
- [ ] Original failed and successful controls, ordered gates and the repair's own
      dual-gate sweep are recorded honestly; no timeout/sleep widening substitutes
      for diagnosis.

## Execution boundary

Filed as267 carry-over, outside the selected implementation queue. No276 product
implementation is authorized or claimed by this filing.

## Execution clarification — 2026-09-23

The owner authorized the 272–278 queue after parking 268. Earlier dated unselected-run
boundaries are historical; 271 remains excluded and 259/266 constraints remain binding.
Original tasks and acceptance criteria above are unchanged.

At merged main c761c1b1 the original data-URL/imported-CSS test remains unchanged and
routes directly via --no-daemon.262’s watched-daemon recovery does not cover this
caller. Pure readystate can accept a fresh-epoch intermediate document; separately,
cascade acquires its own target and DOM root. First discriminate those boundaries using
finite scripted callers, then declare a bounded live schedule retaining identity at both
navigate completion and the failing h 1 query. Source candidates and isolated passes do
not attribute the historical failure. Preserve imported CSS, h 1 and red-color
assertions; do not widen the 300 ms sleep. Original AC 1 still requires an attributable
explanation.

## Scripted caller checkpoint — 2026-09-24

Execution starts from merged main `4e5820b78099e1555c55cb7e0ac7e1004c840c6b`,
without the separate277 candidate. The original267 failure and2.66s isolated
control above were re-read and hashed; neither has failed-document identity.

Six browser-free scripted protocol experiments exercised the actual direct CLI
navigate and cascade callers. A stale initial epoch stayed rejected until the
1000ms deadline. Fresh intermediate blank, failed-baseline fallback0 and split
readiness/href samples returned successful blank completion. An intended navigate
followed by an independently selected blank cascade target produced the original
missing-h1 error. An intended selector control reached getApplied; it deliberately
does not simulate or certify CSS. Each case sent exactly one navigateTo with the
requested fixture and asserted the query's actor/root/selector. These are scripted
protocol tests, not recorded Firefox fixtures or a historical-cause finding.

Private evidence is `.git/ralph-loop/20260923-remaining272-278/iter276/` in the
primary repository. `scripted-fourth.log` is the corrected6/6 characterization;
`scripted-capture.log` is2/2 diagnostic-shape qualification. Earlier harness
failures and the superseded actor-changing control remain preserved. Capture-only
instrumentation retains an atomic successful readiness sample, the actual
navigation output and target metadata, and adjacent document/DOM samples around
the actual cascade walker query. Those adjacent samples are not claimed atomic
with the walker query. Original URL, imported stylesheet, h1/red/rule assertions
and300ms sleep are intact. No functional repair or original-AC completion is
claimed; the capture is stopped before Firefox pending independent admission.

## Bounded outcome — blocked checkpoint, 2026-09-24

This supersedes the pre-capture state above, retaining its private evidence.
One independently admitted original named live occurrence passed1/1 in1.86s:
`iter276/live1/test.stdout`, full135-line `test.stderr`, actual child/runner waits
and `root-live1-admission.json` preserve execution. Firefox was156.0.1,
build20260921121718; the original failure was on156.0. The direct navigation
reported25ms and the requested fixture URL. Its successful atomic readiness
sample observed that same href/documentURI, epoch1790208526443 and h1 present.
Cascade observed the fixture's complete124-character DOM with truncation false;
the actual walker query returned h1, getApplied returned the real imported
h1/color:red rule, and the original red/rule assertions passed. Original fixture,
300ms sleep and assertions were not changed. No after/retry or discovery sweep ran.

The completion trace's innerWindow8589934593/about:blank fields were CACHED
metadata from the preceding getTarget, not the actual readiness document. The
same child12 console returned the new fixture; the subsequent getTarget and
cascade acquisition reported8589934594. An actor name or cached form therefore
cannot be assumed to identify an immutable document across these requests.
This observation corrects any such interpretation of the scripted actor cases;
those cases remain explicit protocol-response schedules, not native lifecycle
proofs. Cascade's DOM sample is adjacent to, not atomic with, its walker query.

The passing occurrence supplies no failing document or missing-h1 explanation.
Neither a shared cause with274/277 nor historical timing/CSS-loading fault is
established. Original AC0/3 remains: AC1 lacks attributable failure; AC2 has no
chosen repair/before-after regression; AC3 has no repair-specific closing sweep.
Task1 is satisfied by preserved original failure/control, task2 remains unmet,
and task3 is satisfied only through its explicit evidence-gap alternative.
No claim that all hypotheses are exhausted follows. Another unchanged pass
would not fill the historical gap. A future attributable failure with actual
readiness/query evidence or a separately justified source-backed investigation
is needed before choosing a repair.

Independent capture review09051077 found one documentation error:500ms was an
absolute READ deadline, not a whole diagnostic-call or total-overhead bound.
The reviewed `live-schedule-v2.md` corrects this; source was unchanged. Source,
artifacts and clean workload state were verified immediately before execution.
Root's full post-run inventory (`live1/root-cleanup.json` and
`root-after-processes.txt`) found no unprotected Firefox or workflow workload;
owned78590 and test78588 were absent, protected14298/14301 retained exact native
birth identities, and no276 recovery signal was used. An earlier preflight had
found and separately recovered an old277-owned orphan before276 admission;
its preserved records are not relabeled as276 failure or cleanup. Process absence
is not worker-return evidence.

Temporary capture and characterization code was reconstructed byte-for-byte from
the baseline archive plus frozen patch/new file, then removed from the checkout.
All runtime/test source equals merged main4e5820b7; exact diagnostic source and
binaries remain private and recoverable. Only this plan remains changed. Ordered
checkpoint gates and applicable static checks are recorded in the private
`checkpoint-*` logs; no live gate, completion PR or merge is authorized by this
checkpoint. The source-characterization holes remain observations, not shipped
repairs or permanent tests asserting successful blank completion.

## Prospective scope adaptation — 2026-09-25

Current work addresses the demonstrated readystate completion contracts:
readiness, navigation freshness and committed href must describe one sample
of one document. Missing baseline evidence must not silently authorize stale
completion; a known nonblank request must not accept an ambiguous blank
intermediate document. Legitimate redirects and explicit blank navigation
remain supported.

The historical missing-h1 result retains its original failed record and
unknown document/route. Original AC1 remains unfulfilled. Actor names and
cached target forms are not immutable document identity. A later successful
cascade does not assign the historical cause.

### Delivery acceptance [3/3]

- [x] Actual navigation callers reject stale, ambiguous-blank and split-sample
      completion, with repair-sensitive controls and unchanged action count
      and deadline; supported navigation boundaries remain covered.
- [x] Real Firefox navigation and cascade reach the intended fixture and
      retain the original imported-CSS, h1, red-color and matched-rule checks,
      without widening the existing sleep or timeout.
- [x] Ordered gates and this iteration's own dual-gate closing sweep pass;
      any independently selected query-document mismatch is either repaired
      with evidence or explicitly dispositioned without claiming old cause.

The owner’s September25 all-open adaptation request supplies the prospective
delivery scope above. Completion under this scope must say so explicitly and
retain every unfulfilled historical AC; it must not report the original
attribution criterion as passed. Earlier investigation schedules and restrictions
remain historical records, not renewable capture allowances. Use only the
finite controls needed for the new contract and the required closing validation.

## Prospective implementation candidate — 2026-09-25

The candidate on base `86110575198422079066bd658d9e3b6c156d4846` samples
readyState, navigationStart and href in one JavaScript evaluation and uses that
accepted sample's href in pure readystate/fallback, Both's interleaved probe,
and279's unresolved watched-terminal revalidation. Refreshing an actor requires
a whole new readiness sample; a new href cannot complete an older ready sample.
The277 shortcut guard and eight existing boundaries remain intact.

Both pre-dispatch epoch captures now retain missing evidence as unavailable.
A known positive finite epoch remains authoritative for the readiness predicate,
even if href changes. With no epoch, a complete changed href against a successful,
exception-free nonempty baseline href establishes progress under the existing
changed-href contract. It does not prove a fresh document or navigation causality.
If both baseline signals are missing, or only unchanged href is available, the
poll keeps its existing deadline. Same-URL reload with unavailable epoch cannot
be certified by this fallback alone; qualifying events remain available. Known
nonblank requests reject ambiguous blank samples; redirects and explicit blank
(including scheme case) remain supported.

Sixteen declared protocol schedules drive actual direct CLI processes. The
original product source failed14/16 controls; two stale-epoch controls passed.
The corrected source passed16/16, including unavailable baseline boundaries,
known-stale epoch with changed href, explicit blank/scheme/content, redirect,
same-URL fresh epoch and Both's blank-complete → refreshed-loading → complete
sequence. Request traces assert one navigateTo and no separate committed-href
read after an accepted sample. These schedules are not native Firefox fixtures
and do not execute JavaScript. A test-only lifetime compilation error was fixed
before the before-control could execute; its raw failure remains preserved.

The focused navigation suite passed62/62 including277's eight cases, watched
Pending/retirement/terminal errors,279 terminal-event gating and the direct
poll's zero-budget, exception, protocol-error and absolute-deadline behavior.
A separate watched-terminal mutation introduced a second href read after the
accepted sample: the strengthened actual request assertion rejected it. Exact
restoration passed the three-leg terminal-event control. The first CLI after
build emitted one unused-assignment warning; that redundant assignment was
removed before the focused pass. All raw outcomes remain retained.

The independent cascade acquisition control still can select a different
query document after successful navigation. This is an explicitly retained
boundary: navigation cannot attest the identity of a later separate command's
query. The positive selector control only reaches getApplied and does not
simulate CSS. The original live imported stylesheet, h1, red-color, matched-rule
assertions and300ms sleep are unchanged. No new native occurrence or historical
cause is claimed by these controls.

Private evidence is primary
`.git/ralph-loop/20260924-all-open/iter276/current/`: accepted design/source
freeze, finite control schedule, raw argv/env allowlist/start/end/actual exits,
failed before controls, restored controls and final candidate attestations.
Original tasks2/3 and historical AC0/3 remain unchanged. Delivery acceptance
remains unticked pending final ordered gates, independent implementation review
and this iteration's own dual-gate closing sweep. Root owns those closing
steps and publication; no PR, merge or completion is claimed here.

The initial ordered strict Clippy run rejected two test-only style issues
(enum-variant naming and a redundant let-and-return). Both were corrected;
the failed gate is preserved and the ordered sequence restarts before testing.

Current candidate ordered gates then passed: formatting, strict all-targets
workspace Clippy, and normal parallel workspace tests2585/0/421. The separately
printed282 child result is excluded from2585 (raw summary sum2586).
Plan validation and HYALO005 passed. This is nonlive implementation validation;
independent implementation review and the candidate's own dual-gate sweep still
remain. The earlier failed Clippy record is retained, not replaced by this pass.

## Initial review correction — serialized long strings, 2026-09-25

Independent implementation review found one P2 regression: a previously inline
9990-character href becomes a serialized readiness sample above Firefox's
10000-character long-string cutoff. The initial candidate accepted inline
strings only, causing the valid sample to time out. Installed Firefox source
confirms the cutoff; this is source-backed protocol evidence, not a new native
occurrence or attribution of the historical missing-h1 failure.

The correction resolves the returned immutable long-string sample with the
existing bounded LongStringActor support before applying the same predicate.
Evaluation, substring retrieval and completed-fetch release stay inside the
existing absolute deadline, target guard and event-replay scopes. Both's
interleaved operation is explicitly bounded by its existing event deadline.
The MAX_FETCH announced-length cap and an actual UTF-8 byte cap bound input;
JavaScript exceptions do not trigger a result fetch. An incomplete fetch sends
no speculative cleanup request. This does not claim to repair the separate
general reply-ownership/connection-lifecycle restrictions.

One finite actual-CLI before matrix failed0/2 for direct and Both routes at the
9990-character URL boundary. The corrected direct control passed. The first
corrected Both command returned the exact URL and HTTP200, but its scripted
server incorrectly replied to one-way unwatchResources and hit BrokenPipe;
that harness failure is preserved and the protocol script was corrected.
The single corrected Both control then passed1/1 with the exact returned URL,
HTTP200, substring/release request trace and no second href query.
The substring-time network/status events exercise replay; no second href
evaluation or additional navigation action is permitted. The new watched
terminal control retains all three terminal/stale/no-terminal legs. Fifteen
direct/watched fetch cases cover successful fetch/release, the absolute budget,
size rejection without substring, exceptions without fetch, terminal actor and
malformed-reply errors, release errors and watched destruction during fetch.
These remain declared protocol schedules, not Firefox fixtures or JavaScript
execution. The original cascade source and historical AC0/3 are unchanged.

Repair evidence is primary
`.git/ralph-loop/20260924-all-open/iter276/repair1/`. The initial candidate's
source, binaries and all failed/passing evidence remain immutable under
`iter276/current/`; its initial implementation checkpoint is superseded by this correction.
The corrected candidate still needs fresh scoped review and its own closing
sweep before prospective delivery can be completed.

Repair1 ordered gates passed: stable update, cargo fmt, strict all-targets
workspace Clippy, and normal parallel workspace tests2589/0/421. The raw
summary sum2590 includes the separately invoked282 child; it is excluded from
2589. One earlier Clippy failure required a test-only missing semicolon; that
failure and the pre-correction source pins are retained. The corrected source
was formatted and pinned before the successful Clippy/test sequence.

## First closing sweep and consolidated repair2 — 2026-09-25

Repair1 passed fresh scoped review. Its own closing sweep finished347/349 with
zero profile leaks and unchanged frozen575 source inputs/five binaries. The
original cascade case passed. Two failures remain preserved under primary
`.git/ralph-loop/20260924-all-open/iter276/closing1/`; this was not a qualifying
all-green closing result and does not complete delivery.

The166 daemon no-trailing-slash leg returned the correct canonical committed
URL and complete readiness after21196ms, but null/not_observed instead of200.
Source identifies Both's dedicated readystate fallback: its events phase loses
its local network-status tracker when it times out, and fallback always returns
not_observed. This behavior predates276. The occurrence has no retained packet
trace proving whether200 arrived or why event completion was unavailable; generic
interleaved daemon EOF lines do not establish either cause. The272 direct moving
case returned an unanswered stability-probe diagnostic after1396ms against a
4000ms overall budget. Its accepted rect-sample count is unknown, so the native
occurrence does not establish demonstrated motion. Source separately showed the
stability eval-timeout branch dropped rect_changed while its ordinary deadline
branch preserved that observation. Neither source finding attributes the two
native occurrences or the historical missing-h1 failure.

An independently reviewed repair2 design retains raw network observations for
one Both dispatch through the existing event receive/replay boundaries. Only
after the fallback accepts its atomic URL and passes the existing neterror check
does it resolve those observations. It requires one qualifying resource ID,
matching accepted canonical URL (never requested-URL fallback), top-level
navigation flag, browsing context and outgoing-window identity fields. Missing
metadata, unrelated/old-window resources and ambiguous IDs remain null. Request
and update IDs stay paired; updates may precede resources. Identified requests
without status report no_status_reported, rejected/unidentified requests report
no_document_request. There is no new query, action or wait allowance, and saved200
cannot bypass the existing classified neterror failure.

Installed Firefox source supports the outgoing-window meaning of navigation
request innerWindowId. This is conservative request correlation, not immutable
document identity or proof against an unobserved concurrent same-URL navigation
from that same outgoing window. Packets without ownership fields cannot supply
a retained fallback status. Existing successful event-path status behavior and
pure Readystate semantics remain unchanged.

The diagnostic correction preserves "rect did not stabilise" only after two
differing accepted rect samples and also states the final probe did not answer.
Zero/one accepted sample stays unknown. No native assertions, stability caps,
read deadlines or post-expiry queries were changed.

The finite nonlive status matrix has10 Rust tests/15 actual-CLI legs. Valid before
source passed the two terminal/neterror preservation legs and failed13 status or
exact-reason legs; corrected source passed all15 legs. The production-helper
matrix has three zero/one/two-sample legs: before lost the known-motion wording
only in the two-sample leg. All traces and actual exits are private under
`iter276/repair2/`. These are declared protocol schedules, not native fixtures.
The first status attempt used an older shared-target CLI (its dep-info named281
and its trace sent the old boolean readiness predicate); all15 legs were rejected
as invalid product evidence. Actual binary/source/logs are preserved; rebuilding
the owned CLI corrected the evidence binding before the valid before run. A
later after attempt failed compilation on an incorrect helper parameter type;
that raw error/source is retained and the type was corrected before controls ran.
Initial/repair1 archives and both design revisions remain intact. Original tasks
and historical AC0/3 remain verbatim; fresh repair implementation review and this
candidate's own closing sweep remain owed.

Repair2's corrected production-helper matrix passed all three legs. Final ordered
stable update, fmt, strict workspace/all-targets Clippy and normal parallel
workspace tests passed2600/0/421. The raw summary sum2601 includes the separately
printed282 child and excludes no other results. Both Clippy and workspace passed
on their first final gate attempts;575 source inputs stayed unchanged from the
formatted gate pins. This is nonlive repair validation, not a new native result.

## Prospective delivery completed — 2026-09-25

The independently reviewed prospective scope above is complete (delivery3/3).
Original tasks2/3 and historical AC0/3 remain unchanged: no historical missing-h1
document or cause was recovered. Repair2 passed a fresh92-second independent
review with zero findings; the unchanged initial and long-string contracts retain
their earlier reviewed evidence. The final ordered gates passed2600/0/421 as
recorded above; no unchanged workspace run was repeated for publication.

The candidate's second closing sweep passed349/349 across all six exact tiers,
with both live gates enabled, zero skipped tests, zero profile leaks and zero
unattributed profiles. The original imported-CSS cascade, h1/red/matched-rule
assertions and300ms sleep passed unchanged. The166 canonical-URL status and272
moving-element cases also passed; these later results do not attribute their
earlier failures. All first-sweep failures remain retained separately.

Private `iter276/closing2/` records actual sweep wait0, owned raw-Firefox wait-15,
341 paired launch records with zero unpaired entries, ten attributable new
profiles and exact conservation of the224-profile baseline. Protected desktop
process identities and all four real profile markers remained unchanged.
575 source inputs, five reviewed binaries and110 Firefox package pins matched
before and after; six actual sweep test executables were additionally frozen.
Process absence is not claimed as worker return. Raw profile archive SHA256 is
`ee3ef64f93f237e0050cc381de7461f334158fca57c2167d3f6442ec635bd667`; sweep log
SHA256 is `29fd28bd0406b2a757ae1793ea805f7d80bf47ff5a03b66069830227f3b6039b`.
All applicable closing xtask checks, HYALO005 and diff whitespace checks passed.

Carry-over disposition: independently selected later query documents remain a
separate-command boundary, not a promise of navigation identity across commands.
The failed166 occurrence lacks packet/status attribution; the source-backed
status-preservation controls establish the conservative repair's contract only.
The failed272 occurrence lacks accepted-sample attribution; the diagnostic repair
reports known motion only when it was actually observed. Neither failure is
relabelled as proven old cause. General worker-return attribution remains with
284/268, and reply/grip ownership remains with259/266. No additional discovery
sweep or repetition of an answered experiment was used to close this scope.

## Publication CI fixture correction — 2026-09-25

PR274 headf4c098b49092d832a834b14ebbc714e43b3fcc87 had nine green checks and one
macOS workspace failure. The LongString matrix's final destroyed-target leg
returned Timeout at500.407ms with one evaluation, one substring and no release.
Its snapshot worker reached EOF waiting after authentication, then its join
assertion failed. The retained log does not identify which later snapshot
connection closed, whether its greeting had been decoded, or where the budget
was consumed. The other14 matrix legs passed. This failure does not attribute
any historical missing-h1 or prior native occurrence.

One instrumented destroyed-only diagnostic passed in232.370ms with the original
500ms budget and exact three-snapshot/two-evaluation/two-substring/one-release
assertions. Its phase records show initial a, retired a, then replacement b,
and all three greeting-decode boundaries. There was no recurrence, so it does
not establish the CI cause. The existing snapshot observation hook is
thread-local, endpoint-bound and cleared by a scope guard, rather than a global
timing switch; this fixture's callback records phases without delaying work.

The test helper now serializes each complete frame with encode_frame and submits
it through one write_all, removing unnecessary separate header/body writes.
This follows the other protocol fixtures and changes no production framing,
Nagle option, budget, lifecycle or assertion. It is a fixture-quality correction,
not a claim that split writes caused the CI timeout. Phase records remain so a
future failure preserves the missing connection/query boundary. The destroyed
leg is a separately named test: the same15 legs now occupy two tests instead of
one. The single corrected matrix run passed15/15; destroyed passed in222.713ms.

Private evidence is `iter276/ci-fixture-repair/`, retaining original CI logs by
reference, the instrumented diagnostic source and executable, phase output,
corrected source and actual command receipts. The code diff is confined to the
existing cfg(test) LongString module; all production and original live-source
bytes remain unchanged from f4c098b4. Thus the reviewed349/349 closing2 result
remains applicable; no new native sweep was run for this test-only correction.
Prospective implementation/closing completion above remains subject to the
publication gate: fresh scoped review and exact-head green CI are still owed.

The test-only correction passed final ordered stable update, fmt, strict
workspace/all-targets Clippy and normal parallel workspace tests2601/0/421 on
their first attempts. Raw2602 includes the separately printed282 child. The
one additional Rust test comes solely from naming the existing destroyed leg
separately; matrix coverage remains15 legs.575 source pins stayed unchanged
through the final Clippy/workspace gates. No new native validation was consumed.
