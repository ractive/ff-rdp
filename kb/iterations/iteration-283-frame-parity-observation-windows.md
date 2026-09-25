---
title: "Iteration 283: Frame parity across observation windows"
type: iteration
status: planned
date: '2026-09-25'
branch: iter-283/frame-parity-observation-windows
depends_on: ['272']
first_call_sites: []
dogfood_path: >-
  Preserve the original live_159_daemon_watcher_regression::live_159_frame_targets_survive_the_fix
  assertions, operation order and bounds. Before any new owned-Firefox occurrence,
  declare a finite source-backed schedule with independently reviewed attribution,
  exact source/binaries and actual owned-child waits. Stop answered experiments;
  do not use full sweeps for discovery or repeated passes to erase a failure.
---

# Iteration 283: Frame parity across observation windows

## Evidence and scope

Iteration272's closing1 on September24 failed the original159 frame-survival
regression with daemon1/direct2. Its exact source, binaries and complete log are
preserved under the primary checkout's private
`.git/ralph-loop/20260924-all-open/iter272/closing1/`. A later isolated original159
passed2/2. That passing sample does not explain the earlier mismatch.

The original test navigates, queries daemon frames, queries direct frames and
reads daemon status in successive windows. Connection-local actor strings do not
establish cross-route document identity. Passive records now retain target forms,
operation windows and daemon lifecycle messages when available; missing records
remain unknown, not proof of missing targets or worker return.

272 separately corrected largest-historical-snapshot retention. Its production
helper now retains observed removals, equal-count form replacements and readiness
withdrawal. Three meaningful controls fail before that correction and pass after;
a fourth shows that different event cuts can differ while same-cut sets agree.
That fourth control is not proof of the old1/2 cause and does not replace the
unchanged sequential159 regression. The September25 closing2 reports159 `ok`,
but passing libtest stdout is suppressed; no new occurrence-specific full-body
trace or old-cause conclusion is inferred from that verdict.

This plan owns the remaining parity/observation-window question. Original275's
Guardian consent outcomes and279's navigation timing gaps remain separate.

## Tasks [0/3]

- [ ] Map all retained original159 identities and observation windows; enumerate
      unavailable identity/order joins. Distinguish missing evidence from missing
      targets and account for connection-local actor identifiers.
- [ ] Declare a finite source-backed investigation and meaningful production-path
      controls for observed add/remove/update events and differing observation
      windows. Preserve original159 assertions, bounds, command order and every
      old/new failure. Independently review and admit any needed native schedule
      before execution, with actual owned-child waits and explicit unknowns.
- [ ] Apply only a demonstrated correction to the established cause or contract,
      preserving original frame-survival and count-parity coverage. No readiness
      delay, lifecycle hold, widened wait, blind retry or reduced assertion may
      manufacture agreement. Leave the historical cause unknown unless its own
      occurrence evidence establishes it.

## Acceptance Criteria [0/3]

- [ ] The failed1/2 occurrence remains preserved with exact attribution limits;
      the direct/daemon parity contract states identity and observation-window
      requirements, supported by production-path controls detecting real missing
      or stale targets. Unavailable historical joins remain explicit.
- [ ] Any correction has meaningful before/after failure and success and passes
      independent review, with original159 assertions and bounds retained. A later
      passing sample alone does not close an unexplained behavioral claim; state
      exactly which cause or contract question the work actually resolves.
- [ ] For product/test source changes, ordered gates and this iteration's own
      dual-gate closing pass with exact names and profile reconciliation. Audit
      prose does not replace original required behavioral coverage.

## Out of scope

Reconstructing unavailable historical packets as facts; treating differing actor
strings as distinct documents without connection context; relaxing original159;
absorbing original275 consent or279 timing criteria; interpreting process death
as worker return; declaring272 complete without its own successful closing.

## Carry-over provenance

Filed from272's independently reviewed carry-over proposal. The private review
`iter272/review-carryover/report.md` accepts this separate scope with zero findings;
it does not approve a future test-contract change, native capture or completion.
272 still has unmet closing acceptance after its latest348/349 failed sweep.
