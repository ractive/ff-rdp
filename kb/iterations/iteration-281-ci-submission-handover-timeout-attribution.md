---
type: iteration
status: in-progress
date: 2026-09-24
branch: iter-281/caller-timing-20260925
depends_on: []
first_call_sites: []
dogfood_path: >-
  Inspect retained failed CI and reviewed diagnostic-head evidence; then execute
  a finite, supervisor-scheduled local socket-fixture schedule distinguishing
  side-response consumption, main-channel metadata and caller decision under the
  original400/1000ms deadlines. No native collector or live-Firefox script is required.
tags:
  - iteration
  - carry-over
  - testing
---
# Iteration 281: attribute the CI submission-handover timeout

Planning only. Filed from iteration278 exact-head CI; this plan does not authorize
execution, another local capture, a collector repair or an unchanged CI retry.
The selected272–278 scope is unchanged. No established shared cause with278’s
launch-receipt hazard,262 runtime repairs or273 is claimed.

## Observations

On2026-09-23, macOS CI run35918983412/job107377755017 at head
`dd32a3b42ef3b1bfd85c7785786b83f657b55475` failed
`commands::type_text::tests::unit_262_submission_callers_wait_for_replacement`
at `type_text.rs:997`: `predicate/ok: Err(Timeout("waiting for submission target handover"))`.
The target reported1283passed/1failed/2ignored. The message omitted interruption
and exhausted state; the failing assertion identifies a non-exhausted case, but
both predicate/poll_result (400ms) and predicate/request_ack (1000ms) fit the label.
Neither fixture lateness nor a caller bug was established.

Preserved bounded controls under
`.git/ralph-loop/20260923-remaining272-278/iter278/`:

- `ci-caller-diagnostic1`: one exact invocation passed all21 cases with actual
  caller/deadline/progression and42 actual peer joins. This does not explain CI.
- `ci-caller-parallel1`: consumed INVALID125 after an unsupported native status2
  identity state; zero named-case evidence. No test/worker outcome inferred.
- `ci-caller-parallel2`: consumed INVALID125 after native EPERM for PID77739.
  All21 named cases and42 joins completed; both CI-compatible predicate cases
  passed, but129 of1286 target tests lacked verdicts. The invalid full target
  remains invalid; complete named evidence does not explain the earlier failure.
- Owned status2 collector qualifications and all earlier failures remain separate.
  No further native collector work or local parallel diagnostic is authorized.

Iteration278 adds durable, thread-scoped test diagnostics so a separately admitted
new reviewed CI head can identify a failure in its actual environment. This is a
missing-evidence repair, not a behavioral correction or a reset of consumed slots.
A green CI run is validation only. Publication and the single new-head observation
belong to the supervisor after independent review; this plan does not launch it.

## Tasks [2/3]

- [x] Inspect the retained failed CI and reviewed diagnostic-head result, preserving the exact head, environment, case labels and original deadlines.
- [x] Attribute an observed failing case from actual caller/request/snapshot and peer-return/join evidence, or record the precise missing evidence and obtain a finite distinguishing schedule before further execution.
- [ ] If a mechanism is established, implement and independently review a scoped correction, preserve the original assertions, and validate required gates with all failures retained.

## Acceptance Criteria [0/4]

- [ ] A failing occurrence identifies option, interruption, metadata and exhausted state, the actual deadline, caller outcome and complete available protocol progression; unavailable observations remain explicit.
- [ ] Fixture delivery timing and caller behavior are distinguished with evidence; a pass, socket write, native disappearance or process death is not substituted for consumption or worker return.
- [ ] Every acquired test peer has an actual join outcome before outcome assertions, with panic or missing normal return recorded; original400/1000ms and success/exhaustion/protocol contracts remain intact.
- [ ] Any selected correction has a demonstrated mechanism and independent review, required ordered gates and explicit dispositions for remaining failures; no unexplained green rerun is claimed to resolve the original CI cause.

## Dogfood path

Planning only: review retained logs and the supervisor-admitted diagnostic CI head.
No runnable script or live-Firefox command. Any later experiment requires its own
finite inputs, ownership, actual-wait/cleanup evidence, stop condition and admission.

## Carry-over

| Finding | Disposition |
| --- | --- |
| Original ambiguous CI Timeout | Open here; cause unknown. |
| Two local INVALID125 captures | Preserved; no additional local collector/capture work planned. Their errors do not explain CI. |
| Existing278 receipt/cleanup correction | Separate controlled mechanism with its own evidence; do not weaken or undo it to fit this failure. |

## Current-main adaptation — 2026-09-25
The reviewed diagnostic head 9de0ecdcc5743eaa295527ba313f074263d1484e passed the named fixture in macOS job 107407292892; the retained summary records 2538 passed/0 failed/419 ignored across 37 target summaries. This is validation only; the original dd32 timeout remains unattributed. The old single-head observation is consumed, not a pending invitation to retry it.

Reuse 278's actual peer joins and thread-scoped diagnostics. Extend observations through parsed side-snapshot consumption, main-channel metadata acquisition, replacement decision and timeout conversion. Side write completion alone cannot prove consumption before the deadline.

Declare one finite controlled schedule distinguishing timely replacement and metadata, delayed side delivery, and timely side delivery with delayed metadata. Preserve the original 400/1000ms bounds, success/exhaustion/protocol assertions and real-socket deadline case. A synthetic timeout control establishes classification only. Select a correction only from demonstrated evidence; do not resume native collector work or claim that a passing retry explains the historical CI failure.

This prospective finite local schedule follows the owner’s September25
all-open request and supersedes the planning-only wording above for that
work alone. The old native captures and single-head CI observation remain
consumed; no new native collector schedule or unchanged CI retry is implied.

## Bounded caller controls — 2026-09-25

The admitted local schedule is preserved in
`.git/ralph-loop/20260924-all-open/iter281/schedule.md`, declared before execution
on checkpoint `407ff24945b211ea9b7c62b3da137ae5b210b2d4`. The retained failed
macOS log still identifies only `predicate/ok` at `dd32a3b4`; the reviewed
`9de0ecd` CI pass establishes validation, not a historical cause.

The existing fixture now observes caller-side decoded transport packets, parsed
side snapshots, metadata acquisition/outcome, remaining deadline, replacement
decision and timeout conversion. The transport's `send` observation precedes
the write and is explicitly an attempt; the peer's received request supplies
delivery evidence. A decoded packet alone is not acceptance: the separate
metadata outcome and replacement decision show the caller's use. All collection
is test-only and scoped to the caller thread. Existing peer ownership and actual
joins, including unwind, are reused.

Six real-socket controls use the production submission caller with the unchanged
400ms `predicate/poll_result` and 1000ms `predicate/request_ack` contracts:

| Schedule | Observed caller outcome | Interpretation |
| --- | --- | --- |
| Timely side and metadata | Success at 45.5ms / 641.6ms; parsed replacement, metadata and positive replacement decision | Both real caller paths can complete under their existing budgets. |
| Replacement side reply withheld until caller returns | Timeout at 401.4ms / 1001.5ms; no parsed replacement | This controlled occurrence exhausted while receiving the side reply. |
| Timely replacement side; replacement metadata withheld until caller returns | Timeout at 401.4ms / 1001.3ms; parsed replacement, metadata Timeout, no replacement decision | Timely side delivery/consumption alone cannot establish successful handover. |

All six controls recorded both normal peer returns and twelve actual joins before
assertions. The original 21-case matrix passed with 42 joins and unchanged
success/exhaustion/protocol assertions. The six existing `target_query_` tests,
including the trickled-frame absolute-deadline socket case, also passed.
Private raw logs, command receipts and exact input/binary freeze are under
`.git/ralph-loop/20260924-all-open/iter281/`.

Development failures remain preserved: the first compile rejected formatting
`TargetSnapshot` without Debug (no test ran); the first control assertion matched
`true` in its case label instead of the decision detail. Its two timely controls
completed and the first delayed-side control returned the expected Timeout with
both joins before that diagnostic assertion failed. The assertion was scoped to
the decision payload; the six-case regression then passed. Neither failure is
evidence of a product defect or the historical CI cause. The first strict Clippy
gate rejected six formatting/item-placement issues; they were corrected before
the workspace test gate. Intentional peer withholding also retains the existing
two-second peer ceiling as a failure backstop, without changing caller deadlines.

No unexpected caller mechanism was demonstrated and no behavioral correction was
selected. Historical attribution remains open: the failed CI occurrence lacks
consumption/metadata/decision timing and cannot be reconstructed by these controls.
No original acceptance box is ticked by this synthetic classification. Independent
review, supervisor closure and any publication remain separate from these local
results. No native collector, live Firefox run or historical capture retry occurred.

## Prospective delivery scope — 2026-09-25

The owner's all-open request explicitly authorizes reviewing and adapting plans.
This iteration will deliver durable caller diagnostics and verified classification
controls. Recovering the missing timing from the old CI log is not a deliverable
that another unchanged run can satisfy. No unexplained behavior has been
reproduced, so there is no evidenced product correction to select.

This prospective scope preserves every original acceptance criterion and its
unticked state. Completion must be reported as diagnostic delivery, not resolution
of the historical timeout. The original dd32 occurrence remains unknown; the
passing 9de0ecd head and intentional timeout controls do not explain it. The
original conditional correction task remains unticked because no product
mechanism was established. This scope amendment requires independent review
before completion and does not reset any historical capture allowance.

### Delivery acceptance [0/3]

- [ ] Test-only observations distinguish the actual caller's decoded side reply,
      parsed snapshot, metadata outcome, replacement decision and timeout
      conversion, retaining precise case/deadline and actual peer-join evidence.
- [ ] The finite timely/delayed-side/delayed-metadata controls verify those
      distinctions at the original 400/1000ms bounds; existing success,
      exhaustion, protocol and real-socket deadline regressions remain intact.
- [ ] Independent review, current ordered gates and this diagnostic candidate's
      own dual-gate closing sweep pass, with every failed result retained.

### Historical attribution disposition

No further unchanged capture or retry is planned for the original ambiguous CI
timeout. Reopen attribution on recovered contemporaneous protocol evidence or a
new unexpected failure with the durable diagnostics. At that point retain the
exact head, environment, option/interruption/metadata/exhausted case, deadline,
caller outcome, consumed snapshots and metadata, and all actual peer join
outcomes; declare a finite schedule aimed at the remaining concrete distinction.
This is an evidence-triggered follow-up, not a claim that the old event was fixed
or that future failures may be ignored. The two INVALID125 captures remain
invalid and consumed. Their collector problems stay separate from caller behavior.

## Closing validation and timing carry-over — 2026-09-25

The first own dual-gate sweep failed: 348 passed/1 failed across all349
exactly reconciled names, zero profile leaks or unattributed profiles. The sole
failure was `live_navigate_default_fast::live_navigate_elapsed_matches_wall`:
wall1491ms, reported512ms, gap979ms. It is retained under
`.git/ralph-loop/20260924-all-open/iter281/closing1/`, not reclassified as green.

Same-PID timing records explain the gap quantitatively: connection577.900ms,
connected-to-dispatch340.676ms, dispatch-to-commit minus reported0.918ms,
postcommit0.058ms, call/drop0.042ms, postcore46.542ms, output0.125ms, precall0.048ms
and combined outside-run/rounding12.691ms. The measured predispatch interval alone
exceeds750ms. The public dispatch-to-commit interval is consistent with the
reported512ms;279's final refresh was explicitly skipped. Individual RPC latency,
Firefox processing and descheduling remain indistinguishable within the setup
aggregates. Host load237.42/142.79/73.23 is context, not a proved cause.

One predeclared focused invocation of the unchanged test passed: wall515ms,
reported227ms, gap288ms, actual test exit0. Source, binaries and Firefox package
matched the same frozen candidate; normal host scheduling and original750ms/>5ms
assertions were retained. All169 baseline profiles were conserved, the one new
profile was attributed to the named launch, no unexpected owned process remained,
and the protected desktop was preserved. This actual process wait and native
absence do not establish otherwise unobserved worker returns. The finite focused
schedule is closed; its pass neither explains setup scheduling nor erases the red.

Disposition: no new product correction is justified by this occurrence. No new
plan is filed for unidentified setup scheduling: the measured intervals account
for the failed comparison without an inconsistent public timer or unnecessary
postcommit refresh. Reopen with evidence of incorrect/unnecessary setup or a
timing inconsistency, preserving exact occurrence diagnostics. The unchanged
assertions remain active. One further required closing sweep is admitted after
this diagnosis and focused validation; it is not discovery or a repeat-until-green
schedule. A new failure must receive its own attributable disposition.

## Preserved candidate after final validation — 2026-09-25

Closing2 also failed:346passed/3failed of349, no missing verdicts, no profile
leaks/unattributed profiles, actual sweep exit1. Original159 again reported
daemon1/direct2; passive forms and successive operation windows are retained
for iteration283. The220 slow-destination test failed during its initial
`navigate --with-page`, before the click under test: fresh-navigation readiness
timed out after20001ms. That occurrence has no raw protocol trace, so its cause
and relationship to other navigation plans remain unknown pending source triage.
The timing test failed at wall1158/reported393/gap765ms, with the refresh skipped;
its new stage records remain distinct from closing1's979ms occurrence.

Both failed sweeps and all captured binaries/profiles remain preserved privately.
The ten new profiles in each sweep are attributed and all baseline profile
identities/markers conserved; owned raw browsers were actually waited, protected
desktop identities unchanged. No worker return is inferred from external process
absence. No third unchanged sweep is scheduled.

This candidate is not complete or published: delivery acceptance remains0/3
pending successful closing validation, original AC0/4 remains unchanged. The
supervisor preserves the reviewed diagnostic source and proceeds to the separate
frame defect/timeout investigation. Resumption must integrate any merged repairs
and validate the resulting candidate; no old failure is erased by a later pass.

## Scoped timing diagnostic qualification — 2026-09-25

The independently accepted `timing-diagnostic-design1` patch was applied against
all four verified working-file bases, preserving the retained caller diagnostics.
It adds twenty successful-path timing records across connection routing,
listTabs, target attachment, predispatch setup and postcore connection metadata.
The existing ten core/run records remain. Nested scopes have independent origins;
setup shares the core origin and postcore shares the run origin. Differences
include logging, intervening work and descheduling; they are aggregate intervals,
not pure Firefox or RPC durations. Missing terminal markers do not prove return
or cause. No protocol operation, request ordering, public timer, deadline or
ownership behavior changes. The original750ms and >5ms predicates are unchanged;
the native assertion text now requests attribution instead of presuming an
internal-timer regression.

The exact existing actual-CLI/mock regression ran once and passed1/1, checking
same-child-PID counts, order and monotonic offsets for all seven scopes, with
actual child wait and peer join before outcome assertions. Its passing path does
not print the raw child PID or trace payload into the outer log; those values
were checked internally, not independently recovered from external absence.
The required ordered fmt, strict workspace/all-target Clippy and normal parallel
workspace tests passed:2602outer passed/0failed/421ignored, plus one separately
reported successful nested child test. Same-day stable-update evidence and
installed versions were verified. Fresh producer records identify this checkout
and rebuilt executable paths; source and binaries are frozen privately under
`.git/ralph-loop/20260924-all-open/iter281/timing-diagnostic-implementation1/`.
The focused case also participates normally in the required workspace suite;
there was no second standalone focused invocation.

This is diagnostic qualification, not explanation of any historical failure or
successful closing validation. Original AC0/4 and delivery acceptance0/3 remain
untouched. Both failed281 sweeps,283's timing occurrence and all consumed captures
remain preserved. Baseline consolidation remains unimplemented because common
RPC failure and LongString semantics require a separate substantive contract.
The finished batch still needs independent review. At most one original native
timing case remains conditional on supervisor admission after workload release;
no native/Firefox run, full sweep, commit, push or PR ran in this batch.

### September 25 focused phase diagnostic — completed once

The independently reviewed diagnostic batch passed its exact CLI/mock control
once, then ordered formatting, strict workspace Clippy and normal workspace
tests:2602 outer passed,0 failed,421 ignored, plus one separately identified
nested child pass. All575 runtime inputs and12 actual fresh producer binaries
were frozen. An initially stale top-level dep-info file was rejected and
replaced as evidence by the byte-identical actual hashed CLI binding; source
and gates were not rerun for this archival correction.

Exactly one original native timing case then passed:428ms wall,230ms reported,
198ms gap, with the original750ms and>5ms assertions unchanged. The30 requested
records covered all seven timing scopes. One additional preexisting279
postcommit-refresh/skipped marker was also present, on the same PID and inside
the core commit/return interval. The first offline parser incorrectly rejected
that known extra marker; its output and parser were preserved, and a strict
31-record correction passed fresh independent evidence review. No second
capture occurred.

This occurrence spent about95.8ms connecting/acquiring the target and51.7ms
in subsequent pre-dispatch setup; target acquisition was68.2ms within that
connection interval. Postcore metadata took38.8ms. Nested intervals are not
added to their parents, and completed calls include local work and scheduling.
The earlier slow occurrences remain unexplained; this pass demonstrates no
product correction or host-load cause. The finite diagnostic capture is spent.

The test process was actually waited with exit0 and no watchdog. All304
baseline private profiles were conserved; one new owned profile was retained
and attributed, with no surviving owned process. Source575, installedFirefox110
and four real managed profile identities remained unchanged. No daemon worker
return is inferred. The private timing-native1 archive retains all raw output,
parser failures, original/final qualifications and independent review.

Original historical acceptance remains0/4 and prospective delivery0/3. The
finished diagnostic candidate still owes its own successful dual-gate closing
validation, static closing gates and exact-head CI before completion/merge.
