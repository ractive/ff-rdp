---
title: "Iteration 233: nothing runs the green plan-linter sweep, and one plan is invisible to every hyalo sweep"
type: iteration
date: 2026-08-24
status: planned
branch: iter-233/enforce-plan-linter-sweep
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

  # --- from iteration 234 ---

  # 1. The defect. hyalo cannot read one plan, and says so on stderr — which
  #    every scripted `hyalo find` in this repo discards:
  hyalo find --property type=iteration --format text 2>&1 >/dev/null \
    | grep -o 'skipping [^:]*'
  #    expected TODAY: skipping iterations/iteration-84-dogfood-56-real-real-fixes.md
  #    expected AFTER: no output

  # 2. The cause — a 9086-byte block scalar, over hyalo's ScalarBytes budget:
  hyalo find --file kb/iterations/iteration-84-dogfood-56-real-real-fixes.md 2>&1 | head -8

  # 3. Confirm the plan is otherwise fine, so this is a hyalo-side limit and not
  #    malformed YAML — xtask reads the same frontmatter without complaint:
  cargo run -q -p xtask -- check-iteration-plan \
    kb/iterations/iteration-84-dogfood-56-real-real-fixes.md
  #    expected: check-iteration-plan: OK (today and after)

  # 4. Whatever the fix, this must hold afterwards — a silent skip must not be
  #    able to come back without something failing:
  hyalo find --property type=iteration --format text 2>&1 >/dev/null | grep -c skipping
  #    expected AFTER: 0 (or only "skipping cleanly")
tags: [iteration, tooling, ci, process, carry-over, hyalo]
---

# Iteration 233: nothing runs the green plan-linter sweep, and one plan is invisible to every hyalo sweep

> **Renumbered 206 → 233 on 2026-09-06** so the pending queue runs as one contiguous sweep (DEC-051). Older PRs, commits and sweep logs cite it as iteration 206.

> **Merged 2026-09-06 (DEC-051 addendum):** absorbs [[iteration-234-plan-84-is-invisible-to-hyalo]] as Part B. One branch, one PR, one carry-over sweep for both parts.

## Part A: the plan-linter sweep is green for the first time and nothing runs it

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

## Part B: one iteration plan is invisible to hyalo, and every vault sweep has been silently skipping it (absorbed from iteration 234)

> **Renumbered 205 → 234 on 2026-09-06** so the pending queue runs as one contiguous sweep (DEC-051). Older PRs, commits and sweep logs cite it as iteration 205.

> **Premise check (2026-09-06):** `hyalo 0.22.0 (2026-09-05)` reads `iteration-84` without a skip warning — Tasks A/B and ACs 1–2 are already satisfied by an upstream hyalo change, not by a merged iteration. Only Task C (a written disposition on whether a silent hyalo skip should be detectable) survives; verify with `hyalo find --file kb/iterations/iteration-84-*.md` on arrival and close `obsolete` if that holds, recording the version.

## Where this came from

Carry-over from iteration 195 (`kb/decision-log.md` DEC-047).

195's plan asserted that plans 80, 82 and 83 had frontmatter that "does not parse at all",
and that this made them "invisible to every tool that walks the vault, not just to xtask".
The first half was wrong — their YAML was valid, only xtask's typed view of it failed — and
195 corrected both the files and the misleading error message.

But checking the second half turned up a file the plan had not named. `hyalo` reads 80, 82
and 83 without complaint. It refuses exactly one plan in the tree:

```
warning: skipping iterations/iteration-84-dogfood-56-real-real-fixes.md:
  failed to parse YAML frontmatter: error: line 88 column 3:
  budget breached: ScalarBytes { total_scalar_bytes: 9086 }
```

The `dogfood_path` block scalar in that plan is 9086 bytes, over hyalo's per-document scalar
budget. So the claim 195 made about the wrong files is true of this one.

## Why it matters

`hyalo find` is how CLAUDE.md tells every agent to query the knowledgebase, and the skip is a
**warning on stderr** while the query still exits 0. Every `hyalo find --property status=...`
in this repo's workflows has been answering from 231 plans while reporting on 232, and
nothing anywhere says so. A status query that should have surfaced iteration 84 has never
surfaced it.

## The decision this iteration has to make

Three shapes, and they are not equivalent:

- **Shrink the scalar.** Iteration 84's own frontmatter says "See iteration-85 for the
  runnable dogfood_script replacement" — so the giant inline block may be replaceable by a
  `dogfood_script` key plus a sidecar, which is the shape the repo has since standardised on
  (DEC-046). This edits a terminal plan's frontmatter, which needs to be done without
  rewriting its history.
- **Raise hyalo's budget.** Out of this repo's control — hyalo is a separate tool. Worth
  confirming whether the budget is configurable via `.hyalo.toml` before assuming it is not.
- **Make the skip loud.** Whatever else happens, a warning that only appears on a discarded
  stderr is the reason this went unnoticed for months. Decide whether anything in this repo
  should fail when hyalo skips a document.

Pick deliberately and write the reasoning into `kb/decision-log.md`.

## Tasks

### A. Establish the constraint [0/2]
- [ ] Determine whether hyalo's ScalarBytes budget is configurable from `.hyalo.toml`
- [ ] Confirm 84 is the only skipped document, on this branch and on `origin/main`

### B. The fix [0/2]
- [ ] Make `hyalo find --file kb/iterations/iteration-84-*.md` return its `title` and `status`
- [ ] Whatever is edited in 84, its recorded outcome and AC state are unchanged

### C. Stop it recurring silently [0/1]
- [ ] A written disposition on whether a hyalo skip should be detectable without reading stderr

## Acceptance Criteria [0/3]

- [ ] `hyalo find --property type=iteration` walks all plans with no `skipping` warning
- [ ] Iteration 84's outcome, ACs and tick state are byte-identical apart from the
      frontmatter change the fix requires
- [ ] No new xtask subcommand (iteration 162a's decision still stands) — duplicate of Part A AC 3; tick together

## Out of scope

- **The 82 grandfathered plans.** Settled in iteration 195, DEC-047.
- **Rewriting other plans' `dogfood_path` blocks.** Only the one that breaches the budget.

## References

- `kb/decision-log.md` DEC-047 — the correction that surfaced this
- `kb/iterations/iteration-195-check-iteration-plan-fails-on-85-of-222-plans.md`
