---
title: "Iteration 276: Keep readystate completion bound to one fresh document"
type: iteration
date: 2026-09-19
status: in-progress
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

### Delivery acceptance [0/3]

- [ ] Actual navigation callers reject stale, ambiguous-blank and split-sample
      completion, with repair-sensitive controls and unchanged action count
      and deadline; supported navigation boundaries remain covered.
- [ ] Real Firefox navigation and cascade reach the intended fixture and
      retain the original imported-CSS, h1, red-color and matched-rule checks,
      without widening the existing sleep or timeout.
- [ ] Ordered gates and this iteration's own dual-gate closing sweep pass;
      any independently selected query-document mismatch is either repaired
      with evidence or explicitly dispositioned without claiming old cause.

The owner’s September25 all-open adaptation request supplies the prospective
delivery scope above. Completion under this scope must say so explicitly and
retain every unfulfilled historical AC; it must not report the original
attribution criterion as passed. Earlier investigation schedules and restrictions
remain historical records, not renewable capture allowances. Use only the
finite controls needed for the new contract and the required closing validation.
