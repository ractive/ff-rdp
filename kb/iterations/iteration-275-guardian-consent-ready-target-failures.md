---
title: "Iteration 275: Consent selection and truthful ready-target outcomes"
type: iteration
date: 2026-09-19
status: done
branch: iter-275/consent-current-20260925
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

### Delivery acceptance [3/3]

- [x] The supported frame-selection/action contract is explicit and exercised
      on actual documents; acceptance requires an observed action and its
      post-action effect, while absence and failed action remain distinguishable.
- [x] Any behavior repair has meaningful before/after Firefox and regression
      proof. The unsupported dom --frames hint is removed or replaced only by
      a verified supported instruction.
- [x] Final ordered gates and this iteration's own dual-gate closing sweep
      pass, with ready-target Guardian observations classified honestly and
      historical failures preserved.

The owner’s September25 all-open adaptation request supplies the prospective
delivery scope above. Completion under this scope must say so explicitly and
retain every unfulfilled historical AC; it must not report the original
attribution criterion as passed. Earlier investigation schedules and restrictions
remain historical records, not renewable capture allowances. Use only the
finite controls needed for the new contract and the required closing validation.

## Selection qualification preparation — 2026-09-25

The prospective design passed independent review after clarifying that both native
and iframe expressions distinguish a known pre-action miss from an unexpected
exception or malformed result. Native-first ordering/recognition/visibility stay;
unconfirmed action must stop the pass, without claiming the banner remains visible.
No behavior repair is installed in this preparation checkpoint: consent.rs remains
byte-identical to merged main86110575. Original tasks/AC0/3 and delivery0/3 remain.

Three actual-CLI browser-free controls against the old implementation failed0/3
as expected: an earlier recognized missing-control frame, an earlier frame with
no console, and an all-missing-controls set each prevented evaluating the later
recognized frame. Request traces and actual exits are retained. These scripted
responses do not execute JavaScript or explain either historical Guardian failure.

The prepared finite native schedule has two before occurrences, one per actual
direct/daemon route, then ten post-repair document actions across five local
cases per route. It requires the consent invocation's actual returned target
order, the selected document's own synchronous fixture boundary records, and
click counters plus post-action DOM effects. Same-origin local frames keep
boundary/readback evidence synchronous; installed Firefox source explicitly
visits nested same-process browsing contexts. This is controlled selection/action
qualification, not cross-origin-policy or historical-site proof. The existing
272 enumeration behavior is reused without modification.

A scoped fixture helper retains the reviewed cancellable I/O and actual joins,
adding only multi-route dispatch. Native captures require independent tooling
review and root admission. No Firefox occurrence, remote request, whole-workspace
gate, closing sweep, commit or PR ran during preparation. Every failed preparation
result remains private under `.git/ralph-loop/20260924-all-open/iter275/phase1/`
in the primary repository. Its final handoff/freeze records exact builds, checks,
source inputs, binary ownership and unresolved qualification limitations.

## Controlled native before proof and unvalidated draft — 2026-09-25

Root qualified both declared old-source before occurrences: direct1/1 in2.71s
and daemon1/1 in3.83s. Each actual consent command exited1, evaluated only the
first recognized frame, and left the later actual Accept all control present
with positive geometry and zero clicks. The invocation's returned ordering and
resolved route, ready actual document signatures and unchanged post-state were
retained. Each fixture recorded four successful worker joins and one accept
join; the session cleanup returned and its exact profile/private home were gone.
Root censuses conserved224profiles through both occurrences, with protected
desktop identities unchanged. Native Firefox parent wait/birth was not retained;
validated product receipts and cleanup plus post-census are the available proof.
No daemon worker return or historical Guardian cause is inferred. Both before
slots are consumed; there is no further baseline or Guardian retry allowance.

The initial phase1 standalone ff-rdp.d referenced an older checkout despite its
current binary/source assertions. Root preserved that artifact and explicitly
invalidated only main.rs mtime, then rebuilt with all577input bytes unchanged.
The fresh=false build and runtime attestation under `iter275/pre-native-build/`
pin the actual native CLI SHA256
`1e05f1ff713d82997096c7a9ee3fbe51f361f13163edbd809a438d700c319590`.
Both native occurrences used that CLI and the independently reviewed frozen live
binary. The old phase1 freeze remains historical and must not be presented as
attestation of the executed native CLI.

The draft now continues after known pre-action misses, applies explicit Boolean
outcomes to both native and iframe adapters, and stops after success or ambiguous
evaluation. The compatible unsuccessful status now says acceptance could not be
confirmed; the unsupported dom --frames hint is replaced by verified click --frame
wording. Recognition tables, native-first order, visibility checks and enumeration
remain unchanged. The permanent live tests no longer read a before-phase switch:
only the five after cases per route remain. Actual-CLI controls cover sixteen
scripted selections, including native/iframe exception, malformed-result and
transport stopping, first-success stopping, absence and --allow-no-cmp distinctions.

This is an unformatted, uncompiled source checkpoint, not a passing candidate.
No Cargo, rustc, test, Firefox or remote operation ran during this draft phase,
while root owned276closing2. Focused proof, integration with the final merged
base, independent review, the ten after actions and own closing validation remain
pending. Original tasks/AC0/3 and prospective delivery0/3 are unchanged.

## Integrated focused proof — 2026-09-25

Root preserved the original275checkout/draft and integrated its owned changes
onto reviewed276PR274headf4c098b49092d832a834b14ebbc714e43b3fcc87, pending
merge at integration time. The e2e entrypoint retains both276readiness_document
and275consent_selection; no276source was replaced.

After formatting, one focused actual-CLI invocation passed16/16e2e tests: fifteen
selection tests contain sixteen scripted command scenarios, plus one multi-route
fixture/join test. They cover the approved successful, absent, known-miss and
ambiguous-action outcomes, exact actor ordering, terminal stopping and transport
propagation. Existing consent contract tests passed20/20 in one invocation.
Neither invocation ran Firefox, and neither substitutes for native after proof.
The integrated native/e2e targets were compiled for the forthcoming reviewed
capture. Compiler-artifact receipts identify fresh=false CLI/e2e producers in
this actual checkout after an explicit main.rs mtime-only invalidation; standalone
.d metadata alone is not used as binary provenance.

No focused failure occurred in this integration phase. Earlier failures and both
qualified native-before records remain intact. No whole-workspace/strict-Clippy
sequence, native after capture, closing sweep or remote operation ran here.
The final merged-base ordered gates remain root-supervised and are intentionally
not repeated ahead of integration. Exact source/binary freeze, command receipts
and the two-arm after-only runner are private under
`.git/ralph-loop/20260924-all-open/iter275/focused1/` in the primary repository.
Original tasks/AC0/3 and delivery0/3 remain unchanged pending full qualification.

## Native after evidence and returned-order oracle repair — 2026-09-25

The reviewed focused1 candidate ran both declared native after arms. Direct
passed all five cases in7.85s, including actual traversal of a recognized miss
before accepting the later frame, one observed click and same-document removal.
Its fixture recorded16successful worker joins and an accept join. The daemon
arm stopped after its first case in3.81s: Firefox returned the actionable
`sourcepoint-second` document before `sourcepoint-first`. Consent correctly
selected that first returned actionable document, exited0 and produced one
click/removal. The test incorrectly required DOM iframe order, so its overall
exit101 and failed qualification remain preserved. This observation proves
first-selected action only; four daemon cases were unexecuted. Its unwind
recorded4successful fixture worker joins and an accept join. Root's censuses
conserved238profiles for both arms, with exact owned profile/private-home removal
and protected desktop identities unchanged. No detached Firefox parent wait,
native birth or daemon worker-return proof is inferred.

Both original after slots are consumed. A separately reviewed oracle repair
design passed independent review with zero findings. It derives expectations
only from the actual consent command's one returned target vector and independently
qualified pre-action fixture controls. The fixed documents' names are not order
guarantees. The draft parser requires complete known fixture target records and
matching invocation/route; the validator retains exact traversal, first-success
stopping, native priority, distinct absence/failure/success, action counters and
removal, and strengthens unchanged-document checks for unselected final documents.
Product selection, 272 enumeration, fixture HTML/scripts, I/O, commands, lifecycle
and timing remain unchanged.

The finite repair validation is ten handwritten positive cases across both returned
orders, twelve invalid observations, seven invalid traces, and one offline replay
of the six existing raw after records. Replay must retain all five direct meanings
and classify the sole daemon action as first-selected success without changing its
failed arm verdict. Subject to independent implementation review and root admission,
one newly charged daemon five-case matrix may run. There is no direct, before or
Guardian rerun, and no retry to obtain a preferred order. A successful daemon
AllMiss case proves continuation under either order; direct positive continuation,
the before failures on both routes, shared selection implementation and existing
actual-CLI regression controls supply the reviewed prospective evidence composition.
Daemon positive-later success remains explicitly unobserved unless the new occurrence
naturally demonstrates it.

This source-only draft is on merged main1df6c864127a7ed90cabd6c6a258fba715712978.
No formatting, compilation, test, offline replay or new Firefox action ran during
this phase. Its implementation and replay remain unvalidated; final ordered gates,
own dual-gate closing validation and publication are still owed. Original tasks/AC
and prospective delivery remain0/3. Historical Guardian causes are unchanged.
The design/review, source freeze and separate expenditure are private under
`.git/ralph-loop/20260924-all-open/iter275/order-repair/`; all earlier freezes,
producer attestations, failures and cleanup receipts remain intact.

## Returned-order oracle focused proof — 2026-09-25

Formatting passed. The first filtered oracle invocation reused a stale shared-target
e2e executable (`fresh=true`) and ran zero matching tests; exit0 was rejected as
qualification, and its logs/receipt remain preserved. After byte-preserving e2e
entrypoint timestamp invalidation, a fresh compiler producer ran all four oracle
tests: ten literal positive cases, twelve rejected invalid observations and seven
rejected invalid traces passed. The single separately selected offline replay then
passed all six retained raw observations. All five direct outcomes and positive
later continuation remain unchanged. The daemon observation is only first-selected
action; its original failed qualification/exit101 and four unexecuted cases remain
unchanged. No capture was repeated to obtain a preferred order.

The live target compiled once with a fresh producer after its own byte-preserving
entrypoint invalidation. The CLI was similarly rebuilt explicitly because root had
restored a frozen executable into the shared target. Compiler-artifact receipts,
source and binary pins distinguish actual producers from stale standalone .d
metadata. Only formatting changed the draft Rust bytes; no semantic repair was
needed in this focused phase. Original16actual-CLI/fixture and20consent contract
tests were not repeated. No Firefox, full workspace sequence, closing sweep,
commit or remote operation ran. Source/binaries and all results are preserved
under `iter275/order-repair/focused1/` in the existing private evidence root.
Independent implementation/capture review and root admission remain required
before the separately proposed single daemon matrix. Original tasks/AC and
prospective delivery remain0/3; no historical cause or completion is claimed.

## Qualified daemon matrix after oracle repair — 2026-09-25

Fresh independent implementation/capture review accepted the exact returned-order
oracle with zero findings. Root then admitted exactly one new daemon five-case
matrix on the frozen merged-main candidate; no direct, before or Guardian capture
was repeated. The test passed in11.80s, and its actual owned test-child wait was0.

The Later case naturally returned the actionable second-named frame first and
selected only it. This is first-selected success, not daemon positive-later
continuation. The First case selected its first returned actionable document;
AllMiss traversed both returned recognized documents and remained
`detected_not_actioned`; NoCMP remained `no_cmp_detected`; Native selected only
the native BBC fixture control. Each accepted case retained one observed click
and same-document removal, with no action in unselected documents. This local
BBC-named fixture is native-adapter coverage, not a remote BBC-site observation.
Direct positive-later continuation, both actual old-source failures, the daemon
AllMiss traversal and shared actual-CLI contract controls supply the independently
reviewed prospective evidence composition. Missing positive-later daemon evidence
remains explicit; there is no retry to obtain a preferred order.

All16fixture workers and the accept thread recorded successful actual joins;
`session.finish()` returned before the completed marker. The exact owned profile
and private home were absent afterward. Root's qualified process/profile censuses
conserved238profiles, found no unexpected workload and preserved protected desktop
identities. All579compiler inputs, three frozen binaries, the unchanged bounded
runner,110Firefox package files and four real managed-profile identities matched
before and after. Detached Firefox actual wait/native birth and daemon worker
return remain unavailable; neither absence nor this test's success invents them.

Evidence is under `iter275/order-repair/daemon-after1/` in the existing private
root, including raw commands, one-use occurrence directory, actual child wait,
qualification and conservation receipts. The original failed daemon arm remains
failed with its four unexecuted cases. The separate new single-matrix grant is
consumed and this answered experiment is stopped. Final ordered workspace gates,
this iteration's own dual-gate closing, closing checks and publication remain
pending; original tasks/AC0/3 and historical Guardian attribution remain unchanged.

## First closing sweep: initial-tab setup failure — 2026-09-25

After the style-only ordered2621/0/424workspace gates, root's first dual-gate
closing sweep completed350/351passing with zero reported profile leaks. Its sole
failure was `consent_selection_direct` before consent: the first
`direct-later-navigate` command exited1 because the owned FirefoxPID8924 exposed
zero debuggable tabs. The actual command child8979returned that error; no275
consent action or document-boundary qualification occurred in this failed test.
The occurrence, full sweep, cleanup limits and prior passes remain preserved in
`iter275/closing1/`. Original tasks/AC remain0/3 and closing acceptance is unmet.

Offline source inspection establishes a setup precondition gap, not the cause
of this Firefox occurrence: `IsolatedLiveFirefox::launch` validates ownership,
debugger-port reachability and version, but does not establish that a tab exists.
The older `LiveFirefox` helper explicitly checks that precondition. Managed
`launch` documents a debug-port wait, not tab readiness. Its explicit-profile
path appends debugger preferences without applying the temporary-profile startup
preferences. These differences do not prove delayed startup, locale failure or
any relation to147's separate zero-tab observation.

A test-only, event-attributed initial-tab readiness proposal is recorded under
`iter275/startup-repair/design1/`, using the existing root `listTabs` /
`tabListChanged` contract rather than retrying navigation or changing consent.
It is not implemented or capture-admitted. No Cargo, test, browser, socket,
network or product-source operation ran in this diagnosis phase. Original
ordered source/binaries and every previous native result remain recoverable;
the initial-tab cause and final closing validation remain unresolved.

The initial-tab design passed independent review with zero findings. A code-only
draft now adds a275-specific observer before the daemon/direct branch: one owned
endpoint connection, one absolute10second connect/greeting/read/write bound,
and at most9listTabs requests, with re-queries authorized only by observed
tabListChanged notifications (including an interleaved notification). A valid
nonempty tab-descriptor reply establishes only the setup prerequisite; actual
fixture document and consent assertions remain unchanged. Ten browser-free
controls are prepared with bounded worker I/O and retained actual joins.
No formatting, compilation, socket/test execution or new native capture ran
during this source-only phase. The observer remains unvalidated pending focused
proof and independent implementation/capture review; historical failures and
all original acceptance criteria remain unchanged.

The first focused initial-tab invocation compiled but failed all10browser-free
controls in fixture setup: the nonblocking listener's accepted streams were not
reset to blocking mode, and each worker's first read returned immediately before
receiving an observer request. All10actual joins returned worker panics; the raw
failure, formatted source and exact producer binaries are retained. A one-line
fixture correction now explicitly sets the accepted stream to blocking mode,
matching the existing working protocol fixture. CLI/e2e/live targets compile on
the corrected source, but no second ten-case invocation or native run has occurred.
The correction is unqualified pending fresh scoped review and a renewed focused
test grant; it changes no observer/product deadline, protocol or consent behavior.
Evidence is under `iter275/startup-repair/focused1/`; original criteria remain
untouched and no startup cause or completion is claimed.

## Initial-tab qualification and final ordered gates — 2026-09-25

Fresh scoped review accepted the mock blocking-mode correction conditionally,
with zero findings. Root then ran the corrected ten browser-free controls once:
10/10passed and10actual successful worker joins. Their failed first invocation
and exact failed source/binaries remain preserved. The separately admitted single
direct five-case native matrix passed in8.68s, with actual test-child wait0,
16successful fixture worker joins, accept join and completed session cleanup.
The Later case returned first then second and demonstrated a known miss followed
by one action and its same-document effect. The observer's first list reply
contained one tab and completed in82ms with zero notifications: this native run
qualifies immediate descriptor availability, not the delayed event branch or the
cause of the earlier zero-tab occurrence. Event branches retain browser-free proof.
Root's full census conserved281profiles, with no unexpected workload or errors;
580source inputs, three binaries and110Firefox package pins remained unchanged.
The single direct schedule is consumed. The earlier qualified daemon five-case
matrix is reused with its positive-later limitation intact; no daemon, baseline,
Guardian or preferred-order retry ran.

The final ordered block reused the matching same-day stable rustc1.98.1 update.
Initial strict Clippy found two style issues in the new observer helper (similar
variable names and an unnecessary owned record argument). Their failed result
is retained. A variable rename and borrowed JSON record arguments address them
without changing protocol, deadlines, state transitions, observations or consent.
Formatting, strict workspace/all-targets Clippy, then one normal parallel workspace
run passed:2631passed,0failed,424ignored. The separately observed exact-filtered
child-fd fixture's one pass is excluded from that top-level count; it is not blindly
subtracted. No separate focused or native check was repeated for the style changes.

All580compiler inputs and12binaries, including all six closing tiers and xtask,
are frozen under `iter275/ordered3/` in the existing private evidence root, with
actual fresh=false compiler producers identifying this checkout. Content-preserving
entrypoint invalidations, command waits, allowlisted environment, the style-only
delta and every failure are retained. Native product/fixture/oracle/commands/cleanup
are unchanged. No new closing sweep, commit, PR or remote operation ran in this
block. Original tasks/AC0/3 and prospective delivery0/3 remain pending this
iteration's own successful closing validation; historical causes remain unknown.

## Second closing sweep — 2026-09-25

The own closing sweep failed:341passed/10rawfailed across351names. Six failures
were launch timeouts, leaving345executed and4executed failures. The remaining
failures were138navigation timeout,164occupied-port rejection, and both275
initial-tab prerequisites. Neither275case reached consent: one initial empty
listTabs reply, zero notifications, then the unchanged10second deadline. These
are separate occurrences, not evidence of a shared cause or consent regression.
The installed Firefox tab-list implementation permits an initial-window notification
coverage gap, but the captured packets do not prove that gap caused either failure.
No unchanged third sweep or blind tab/navigation retry is admitted.

All580runtime inputs,110Firefox package pins and four real managed profiles
were unchanged. Eight actual sweep binaries are frozen. The345launch records
paired,285baseline profiles were conserved, and ten added retained profiles were
attributed; leaked=0/unattributed=0. The owned raw browser actual wait was-15,
and final native census found no unexpected workload. Detached process absence
is not worker-return proof. Evidence and independent startup audit are private
under `iter275/closing2/` in the existing all-open run root. Original tasks/AC
and prospective delivery remain0/3; all previous failures stay failed.

## Initial-descriptor observation policy draft — 2026-09-25

The installed-source notification coverage gap supports a narrow test-setup
policy correction: successful empty descriptor snapshots may be observed again
at fixed observer-start+0..8second slots within the original10second absolute
budget. Connection/greeting time remains included, missed slots are skipped,
and at most9requests share one connection with one outstanding reply. Typed
notifications remain recorded but cannot accelerate observations. EOF, protocol
and malformed-data errors, invalid descriptors and deadline expiry remain
terminal; no failed request is replayed. This establishes only descriptor
availability, not actual-document readiness or the historical failures' cause.

The code-only draft changes the275initial-tab helper and its actual-loopback
controls. The former no-notification/no-requery assertion is preserved in the
before archive and replaced by a peer whose later descriptor becomes available
without any notification. Other controls retain finite cap/deadline and error
failures, pending-request serialization, real notification observations, slow
greeting/reply slot skipping, partial framing, and actual worker joins. Existing
consent product, fixture, oracle, native assertions and lifecycle are unchanged;
their valid prior evidence remains applicable with its recorded limits.

Before/after580input freezes, the old policy/tests, exact proposed focused list
and handoff are private under `iter275/startup-repair/notification-gap-repair1/`
in the existing all-open evidence root. No formatting, compilation, tests,
socket/native execution, capture or full sweep ran in this source-only phase.
Independent design/implementation review and root admission remain required
before execution. Original tasks/AC and prospective delivery remain0/3; closing2
and both empty-tab failures remain failed, with no worker-return or historical
causal attribution inferred.

Fresh independent review accepted the observation design/helper and requested
one test correction: an80msquiet interval measured from the peer's reply could
reject a valid next absolute slot only50msaway. That was a static counterexample,
not an executed test failure. Both notification-order controls now retain the
observer's exact monotonic start and actual peer request-decode timestamps, and
compare the second request against its absolute scheduled slot. No cooldown,
observer policy, deadline or native assertion changes. Receive completion may
lag kernel arrival due to peer scheduling/decoding; that proof limit is explicit.
The before candidate, review and corrected580input freeze are retained under
`iter275/startup-repair/notification-gap-repair2/`. All21controls remain unexecuted;
fresh scoped review is required before root's proposed focused qualification.

### September 25 initial-tab observation repair — focused qualification

The revised helper observes readiness on fixed slots within the original10s
absolute bound, at most9 successful root-list observations, one connection and
one outstanding request. Only valid empty replies permit another scheduled
observation; EOF, protocol errors and malformed replies remain terminal. It
does not reconnect, repeat failed operations, create tabs or change Firefox
lifecycle. Notifications are retained without accelerating the schedule.

Fresh independent review accepted the helper and corrected absolute-slot
controls with zero remaining findings. One focused invocation passed all21
controls and recorded21 actual successful worker joins. Formatting changed
only layout, current-checkout fresh compiler producers were verified and all580
source inputs remained pinned. No native capture or third closing sweep ran.
The two historical empty-list/no-notification failures remain unexplained; the
source-backed startup notification coverage gap is not claimed as their proven
cause. All original and prospective criteria remain0/3, and earlier failed
results remain preserved in the private iteration275 archive.

## Current-main integration and pending qualification — 2026-09-25

The reviewed nine-path candidate is now integrated into a new isolated checkout
on merged main `b9efed9d9bcdd800c513a205572da6a5ca1ddc81`, retaining completed262
and merged272. Merged284 contributes cancellable daemon startup/shutdown and
transport changes; merged281 contributes caller timing observations. Their main
source and the284e2e registration are preserved. The275consent implementation,
fixture, returned-order oracle, initial-tab observer and controls retain their
previous exact bytes; the e2e registry combines both modules. No product semantic
change was needed for integration. The old checkout and its nine dirty/new paths
remain unchanged and archived. The branch field now names the new checkout.

Prior evidence retains its original scope: two native before failures,16actual-CLI
controls and20unit passes; four oracle tests plus six-record offline replay;
qualified five-case direct and daemon matrices with actual returned-order
classification; and the revised observer's21controls/21actual successful joins.
The daemon positive-later case remains unobserved. Neither those proofs nor the
older ordered2631/0/424result attest the new integrated binary graph. The two
failed closing sweeps remain failed; historical Guardian and initial-tab causes
remain unknown. Original tasks/AC and prospective delivery stay0/3.

Preparation is private under `iter275/current-integration1/` in the all-open run
root. No build, test, Firefox action, closing sweep, commit or publication ran in
this integration phase. The disabled native packet retains the reviewed runner,
240souter/5sowned reap bounds and exactly one original daemon five-case test,
followed by at most one original direct five-case test only after daemon result,
inputs, cleanup and full profile conservation qualify. No prior capture allowance
is reset and no unchanged Guardian, remote BBC or native159occurrence is added.

Fresh scoped integration review must assess284's changed lifecycle/transport
dependencies and281's caller observations against275's unchanged contracts.
Fresh compiler producers, ordered workspace gates and this iteration's own
successful dual-gate closing remain owed. Before building, root must reconcile
actual main again, including147if it merges first, refresh affected input pins
and review coverage, then acquire exclusive global workload ownership. Existing
passing controls need no blind repetition; changed relevant inputs determine
any additional focused qualification.

## Prospective delivery completed — 2026-09-25

The approved consent-selection delivery is complete. Original tasks and historical
AC remain0/3: neither earlier Guardian outcome is explained by the controls or the
passing Guardian observation in the closing sweep. Independent integration review
accepted the implementation with zero findings. Ordered fmt, strict workspace/
all-targets Clippy and one parallel workspace run passed2687/0/426. The initial
Clippy failure and mechanical Box<Report> correction are preserved; observer
behavior, protocol, bounds and assertions are unchanged. Valid unchanged focused
proofs were reused without separately repeating them.

Both newly admitted original five-case native matrices passed: daemon12.13s and
direct7.56s, each retaining20 actual command waits,16 successful fixture joins,
accept join and completed session cleanup. Both naturally observed a known miss
followed by one later-document action and its same-document effect. This supplies
the previously missing daemon positive-later observation; older evidence keeps
its original limits. Initial descriptor availability was immediate in194ms/50ms,
one request and zero notifications, not a proof of the delayed branch or the old
empty-tab cause.

Closing3 failed349/351 and is invalid for closure. Root concurrently ran a
documentation check without FF_RDP_BIN: the frozen283 xtask implicitly invoked
Cargo from its compiled checkout and replaced the shared CLI. Both275 failures
showed the old consent contract/dom--frames hint; the postcheck caught the CLI
hash change, while587 inputs and seven other binaries matched. Per-spawn
executable hashes are unavailable. All failed results and replaced bytes remain.
An independent audit accepted serializing every static/build/live workload and
pinning FF_RDP_BIN for static gates. One current no-run build restored all eight
original qualified hashes; no product change, wider wait or native/workspace
repetition was needed.

The corrected own dual-gate closing4 passed351/351:340 CLI,1 frame-target,3
registry,3 live61u,2 Firefox and2 watcher-protocol.

```text
LIVE_SWEEP_SUMMARY executed=351 skipped=0 preexisting=0 vanished=0 launch_timeout=0 timed_out=0 total=351
LIVE_SWEEP_PROFILES leaked=0 unattributed=0
```

Actual sweep wait0 and raw Firefox wait-15 are retained. All587 source inputs,
eight actual runtime binaries, twelve frozen workspace artifacts,110 Firefox
package files and four real managed profiles matched.379 baseline profiles were
conserved plus nine attributed additions;341 launch attempts paired, zero
unpaired, no owned survivors and protected browser identities unchanged. External
absence is not a daemon-worker-return claim. An offline root assumption of ten
additions was rejected and corrected to the actual nine without another run.

Evidence is under `.git/ralph-loop/20260924-all-open/iter275/`: current-nonlive1,
both native-qualification matrices, failed closing3, corrected closing4 and
review-runtime-conflict1. Main's later283 contract/test/diagnostic changes and203
documentation do not alter this consent behavior and remain preserved by normal
GitHub integration.271's BBC decision remains separately owned and now has this
selection contract as a prerequisite;268 retains connection attribution.203 is a
dated watch-disposition snapshot, not an all-open-complete claim.
