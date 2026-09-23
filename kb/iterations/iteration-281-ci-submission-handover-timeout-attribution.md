---
type: iteration
status: planned
date: 2026-09-24
branch: iter-281/ci-submission-handover-timeout-attribution
depends_on: []
first_call_sites: []
dogfood_path: "Planning only: retained failed CI and one supervisor-admitted reviewed diagnostic-head observation; no live script or execution authorized by this plan."
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

## Tasks [0/3]

- [ ] Inspect the retained failed CI and reviewed diagnostic-head result, preserving the exact head, environment, case labels and original deadlines.
- [ ] Attribute an observed failing case from actual caller/request/snapshot and peer-return/join evidence, or record the precise missing evidence and obtain a finite distinguishing schedule before further execution.
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
