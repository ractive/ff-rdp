---
title: "Iteration 195: check-iteration-plan fails on 85 of the 222 plans in the tree, so it can never be run as a sweep"
type: iteration
date: 2026-08-23
status: in-review
branch: iter-195/plan-linter-whole-directory-sweep
depends_on:
  - 187
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
tags:
  - iteration
  - tooling
  - xtask
  - process
  - carry-over
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

### A. Unparseable frontmatter [2/2]
- [x] Identify what is malformed in plans 80, 82 and 83 and fix it without changing their meaning
      — `first_call_sites` entries written as `"Primitive: crates/…/file.rs"` strings where the
      schema wants `{primitive, site}` maps. Each string was split at its `: ` and rewritten as a
      map; no wording changed.
- [x] `hyalo find --file` returns their `title` and `status` afterwards, not an error
      — but it did so **before** the fix too. See "A correction to this plan's premise" below.

### B. The 82 legacy plans [2/2]
- [x] A written disposition — backfill, grandfather, or out-of-scope — with the reasoning
      → `kb/decision-log.md` DEC-047: grandfathered by exact file name, findings downgraded to
      warnings rather than suppressed. Backfill rejected as inventing evidence; out-of-scope
      rejected because a non-zero expected count is unenforceable.
- [x] Whichever is chosen, `CONTRIBUTING.md`'s "Validate an iteration plan" section says what a
      whole-directory run is expected to report → new "Running it over the whole directory"
      subsection: zero failures, any output is a regression.

### C. Fix the stale expectations [1/1]
- [x] Iteration 187's dogfood steps 3 and 4 either pass as written or are corrected
      — step 3 rewritten (it also had to exclude `.dogfood.sh` sidecars, which the plan had not
      spotted: uncorrected it printed 18 numbers, not 3); step 4 now passes as written.

## Acceptance Criteria [3/3]

- [x] Plans 80, 82 and 83 parse, shown by `check-iteration-plan` output for each
      — all three print `check-iteration-plan: OK`.
- [x] The whole-directory sweep's expected result is documented and matches what it actually
      prints — whether that number is 0 or 82 → documented as 0, and the sweep over all 232
      plans prints nothing.
- [x] No new xtask subcommand (iteration 162a's decision still stands) — `cargo run -p xtask --
      --help` lists the same 9 commands as on `origin/main`.

## Out of scope

- **Weakening the `dogfood_path` requirement for new plans.** The 82 failures are historical; the
  requirement is what makes a new plan reviewable.
- **Duplicate-number detection.** Delivered in iteration 187.

## Outcome (2026-08-24)

The sweep is green: 0 failures over all 232 plans (the tree had grown from 222 to 232 since the
measurement in this plan's header; the failing count was still exactly 85).

The 85 broke down as 3 schema violations + 82 pre-requirement plans. Every one of the 82 carries
an iteration id of 61 or lower, all are terminal, all are missing `dogfood_path`, and 5 are also
missing `first_call_sites` — a single clean class with a clean boundary, because the requirements
arrived with iteration 62.

**Disposition: grandfather, keyed on exact file name.** `LEGACY_PRE_DISCIPLINE_PLANS` in
`crates/xtask/src/check_iteration_plan.rs` lists the 82; `validate_plan` downgrades their two
content findings to warnings, so a sweep still prints what each is missing while exiting 0.
`status` validation and duplicate-number detection still apply to them in full. Keyed on file name
rather than on `id <= 61` for the same reason iteration 187 keyed `LEGACY_COLLISIONS` that way: a
newly filed `iteration-61z-*.md` must not fall into the exemption. Full reasoning and the rejected
alternatives are in `kb/decision-log.md` DEC-047.

### A correction to this plan's premise

This plan asserted that plans 80, 82 and 83 "have frontmatter that does not parse at all" and are
therefore "invisible to every tool that walks the vault, not just to xtask". Both halves were
wrong. Their YAML parses fine — `hyalo find --file` returned their `title` and `status` before any
change here. What failed was xtask's *typed* view of it, and the error text
(`failed to parse YAML frontmatter`) is what made it look like a syntax error. `parse_plan` now
parses in two steps and distinguishes "not YAML" from "valid YAML, wrong shape".

Checking the claim did turn up a real instance of it, in a file this plan never named:
`iteration-84-dogfood-56-real-real-fixes.md` is the one plan `hyalo` genuinely cannot read
(a 9086-byte block scalar over its `ScalarBytes` budget), and the skip is a stderr warning on an
otherwise successful exit — so every scripted `hyalo find` in this repo has been silently
answering from 231 plans. Filed as iteration 205.

### Carry-over

- **[[iteration-205-plan-84-is-invisible-to-hyalo]]** — the one genuinely hyalo-unreadable plan,
  and whether a silent skip should be detectable without reading stderr.
- **[[iteration-206-nothing-runs-the-plan-linter-sweep]]** — this iteration made the sweep green
  because a green sweep is enforceable; wiring the enforcement was not in its tasks or ACs and is
  not done here.

## References

- `crates/xtask/src/check_iteration_plan.rs` — `validate_plan`, `parse_plan`
- `kb/decision-log.md` DEC-047 — the disposition for the 82
- `kb/iterations/iteration-187-nothing-detects-a-duplicate-iteration-number.md` — the measurement
  and the two deliberately-unticked boxes
