---
title: "Iteration 205: one iteration plan is invisible to hyalo, and every vault sweep has been silently skipping it"
type: iteration
date: 2026-08-24
status: planned
branch: iter-205/plan-84-invisible-to-hyalo
depends_on: [195]
first_call_sites: []
dogfood_path: |
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
tags: [iteration, tooling, hyalo, process, carry-over]
---

# Iteration 205: `iteration-84` is invisible to every hyalo sweep

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
- [ ] No new xtask subcommand (iteration 162a's decision still stands)

## Out of scope

- **The 82 grandfathered plans.** Settled in iteration 195, DEC-047.
- **Rewriting other plans' `dogfood_path` blocks.** Only the one that breaches the budget.

## References

- `kb/decision-log.md` DEC-047 — the correction that surfaced this
- `kb/iterations/iteration-195-check-iteration-plan-fails-on-85-of-222-plans.md`
