---
title: "Iteration 276: cascade external-CSS fixture missing during live sweep"
type: iteration
date: 2026-09-19
status: planned
branch: iter-276/cascade-external-css-missing-fixture
depends_on: []
first_call_sites: []
dogfood_path: |
  FF_RDP_LIVE_TESTS=1 FF_RDP_LIVE_NETWORK_TESTS=1 cargo test -p ff-rdp-cli --test live live_cascade::live_cascade_returns_matched_rules_external_css -- --include-ignored --exact --nocapture --test-threads=1
  # Capture failing URL/document/DOM/route before teardown; an isolated pass is only a control.
tags: [iteration, carry-over, cascade, testing]
---

# Iteration 276: external-CSS cascade fixture not found

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

## Tasks [0/3]

- [ ] Retain the failed occurrence and exact isolated control without treating a
      later pass as a repair.
- [ ] Capture URL/document/DOM and route at a failing occurrence in a bounded
      reproduction or independently required sweep; identify the mechanism.
- [ ] Repair only the demonstrated cause with meaningful regression coverage and
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
