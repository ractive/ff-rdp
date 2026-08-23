---
title: "Iteration 195: check-iteration-plan fails on 85 of the 222 plans in the tree, so it can never be run as a sweep"
type: iteration
date: 2026-08-23
status: planned
branch: iter-195/plan-linter-whole-directory-sweep
depends_on: [187]
first_call_sites: []
dogfood_path: |
  # 1. The measurement. 85 of 222 plans fail the check CLAUDE.md requires
  #    before filing a plan:
  cargo build -q -p xtask
  for p in kb/iterations/iteration-*.md; do
    ./target/debug/xtask check-iteration-plan "$p" >/dev/null 2>&1 || echo "$p"
  done | wc -l
  #    expected TODAY: 85
  #    expected AFTER: 0

  # 2. Three of them do not even parse — a different class from the other 82:
  for p in kb/iterations/iteration-80-*.md kb/iterations/iteration-82-*.md \
           kb/iterations/iteration-83-*.md; do
    echo "== $p"; ./target/debug/xtask check-iteration-plan "$p" 2>&1 | head -4
  done
  #    expected TODAY: "Error: failed to parse YAML frontmatter" for each
  #    expected AFTER: OK

  # 3. Iteration 187's dogfood step 3 claims this reports nothing. It does not,
  #    and its sed also strips letter suffixes (61b -> 61). Whatever this
  #    iteration decides, that step's expectation must end up true or be
  #    rewritten to something that is:
  ls kb/iterations/ | sed -E 's/^iteration-([0-9]+).*/\1/' | sort -n | uniq -d
tags: [iteration, tooling, xtask, process, carry-over]
---

# Iteration 195: the plan linter has never been runnable over the whole directory

## Where this came from

Iteration 187 taught `check-iteration-plan` to detect duplicate iteration numbers. Verifying that
it introduced no regression meant running it over every plan on both `origin/main` and the branch.
That produced a number nobody had measured before:

| | plans | failing `check-iteration-plan` |
|---|---|---|
| `origin/main` (before 187) | 222 | 85 |
| iter-187 branch | 222 | 85 |

187 changed nothing here — the two failure sets are identical, `comm` both ways is empty. But its
acceptance criterion "All existing plans still pass" and its Task B box "Every plan currently in
`kb/iterations/` still validates" were both written on the assumption that the sweep was green.
It never was. Those boxes were left unticked with this iteration named as the reason, rather than
reworded to match what happened.

## The two classes

**82 plans predate the requirement they fail.** Plans `iteration-01-*` through `iteration-61y-*`
have no `dogfood_path` frontmatter key and no `## Dogfood path` section; the requirement was added
later and never backfilled. All are terminal.

**3 plans have frontmatter that does not parse at all:** `iteration-80-ff-rdp-ergonomics-bundle`,
`iteration-82-dogfood-54-fixes`, `iteration-83-dogfood-55-real-fixes` all fail with
`failed to parse YAML frontmatter`. This is worse than a missing field: `hyalo` queries and the
ralph-loop's `extract_title`/`check_completion` read the same frontmatter, so whatever these files
contain is invisible to every tool that walks the vault, not just to xtask.

## The decision this iteration has to make

Do **not** start by editing 82 files. The prior question is what a whole-directory sweep is for:

- If the check is only ever meant to run on a plan being filed, then the 82 legacy plans are not
  a defect and the honest fix is to say so — in `CONTRIBUTING.md` and in 187's dogfood step 4 —
  and to fix only the 3 unparseable files. Cheapest, and arguably correct.
- If a green sweep is worth having (it would let CI run the linter over the directory, which is
  the only way any of this gets enforced without a human remembering), then the 82 need
  backfilling and the requirement needs a grandfather date or a `legacy: true` marker.

Pick one deliberately and write the reasoning down. Do not split the difference by weakening the
`dogfood_path` requirement for new plans — that is the requirement doing its job.

## Themes

- **A — The three unparseable files.** Worth fixing under either decision, because they are
  invisible to `hyalo` and to the ralph-loop preflight, not merely to xtask.
- **B — The disposition for the 82.** Backfill, grandfather, or declare the sweep out of scope.
- **C — Make 187's dogfood steps 3 and 4 true.** Both currently say "expected: no output" and
  both are wrong today; step 3's sed additionally maps `61b` to `61`.

## Tasks

### A. Unparseable frontmatter [0/2]
- [ ] Identify what is malformed in plans 80, 82 and 83 and fix it without changing their meaning
- [ ] `hyalo find --file` returns their `title` and `status` afterwards, not an error

### B. The 82 legacy plans [0/2]
- [ ] A written disposition — backfill, grandfather, or out-of-scope — with the reasoning
- [ ] Whichever is chosen, `CONTRIBUTING.md`'s "Validate an iteration plan" section says what a
      whole-directory run is expected to report

### C. Fix the stale expectations [0/1]
- [ ] Iteration 187's dogfood steps 3 and 4 either pass as written or are corrected

## Acceptance Criteria [0/3]

- [ ] Plans 80, 82 and 83 parse, shown by `check-iteration-plan` output for each
- [ ] The whole-directory sweep's expected result is documented and matches what it actually
      prints — whether that number is 0 or 82
- [ ] No new xtask subcommand (iteration 162a's decision still stands)

## Out of scope

- **Weakening the `dogfood_path` requirement for new plans.** The 82 failures are historical; the
  requirement is what makes a new plan reviewable.
- **Duplicate-number detection.** Delivered in iteration 187.

## References

- `crates/xtask/src/check_iteration_plan.rs` — `validate_plan`, `parse_plan`
- `kb/iterations/iteration-187-nothing-detects-a-duplicate-iteration-number.md` — the measurement
  and the two deliberately-unticked boxes
