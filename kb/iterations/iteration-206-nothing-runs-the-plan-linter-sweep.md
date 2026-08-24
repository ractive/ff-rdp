---
title: "Iteration 206: the plan-linter sweep is green for the first time and nothing runs it"
type: iteration
date: 2026-08-24
status: planned
branch: iter-206/enforce-plan-linter-sweep
depends_on: [195]
first_call_sites: []
dogfood_path: |
  # 1. The sweep passes today, on every plan in the tree:
  cargo build -q -p xtask
  for p in kb/iterations/iteration-*.md; do
    ./target/debug/xtask check-iteration-plan "$p" >/dev/null 2>&1 || echo "FAILED: $p"
  done
  #    expected: no output (true since iteration 195)

  # 2. Nothing runs it. Grep the workflows and the xtask gates for a caller:
  grep -rn "check-iteration-plan" .github/workflows/ || echo "NO CI CALLER"
  #    expected TODAY: NO CI CALLER
  #    expected AFTER: a workflow step that runs the sweep

  # 3. Prove the enforcement bites. Plant a plan that violates the requirement
  #    and confirm whatever runs the sweep goes red, then remove it:
  printf -- '---\ntitle: "x"\ntype: iteration\nstatus: planned\n---\n\n# x\n' \
    > kb/iterations/iteration-998-deliberately-invalid.md
  for p in kb/iterations/iteration-*.md; do
    ./target/debug/xtask check-iteration-plan "$p" >/dev/null 2>&1 || echo "FAILED: $p"
  done
  #    expected: FAILED: kb/iterations/iteration-998-deliberately-invalid.md
  rm -f kb/iterations/iteration-998-deliberately-invalid.md
tags: [iteration, tooling, ci, process, carry-over]
---

# Iteration 206: a green sweep that nobody runs

## Where this came from

Carry-over from iteration 195. 195's own reasoning for taking the 82 pre-requirement plans
to zero failures was that a green sweep "would let CI run the linter over the directory,
which is the only way any of this gets enforced without a human remembering". 195 delivered
the green sweep and deliberately stopped there — wiring CI was not among its tasks or
acceptance criteria, and adding it unasked would have been scope creep on an iteration whose
whole subject was making a deliberate disposition rather than a reflexive one.

So the enforcement half is unfiled work, and this is it.

## The point

`check-iteration-plan` is required by CLAUDE.md before filing a plan. "Required by a document"
means "runs when an agent remembers", and iteration 162a deleted the gates that used to try to
remember on the agent's behalf. The sweep is different from those deleted gates in one way that
matters: it is a check that already exists, run over more inputs. It adds no new command, no
new step for a contributor, and no new judgement call — it either exits 0 or names a file.

## The decision this iteration has to make

- **Where it runs.** A step in an existing CI workflow, versus the weekly `toolchain-watch`
  canary, versus nothing (accept that it is a manual sweep and say so).
- **What it runs.** A shell loop in the workflow — which is Bash-only and this repo builds on
  Windows — versus teaching `check-iteration-plan` to accept a directory path. The latter is
  not a new subcommand, but it is new behaviour on an existing one and needs its own
  justification.
- **What it costs when it fails.** A plan-linting failure blocking an unrelated code PR is a
  real cost. Decide whether this is a blocking check or a separate advisory lane — and note
  `kb/decision-log.md`'s record that advisory PR lanes and the ralph-loop do not mix.

## Tasks

### A. Decide and record [0/2]
- [ ] A written disposition covering where it runs, what it runs, and blocking vs advisory
- [ ] `CONTRIBUTING.md`'s "Running it over the whole directory" section says what now runs it

### B. Wire it [0/2]
- [ ] The sweep runs automatically, on a trigger named in the disposition
- [ ] A deliberately invalid plan makes it red, demonstrated in the PR

## Acceptance Criteria [0/3]

- [ ] `grep -rn check-iteration-plan .github/workflows/` names a caller
- [ ] A planted invalid plan fails whatever runs the sweep, shown with output
- [ ] No new xtask subcommand (iteration 162a's decision still stands)

## Out of scope

- **Re-opening the grandfather disposition.** Settled in iteration 195, DEC-047.
- **Any new discipline gate.** This runs an existing required check over more files; if the
  design drifts towards a new gate, that is the signal to stop and reconsider.

## References

- `kb/decision-log.md` DEC-047 — the sweep's disposition and why zero was worth reaching
- `kb/discipline-rationale.md` — why iteration 162a deleted the previous gates
