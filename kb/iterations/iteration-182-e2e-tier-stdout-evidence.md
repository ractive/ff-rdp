---
title: "Iteration 182: 235 e2e-tier assertions still report only stderr, so their failure messages ship empty"
type: iteration
date: 2026-08-22
status: done
branch: iter-182/e2e-tier-stdout-evidence
depends_on: []
first_call_sites: []
dogfood_path: |
  # Harness diagnosability defect. Identical in kind to the one iteration 179
  # fixed for the live tier; this is the remaining tier.
  
  # 1. Count what is left. Widen iteration 179's scanner to the e2e tree and
  #    watch it fail — the scanner already exists and already parses balanced
  #    macro invocations, so this is a one-line change plus the fixes.
  #    crates/ff-rdp-cli/tests/iter_179_harness_stdout_evidence.rs  (scanned_roots)
  #    Measured 2026-08-22, re-measured post-merge: 235 offending invocations under tests/e2e/.
  
  # 2. See why it matters. ff-rdp is JSON-on-stdout; its error envelopes go to
  #    STDOUT and stderr is usually empty, so
  #      assert!(out.status.success(), "eval failed: {}", lossy(&out.stderr))
  #    panics with nothing after the colon.
  cargo test -q -p ff-rdp-cli --test iter_179_harness_stdout_evidence
tags:
  - iteration
  - testing
  - diagnosability
  - e2e
---

# Iteration 182: finish the stdout-evidence sweep in the e2e tier

Carry-over from [[iteration-179-live-62-runner-sees-no-network-events]] Theme A, which fixed the
**live** tier (198 invocations across 60 files) and measured, but deliberately did not touch, the
e2e tier.

## Why it was split rather than done in 179

`crates/ff-rdp-cli/tests/e2e/` is the mock-server tier. It has its own `support` module rather
than the live tier's `common`, so it needs its own `output_note` equivalent; and 235 further
mechanical edits on top of 179's 198 would have produced a diff nobody could review. The scanner
that enforces the rule already ships — only its `scanned_roots` needs widening.

### Re-measured after 179 merged — the scope is 235, not 246

Iteration 179's PR body, its Theme A table and its carry-over row 6 all state **246** e2e
offenders, and this plan was filed inheriting that number. Re-measured against merged `main`
(`a294724`) with the same tool that produces the live-tier count — the guard's own `is_offender`
over `panic_invocations` — the real figure is **235 across 39 files**, out of 1247 invocations.
The live-tier numbers from 179 reproduce exactly (198 before, 0 after), so the discrepancy is in
the e2e count alone, not the method.

Corrected here rather than in 179, which is merged. The knock-on is that 179's "444 edits in one
PR" arithmetic should read 433. Recorded so this plan's own completion check does not go hunting
for 11 sites that do not exist.

### Re-measured again during implementation — the actual scope is 236, not 235

Widening `scanned_roots` to include `tests/e2e` and running the guard unmodified (before any
fix) reported **236 offending invocations across 39 files**, one more than the 235 measured above.
The extra site is `a11y.rs:626` — present in both measurements, but the pre-implementation count
in this plan undercounted by one. 39 files matches; invocation count does not. Fixed here rather
than silently rounded, per this plan's own stated reason for correcting 179's number: so a later
reader hunting for a mismatch does not go looking for a defect in the method. The two `use
super::support::{self, ...}` conversions plus five new `use super::support;` imports needed to
make every rewritten call site resolve are additional to the 236 assertion edits themselves.

## Themes

- **A — Give the e2e tier an `output_note`.** The live tier's lives in
  `crates/ff-rdp-cli/tests/common/mod.rs` (iter-179) and formats
  `status=… stdout=… stderr=…`. Mirror it in `tests/e2e/support/`, or hoist the single copy
  somewhere both tiers can reach.
- **B — Rewrite the 235 sites and widen the guard.** Adding
  `crates/ff-rdp-cli/tests/e2e` to `scanned_roots` is what makes it stay fixed.

## Tasks

### A. Helper [1/1]
- [x] The e2e tier has an `output_note` equivalent, or shares the live tier's — added at
      `crates/ff-rdp-cli/tests/e2e/support/mod.rs`, a same-shaped standalone copy (the tiers do
      not share a module), covered by the guard's own `output_note(` exemption

### B. Sweep [3/3]
- [x] Every e2e assertion that reported only `stderr` now reports `stdout` too — 236 sites across
      39 files fixed (204 by mechanical rewrite to `support::output_note(&EXPR)`, 32 by hand where
      a pre-extracted `stderr` string variable was already the assertion's condition)
- [x] `iter_179_harness_stdout_evidence.rs` scans `tests/e2e` and passes
- [x] The guard's `scanned >= 1200` floor is replaced by per-tier floors (live ≥1150, e2e ≥1120,
      core ≥150 — each ~10% below its measured count on this branch: live 1290, e2e 1258, core
      168), so a lexer desync confined to one tree can no longer hide behind another tree's volume

## Acceptance Criteria [2/2]

- [x] `unit_179_no_assertion_reports_stderr_without_stdout` covers all three test trees — `live`,
      `e2e`, `core`; see `MIN_PER_ROOT` in the guard
- [x] The recorded count of fixed sites is in this plan, measured rather than estimated — 236
      across 39 files (see "Re-measured again during implementation" above), not the 235 the plan
      was filed with

## References

- [[iteration-179-live-62-runner-sees-no-network-events]] — the live-tier half, and the scanner
