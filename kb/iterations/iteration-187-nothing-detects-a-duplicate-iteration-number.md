---
title: "Iteration 187: nothing detects a duplicate iteration number, and it has happened three times in a week"
type: iteration
date: 2026-08-23
status: planned
branch: iter-187/iteration-number-uniqueness
depends_on: []
first_call_sites: []
dogfood_path: |
  # A process defect with a measured recurrence rate, fixed by strengthening a
  # check that ALREADY RUNS rather than by adding a new gate. Read the
  # "Why this is not a new gate" section before implementing — the repo
  # deliberately deleted discipline gates in iteration 162a and this plan is
  # designed not to reverse that.

  # 1. See that the existing check is blind to it. Pick any real plan and note
  #    that validation is per-file and never looks at its siblings:
  cargo run -p xtask -- check-iteration-plan \
    kb/iterations/iteration-186-launch-records-leak-one-file-per-port.md
  #    expected: check-iteration-plan: OK

  # 2. Manufacture the collision the repo has hit three times, and watch it pass.
  cp kb/iterations/iteration-186-launch-records-leak-one-file-per-port.md \
     /tmp/iteration-186-something-else-entirely.md
  cargo run -p xtask -- check-iteration-plan /tmp/iteration-186-something-else-entirely.md
  #    expected TODAY: OK — the duplicate number is invisible
  #    expected AFTER: a failure naming both files and the number they share
  rm -f /tmp/iteration-186-something-else-entirely.md

  # 3. Confirm the directory is currently clean, so the check lands green:
  ls kb/iterations/ | sed -E 's/^iteration-([0-9]+).*/\1/' | sort -n | uniq -d
  #    expected: no output

  # 4. Every existing plan must still validate after the change:
  for p in kb/iterations/iteration-*.md; do
    cargo run -q -p xtask -- check-iteration-plan "$p" >/dev/null || echo "REGRESSED: $p"
  done
  #    expected: no output
tags: [iteration, tooling, xtask, process]
---

# Iteration 187: make the check that already runs notice a duplicate number

## The defect

Two plans can claim the same iteration number and nothing says so. Observed three times in the
week of 2026-08-17:

| # | What happened |
|---|---|
| 171 | Two plans filed as 171; one renumbered to 176 during iteration 171's carry-over |
| 178 | Two plans filed as 178; the sweep-cost plan renumbered to 180 |
| 183 | Filed alongside a better-reasoned disposition in 178, then dropped as a duplicate |

The cost is not the collision itself — it is that the second author has already written the plan
before anyone notices, and the renumber then has to chase every `[[wikilink]]`, every carry-over
row and every PR-body reference that already cited the old number. Iteration 179's PR body
carries a stale `iteration-184-dogfood-sentinel-shared-tmp-path` link from exactly this class of
churn.

`check-iteration-plan` validates one file in isolation: frontmatter, `status`, `dogfood_path`,
`first_call_sites`. It never looks at the file's siblings, so a duplicate number is invisible to
the one check CLAUDE.md already requires before filing.

## Measured 2026-08-23: two collisions already exist, so the check fails on landing

Do not discover this during implementation. Scanning `kb/iterations/*.md` on a numeric-boundary
match finds **two genuine pre-existing collisions**:

| # | Files | Status |
|---|---|---|
| 44 | `iteration-44-github-setup-guide.md`, `iteration-44-public-release.md` | both `done` |
| 73 | `iteration-73-hyalo-schema-for-iteration-plans.md`, `iteration-73-spec-fidelity-gates.md` | `obsolete`, `done` |

All four are terminal. A naive implementation goes red the moment it lands, and the tempting
"fix" — weakening the check until the repo passes — would defeat the entire iteration. Decide the
disposition deliberately and record it: an explicit two-entry exemption naming these pairs as
historical is the cheapest honest answer, since renumbering terminal plans would break inbound
`[[wikilinks]]` in merged PR bodies to no benefit.

Note also that this means the problem is **older and more frequent than the three recent cases** —
at least five collisions across the repo's life, not three in a week.

Two shapes that are NOT collisions and must not be flagged:

- **Letter suffixes.** `iteration-162a-*.md` and `iteration-162b-*.md` are deliberate, and a
  loose regex that strips the letter will report them as duplicates. Match on a numeric boundary
  (`iteration-<digits>-`), not a numeric prefix.
- **Sidecar files.** `iteration-96-profile-leak-cleanup.md` ships alongside
  `iteration-96-profile-leak-cleanup.dogfood.sh`. Consider `*.md` only.

## Why this is not a new gate

[[iteration-162]]a deliberately deleted discipline gates, taking xtask from 16 commands to 9, on
the reasoning that a gate nobody reads is worse than a documented rule. That decision stands and
this plan is not a reversal of it.

The distinction: this adds **no new command and no new step**. `check-iteration-plan` is already
required by CLAUDE.md before filing a plan, and is already run by the ralph-loop preflight. The
change is that the check it already performs becomes aware of the directory it is validating
against. Nobody has to remember anything new — the thing they already run gets one answer more
correct.

If implementation finds that uniqueness genuinely cannot be checked without a new command or a
new required step, that is a signal to stop and close this obsolete rather than to add the gate.
Say so plainly if so.

## Themes

- **A — Uniqueness inside the existing check.** When validating a plan, scan its directory for
  another `iteration-<N>-*.md` sharing `<N>` and fail naming both paths.
- **B — Don't break the ralph-loop.** The loop validates plans it is about to run, including
  plans that already exist on disk. A check that fails a plan for colliding *with itself*, or
  that fails when handed a path outside `kb/iterations/`, would red-line the whole loop.

## Tasks

### A. The check [0/5]
- [ ] `check-iteration-plan` fails when another plan in the same directory shares its number
- [ ] The failure names both file paths and the shared number, so the fix is obvious without
      re-deriving it
- [ ] A plan never collides with itself, and a path outside `kb/iterations/` is handled rather
      than panicking — unit tests for both
- [ ] `162a` / `162b` are not flagged, and `.dogfood.sh` sidecars are not counted — unit tests
      for both, since a loose regex gets each of these wrong
- [ ] The legacy 44 and 73 collisions have a recorded disposition, and the check is **not**
      weakened to accommodate them

### B. Non-regression [0/2]
- [ ] Every plan currently in `kb/iterations/` still validates
- [ ] The ralph-loop preflight still passes on a range whose plans all exist

## Acceptance Criteria [0/3]

- [ ] A manufactured duplicate fails the check, shown with the command and its output
- [ ] All existing plans still pass, shown by the loop in the dogfood path
- [ ] No new xtask command and no new required step — the command count in
      `cargo run -p xtask -- --help` is unchanged

## Out of scope

- **Auto-assigning the next free number.** Tempting and a different problem; detecting a
  collision is what has actually cost time.
- **Renumbering the existing stale references** in iteration 179's merged PR body. Merged, and
  not worth rewriting history over.

## References

- `crates/xtask/src/check_iteration_plan.rs` — `parse_plan`, `validate_plan`, `run`
- `crates/xtask/src/find_iteration_plan.rs` — already scans the directory; likely where the
  listing logic to reuse lives
