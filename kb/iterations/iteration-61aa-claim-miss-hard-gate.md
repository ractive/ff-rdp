---
title: "Iteration 61aa: Promote claim-miss to a hard gate"
type: iteration
date: 2026-05-24
status: planned
branch: iter-61aa/claim-miss-hard-gate
depends_on:
  - iteration-61z-discipline-skill-integration
tags:
  - iteration
  - tooling
  - process
  - ralph-loop
first_call_sites: []
dogfood_path: |
  # Replay several recently-merged branches and check the failure rate is
  # acceptable before flipping the gate.
  for iter in 61t 61u 61v 61w 61x 61y; do
    $HOME/.claude/skills/ralph-loop/scripts/run-iteration.sh --replay "iter-$iter"
  done
---

# Iteration 61aa: Promote claim-miss to a hard gate

Carry-over from [[iter-61z-discipline-skill-integration]]. iter-61z added
`claims-vs-code.sh` as an advisory check — it writes a report and logs WARN if
any ❌ rows appear, but doesn't fail Phase 1.

The original iter-61z plan called for promoting claim misses to a hard fail
(writing `iter-N-failed` instead of `iter-N-done`). That was deferred because
the heuristic is conservative and a false fail blocks all merges. Before
flipping the gate we need to:

1. Run the replay against the last ~8 merged iterations and audit how often
   the heuristic produces unactionable misses.
2. Tighten the kebab-token list (`dom-*`, `chrome-*`, `target-*`, …) and the
   verb regex so process-only ACs don't accidentally produce misses.
3. Make `// allow-claim-miss:` annotations easier to apply (currently they
   require the symbol to appear verbatim in the diff).
4. Then promote the WARN to a hard failure in `check_iteration_discipline`.

## Acceptance Criteria [0/3]

- [ ] `test_replay_false_positive_rate`: replay across iter-61t..iter-61y
      classifies fewer than 1 false-❌ per iteration on average (manually
      audited; record findings in this plan before flipping the gate).
- [ ] `test_check_iteration_discipline_claims_gate`: with the gate flipped to
      hard-fail, `check_iteration_discipline` returns non-zero whenever
      `claims-vs-code.sh` reports any unwhitelisted ❌, and the smoke replay
      against iter-61v still exits 1 with the expected ❌ rows.
- [ ] `cargo fmt && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace -q` clean.

## References

- [[iter-61z-discipline-skill-integration]] §A.2
- [[iter-61m-61s-postmortem-loose-ends]] §"Mitigations" #4
