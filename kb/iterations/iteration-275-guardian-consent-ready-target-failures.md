---
title: "Iteration 275: Consent selection and truthful ready-target outcomes"
type: iteration
date: 2026-09-19
status: planned
branch: iter-275/guardian-consent-ready-target-failures
first_call_sites: []
dogfood_path: |-
  FF_RDP_LIVE_TESTS=1 FF_RDP_LIVE_NETWORK_TESTS=1 cargo test -p ff-rdp-cli --test live live_137_daemon_mode_parity::live_137_consent_accept_via_daemon -- --ignored --exact --nocapture --test-threads=1
  # Retain failed-occurrence document identity, daemon route, target forms,
  # top-level DOM and the actual CMP-frame DOM before cleanup.
tags: [iteration, carry-over, consent, live-tests]
depends_on:
  - "272"
---

# Iteration 275: Consent selection and truthful ready-target outcomes

Filed, not executed, by [[iteration-262-daemon-live-target-never-promoted]].
This owns the separate ready-target consent outcomes; it does not own target
lifecycle loss, and filing it does not satisfy 262's three-green-sweeps AC.
[[iteration-271-bbc-consent-no-cmp-recurrence]] owns BBC, not these Guardian cases.

## Evidence

At source `38add406f711434f520fa06494b2e02ab3f5e0b2`, Firefox 156,
September 19, 2026, the bounded 262 investigation ran twelve instances of the
named 137 test: six serial, then six concurrently. All original assertions
were unchanged; probes 2–12 added failure-only diagnostics before cleanup.

- Probes 1, 4, 5, 6, 7 and 10 reached live targets, then returned
  `consent_not_actioned`, Sourcepoint `detected_not_actioned`. Probe 4 reached
  targets in 70ms on debug51940, then failed. Its retained top document was
  `https://www.theguardian.com/europe`, `readyState=complete`, with
  `<html lang="en" data-previous-scroll-y="-0px" class="sp-message-open">`.
  Its wire trace includes the CMP frame identity and the failed accept-control
  evaluation. The **CMP frame's own DOM was not captured**; do not infer a
  changed selector or label from the top-level page alone.
- Probe 2 reached targets in 24ms on debug51827, then returned `consent_no_cmp`.
  A subsequent failure-time snapshot reached that same Guardian URL through
  the daemon with `readyState=complete` and `<html lang="en">`. Iframe attributes
  and lifecycle packets are retained. Snapshot time follows the failed command;
  this does not establish what was present during the detection attempt.
- Probes 3 and 9 passed. Three other probes failed target readiness before
  consent and remain owned by 262. Isolation success diagnoses neither case.

Artifacts: `.git/ralph-loop/20260919-queue/iter262/`, `probe-N.log`,
`home-N/.ff-rdp/daemon.log`, `derived/page-N.json`, and
`diagnostic-only.patch`. Exact commands, environment, start/end and exit status
are in `probe-N.meta`; diagnostics preserve the top DOM, iframe attributes,
route and status before the owned browser is cleaned up.

The current consent failure hint also suggests `ff-rdp dom --frames`, but
`dom --help` exposes no `--frames` option. Correct the recovery hint alongside
the supported inspection path when addressing this consent work.

## Tasks [0/3]

- [ ] Reproduce each outcome separately and retain the failed occurrence's CMP
      frame DOM, document identity and lifecycle timing, including any later
      change between detection and diagnostics.
- [ ] Establish each cause before changing adapter behavior or timing; retain
      the historical Sourcepoint-action and Guardian-no-CMP observations separately.
- [ ] Implement and verify only an evidence-backed repair, or document a proved
      site/environment change and the resulting explicit supported behavior.

## Acceptance Criteria [0/3]

- [ ] Each outcome has an evidence-backed explanation; unavailable evidence is
      explicit and neither error category is silently treated as success.
- [ ] Any repair has a live before/after demonstration and meaningful regression
      coverage; no assertion, return path or deadline is weakened to obtain green.
- [ ] Required ordered workspace gates and a full dual-gate live sweep pass for
      the final implementation, with all failures and profile summaries accounted for.

## Scope boundary

No new paid benchmark work. Do not execute 262's lifecycle repair or 271's BBC
adapter work from this plan. Coordinate resulting behavior/evidence back into
262, whose mandatory consecutive named-test sweeps remain unchanged.

## Execution clarification — 2026-09-23

The owner authorized the 272–278 queue after parking 268. Earlier dated unselected-run
boundaries are historical; 271 remains excluded and 259/266 constraints remain binding.
Original tasks and acceptance criteria above are unchanged.

Start from merged main c761c1b1, retaining 262’s reviewed readiness repairs. Classify
consent failures only after proving the ready target and actual document for that
occurrence. Capture top-level and actual CMP-frame DOM at the failed detection/action
boundary; a later snapshot cannot substitute. Keep detected_not_actioned and
consent_no_cmp distinct. The current recovery hint advertises unsupported dom --frames;
verify a supported replacement against the actual CLI. Declare separate finite local-
fixture and Guardian schedules before execution. BBC 271 stays excluded, and no common
cause with 268 is assumed.

## Diagnostic preparation — 2026-09-24

Temporary opt-in instrumentation is prepared on merged main00403995 with no276/277
candidate imported. Historical Sourcepoint action failure and no-CMP observations
remain distinct. Original tasks/AC stay0/3. No product adapter or timing repair,
Firefox capture, closing sweep, PR or merge has run in this preparation phase.

The probe uses each enumerated target's own console on the existing connection,
retaining cached routing forms separately from actual document samples. A selected
Sourcepoint action failure captures its own DOM synchronously in the JS exception
path. Its thrown exception and original product failure classification remain;
longString exception messages and document samples are fetched on the owning
connection with explicit limits. Top-document and no-CMP samples are sequential
in-command boundary observations, not atomic cross-document identity proof.
Missing/truncated samples make attribution incomplete. Instrumentation can perturb
an intermittent failure; a pass is only a current instrumented control.

A finite schedule declares two local diagnostic fixtures (action failure and
unrecognized-frame no-CMP), then at most one original Guardian137 occurrence only
if both native qualifiers are complete. Source, binaries, browser-free routing and
longString checks, failures and exact admission constraints are private under
`.git/ralph-loop/20260923-remaining272-278/iter275/` in the primary repository.
Execution stops before Firefox for fresh independent review/root admission.
The unsupported dom --frames hint remains pending the evidence-backed product
phase; no unsupported watcher-target eval recipe has been substituted.

The first independent capture review withheld admission for two diagnostic gaps:
shared fixture cleanup discarded join failures and the small native fixtures did
not force longString coverage. A scoped repair replaces only275's local fixture
server with cancellable nonblocking I/O and retained accept/worker join results,
including assertion-unwind cleanup. Both local pages now carry inert padding and
a suffix beyond the initial grip prefix; native qualification requires actual
sample/Error-message grips, same-connection complete substring fetches, matching
UTF-16 lengths and parsed boundary data. The same two local arms and at most one
Guardian arm remain unexecuted (zero native occurrences). The new repair packet
is under the existing evidence root's `repair1/`; original reviewed bytes remain.

## Final diagnostic outcome — blocked, 2026-09-24

The separately reviewed finite schedule is now consumed: two local qualifiers
(2/2) and one original Guardian occurrence (1/1). The local action/no-CMP tests
passed in 3.86s/3.69s with actual owning-console longString/substring evidence,
complete UTF-16 payloads and retained successful fixture accept/worker joins.
They intentionally induced different errors and are instrumentation qualification,
not reproductions of either historical Guardian failure. An initial root parser
expected five no-CMP records; it was corrected offline to the actual six including
`decision-no-cmp`, preserving the failure and without another native occurrence.

The original 137 Guardian test passed 1/1 in 4.82s on the pinned diagnostic source.
Navigation returned 200, committed `/europe`, and reported 293ms; ready-target
polling reached live targets in 81ms. Consent returned Sourcepoint `accepted`
through the daemon. All seven diagnostic records were retained. The selected
CMP's own console sampled its actual Sourcepoint document with 11 controls,
including a 266×38 `Accept all` button, and the action evaluation returned that
label successfully. Its target-destroyed event followed the action reply; the
bracketing top-document iframe inventory changed from two to one. These are
current instrumented passing observations, not an explanation of earlier failures.
Cached routing forms and actual document samples remain distinct observations.

Top-page HTML was untruncated, but both top samples had 397 controls against the
256-control cap (`controlsTruncated=true`). The selected CMP sample was
untruncated. All three Guardian longStrings were independently reconstructed
from raw substring replies with matching UTF-16 lengths and complete request
coverage. Raw and normalized diagnostic JSON retain small control-rectangle
number differences between the two representations; document identity,
HTML and labels agree. Do not call this complete historical attribution or an
atomic cross-document snapshot. Diagnostic timing perturbation remains a limit.

Root's full before/after all-attempt process/profile censuses reported no owned
remnants after each arm and preserved the protected desktop identities. Fixture
joins are actual recorded joins; external death is not a return proof. All capture,
parser, compilation and qualification failures remain in the private evidence
root. The original and repaired diagnostic source/binary archives are recoverable;
temporary runtime/test changes are restored to base 00403995 for this documentation-
only checkpoint. No adapter or timing repair, full closing live sweep, 275 completion
PR or merge is claimed. Original tasks and acceptance criteria remain 0/3.

No further capture is currently admitted. Reopening needs a source-backed,
unanswered discriminating hypothesis or a newly attributable failing occurrence
with ready-target proof and actual top/CMP document evidence at the failed boundary,
plus a declared finite schedule and verified remaining authority and budget.
Re-review diagnostic completeness/limits, artifact ownership and the capture packet
before execution. Seek new owner authorization only if the proposed work exceeds
or changes the existing grant or its actual recorded caps; ordinary work already
covered by that grant does not need fresh permission. The consumed 2+1 schedule
does not reset in a new session; another unchanged passing capture cannot
reconstruct the historical DOM.
Both historical outcomes still need separate evidence-backed explanations before
any behavior/timing change. The unsupported `dom --frames` hint remains pending.
The static preflight establishes no supported exact-target CMP-document DOM CLI
workflow: `dom 'iframe' --attrs --all` inspects iframe-element attributes only;
`click --frame` acts on a URL substring, and `eval --frame` takes a debugger stack-
frame actor, not a watcher target. Do not invent a replacement inspection recipe.

Final baseline checkpoint validation: formatting, strict workspace/all-targets
Clippy, then normal parallel workspace tests passed (2538 passed, 0 failed,
419 ignored). All nine xtask checks and Hyalo HYALO005 passed; dogfood explicitly
skipped because the plan has no `dogfood_script`. These checks launched no Firefox
and do not discharge the original live acceptance criteria.

## Prospective scope adaptation — 2026-09-25

Iteration 272 owns current daemon snapshot replacement/removal/readiness
semantics. Reuse that implementation and its reviewed controls after merge;
do not duplicate its experiment here.

This iteration qualifies selection and action among the currently enumerated
CMP targets, including an earlier recognized but nonactionable frame and a
later actionable frame. Establish the supported selection contract with actual
Firefox controls before changing adapter behavior. Preserve native-first
handling and distinct no-CMP, detected-not-actioned and accepted outcomes.

The historical Guardian no-CMP and Sourcepoint action failures remain separate,
unattributed occurrences. Original AC1 is not fulfilled by a generic fix or
passing Guardian control. No further unchanged Guardian retry is required to
reconstruct unavailable DOM.

### Delivery acceptance [0/3]

- [ ] The supported frame-selection/action contract is explicit and exercised
      on actual documents; acceptance requires an observed action and its
      post-action effect, while absence and failed action remain distinguishable.
- [ ] Any behavior repair has meaningful before/after Firefox and regression
      proof. The unsupported dom --frames hint is removed or replaced only by
      a verified supported instruction.
- [ ] Final ordered gates and this iteration's own dual-gate closing sweep
      pass, with ready-target Guardian observations classified honestly and
      historical failures preserved.

The owner’s September25 all-open adaptation request supplies the prospective
delivery scope above. Completion under this scope must say so explicitly and
retain every unfulfilled historical AC; it must not report the original
attribution criterion as passed. Earlier investigation schedules and restrictions
remain historical records, not renewable capture allowances. Use only the
finite controls needed for the new contract and the required closing validation.
