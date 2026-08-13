---
branch: iter-157/live-sweep-classifier-drift
date: 2026-08-13
depends_on:
  - kb/iterations/iteration-155-live-skip-reports-green.md
dogfood_path: |
  # A live test whose #[ignore] reason disagrees with the env vars its body reads
  # must be reported, not silently classified:
  cargo run -p xtask -- live-sweep --check-classification
  # → exit 0 today (no drift), exit 1 naming the test if a reason and body diverge
  # The executed-count claim must still hold with no gates set:
  cargo run -p xtask -- live-sweep | grep LIVE_SWEEP_SUMMARY
  # → LIVE_SWEEP_SUMMARY executed=0 skipped=219 total=219
first_call_sites: []
status: obsolete
title: "Iteration 157: live-sweep's classifier trusts ignore-reason prose that nothing keeps in sync with the test body"
type: iteration
tags:
  - testing
  - live-tests
---

# Iteration 157: close DEC-031's named residual

> **CLOSED OBSOLETE 2026-08-13.** Its own AC said "measure first" — so it was measured. The
> first full qualified `live-sweep` run in the project's history
> ([[analysis-2026-08-13-what-ff-rdp-became]] §6: `executed=197 skipped=25 total=222`, 49.5 min,
> 190 passed / 7 failed) shows **classifier drift is not what is wrong with the sweep**. The two
> real ways a test is counted `executed` without testing anything are (a) Firefox failed to
> launch, which returns a silent `ok` from 167 harness call sites, and (b) the `ff-rdp-core`
> live tests needing a hand-started Firefox on port 6000. Both live in the test harness, not in
> the `#[ignore]`-reason classifier. (a) is addressed by
> [[iteration-158-launch-lifecycle-and-harness-honesty]]; (b) is an in-scope decision there too.

Filed out of [[iteration-155-live-skip-reports-green]] / [[decision-log#DEC-031]], which names
this gap explicitly rather than leaving it implicit.

## The defect

`xtask live-sweep` classifies every `#[ignore]`-gated live test by reading its **ignore-reason
text** and looking for `FF_RDP_LIVE_TESTS` / `FF_RDP_LIVE_NETWORK_TESTS`. The actual runtime gate
is the `if std::env::var("…").is_err() { return; }` inside each test body. Nothing keeps the two
in sync.

For a test classified **unqualified** this is harmless — the body never runs, so its early
`return` is unreachable, which is exactly why DEC-031 left the ~94 call sites untouched. The
reachable case is the other direction: a test whose reason text mentions only
`FF_RDP_LIVE_TESTS` while its body *also* checks `FF_RDP_LIVE_NETWORK_TESTS` is classified
qualified on a `FF_RDP_LIVE_TESTS=1` run, executes, hits the bare `return`, and reports `ok` —
**the original iter-155 defect, intact, inside the tool built to fix it.**

DEC-031 records that an iter-155 audit found every current reason under `tests/live/` names at
least one env var. That establishes coverage at one point in time; it does not establish
agreement between reason and body, and nothing enforces either going forward. A contributor
adding an env check to a test body without editing its `#[ignore]` string reintroduces the bug
silently.

## Themes

### Theme A — cross-check reason text against the body's env reads

Extend `live_sweep` (or add `--check-classification`) to parse, per gated test, the set of
`FF_RDP_LIVE*` vars its **body** reads, and compare against the set implied by its
`#[ignore = "…"]` reason. Disagreement in either direction is reported; the body-reads-more case
is the dangerous one and must fail.

Verify on the wire before designing: **measure how many of the current ~219 gated tests already
disagree.** If the answer is zero, this iteration is pure regression-prevention and should say so
in the Resolution rather than implying it fixed live breakage. If it is non-zero, those tests are
reporting fake `ok` today and each one is a finding.

Note the parse is over Rust source, not a running program — `env::var` calls can be indirect
(`live_tests_enabled()` helpers). Decide how deep to follow, and record what the analysis
deliberately cannot see.

### Theme B — wire it so it cannot rot

A check nobody runs is documentation. Decide where this belongs: a `#[test]` in the xtask suite
that runs on every `cargo test --workspace` (cheapest, catches it at PR time), a CI job, or a
`check-iteration-ready` sub-check. Prefer the first unless there is a reason not to.

## Acceptance Criteria [0/3]

- [ ] test_157_reason_body_disagreement_is_detected: a fixture test whose `#[ignore]` reason names
      fewer env vars than its body reads is reported by the classification check with a non-zero
      exit, naming the offending test
- [ ] test_157_current_suite_is_clean_or_findings_filed: the check runs over the real
      `tests/live/` tree; either it reports zero disagreements (recorded in the Resolution with
      the measured count of tests examined), or every disagreement it finds is fixed in this PR
- [ ] check_157_sweep_summary_unchanged: `cargo run -p xtask -- live-sweep` still reports
      `executed=0 skipped=219 total=219` with no env gates set (or the Resolution explains the
      changed totals)

## Notes

- The qualified path of `live-sweep` — actually launching Firefox and running live tests — was
  **never exercised** in iter-155; only `executed=0` was verified. If this iteration touches
  classification, exercising the qualified path at least once (`FF_RDP_LIVE_TESTS=1`) and
  recording the real numbers is the cheapest way to close that gap.
- Not a skill-edit iteration — this one can run through the loop.
