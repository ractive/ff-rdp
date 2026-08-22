---
title: "Iteration 182: 235 e2e-tier assertions still report only stderr, so their failure messages ship empty"
type: iteration
date: 2026-08-22
status: planned
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
tags: [iteration, testing, diagnosability, e2e]
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

## Themes

- **A — Give the e2e tier an `output_note`.** The live tier's lives in
  `crates/ff-rdp-cli/tests/common/mod.rs` (iter-179) and formats
  `status=… stdout=… stderr=…`. Mirror it in `tests/e2e/support/`, or hoist the single copy
  somewhere both tiers can reach.
- **B — Rewrite the 235 sites and widen the guard.** Adding
  `crates/ff-rdp-cli/tests/e2e` to `scanned_roots` is what makes it stay fixed.

## Tasks

### A. Helper [0/1]
- [ ] The e2e tier has an `output_note` equivalent, or shares the live tier's

### B. Sweep [0/3]
- [ ] Every e2e assertion that reported only `stderr` now reports `stdout` too
- [ ] `iter_179_harness_stdout_evidence.rs` scans `tests/e2e` and passes
- [ ] The guard's `scanned >= 1200` floor is raised or replaced by per-tier counts — it currently
      sits 16% below the actual 1427 (1259 live + 168 core), so a lexer desync that silently
      swallowed up to 16% of invocations would still clear the floor AND the positive control

## Acceptance Criteria [0/2]

- [ ] `unit_179_no_assertion_reports_stderr_without_stdout` covers all three test trees
- [ ] The recorded count of fixed sites is in this plan, measured rather than estimated

## References

- [[iteration-179-live-62-runner-sees-no-network-events]] — the live-tier half, and the scanner
