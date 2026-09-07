---
title: "Iteration 233: nothing runs the green plan-linter sweep, and one plan is invisible to every hyalo sweep"
type: iteration
date: 2026-08-24
status: done
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

### A. Decide and record [2/2]
- [x] A written disposition covering where it runs, what it runs, and blocking vs advisory — `kb/decision-log.md` DEC-052 Part A
- [x] `CONTRIBUTING.md`'s "Running it over the whole directory" section says what now runs it

### B. Wire it [2/2]
- [x] The sweep runs automatically, on a trigger named in the disposition — the `discipline` job in `.github/workflows/ci.yml`, on every `pull_request`
- [x] A deliberately invalid plan makes it red, demonstrated in the PR — output in Results below, and pinned as a unit test

## Acceptance Criteria [3/3]

- [x] `grep -rn check-iteration-plan .github/workflows/` names a caller — `.github/workflows/ci.yml:155`
- [x] A planted invalid plan fails whatever runs the sweep, shown with output — see Results
- [x] No new xtask subcommand (iteration 162a's decision still stands) — `check-iteration-plan` gained a directory argument; `xtask --help` lists the same 12 subcommands as on `origin/main`

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

### A. Establish the constraint [2/2]
- [x] Determine whether hyalo's ScalarBytes budget is configurable from `.hyalo.toml` — it is not; `hyalo config` reports no scalar/budget key and no help text names one. Moot in any case: the budget no longer rejects the file.
- [x] Confirm 84 is the only skipped document, on this branch and on `origin/main` — **zero** documents are skipped on either. The first command run in this iteration, on a clean tree at `origin/main`, wrote 0 bytes to stderr over 450 files.

### B. The fix [2/2]
- [x] Make `hyalo find --file kb/iterations/iteration-84-*.md` return its `title` and `status` — already true on arrival; fixed upstream in hyalo, not by this iteration
- [x] Whatever is edited in 84, its recorded outcome and AC state are unchanged — **nothing was edited**; the file is untouched by this branch

### C. Stop it recurring silently [1/1]
- [x] A written disposition on whether a hyalo skip should be detectable without reading stderr — DEC-052 Part B: it already is (`hyalo lint --rule HYALO005`, exit 1), documented in `CONTRIBUTING.md`; no repo-side gate

## Acceptance Criteria [3/3]

- [x] `hyalo find --property type=iteration` walks all plans with no `skipping` warning — **satisfied by hyalo 0.22.0, not by this iteration**; verified, not delivered
- [x] Iteration 84's outcome, ACs and tick state are byte-identical apart from the
      frontmatter change the fix requires — no frontmatter change was required, so byte-identical outright
- [x] No new xtask subcommand (iteration 162a's decision still stands) — duplicate of Part A AC 3; tick together

## Out of scope

- **The 82 grandfathered plans.** Settled in iteration 195, DEC-047.
- **Rewriting other plans' `dogfood_path` blocks.** Only the one that breaches the budget.

## References

- `kb/decision-log.md` DEC-047 — the correction that surfaced this
- `kb/iterations/iteration-195-check-iteration-plan-fails-on-85-of-222-plans.md`

## Results (2026-09-07)

### Part A — the sweep now runs in CI

`check-iteration-plan` accepts a directory. Given one it walks every
`iteration-*.md`, prints each failing plan's findings, and exits 1 if any failed:

```
$ cargo run -p xtask -- check-iteration-plan kb/iterations
check-iteration-plan: swept 260 plan(s) in kb/iterations: 0 failed, 86 with warnings only
$ echo $?
0
```

The `discipline` job in `.github/workflows/ci.yml` runs exactly that line on every
pull request, blocking. `grep -rn check-iteration-plan .github/workflows/` now
names `.github/workflows/ci.yml:155`.

**The enforcement bites.** Planting the invalid plan from this iteration's
`dogfood_path`:

```
$ printf -- '---\ntitle: "x"\ntype: iteration\nstatus: planned\n---\n\n# x\n' \
    > kb/iterations/iteration-998-deliberately-invalid.md
$ cargo run -p xtask -- check-iteration-plan kb/iterations
check-iteration-plan: 1 finding(s) in kb/iterations/iteration-998-deliberately-invalid.md
  - missing dogfood_path: add a dogfood_path frontmatter key, a ## Dogfood path section, or a dogfood_script frontmatter key pointing to a sibling .sh file
check-iteration-plan: swept 261 plan(s) in kb/iterations: 1 failed, 86 with warnings only
check-iteration-plan: 1 plan(s) failed. Re-run on one file for its warnings too: cargo run -p xtask -- check-iteration-plan <plan>
$ echo $?
1
$ rm -f kb/iterations/iteration-998-deliberately-invalid.md
$ cargo run -p xtask -- check-iteration-plan kb/iterations
check-iteration-plan: swept 260 plan(s) in kb/iterations: 0 failed, 86 with warnings only
```

That demonstration lives only in this PR body, so the same plan content is pinned
as a unit test (`test_check_plan_content_fails_the_planted_invalid_plan`), along
with the directory glob and the "a parse failure is one plan's finding, not an
abort of the walk" behaviour the sweep needs and single-file mode does not have.

The repo's existing tripwire fired as designed:
`ci_162b_discipline_job_xtask_steps_are_pinned` asserts the exact number of
`cargo run -p xtask --` steps in `ci.yml`, so adding one is a deliberate edit to
that test rather than something that slips in. Bumped 5 → 6, with the reason
beside the four previous bumps.

Reasoning for all three decisions the plan demanded — where it runs, what it runs,
blocking vs advisory — is `kb/decision-log.md` DEC-052 Part A.

### Part B — the defect was fixed upstream before this iteration reached it

The plan's own premise check was right. Against
`hyalo 0.22.0 (625c5c19510d 2026-09-05)`, on a clean tree at `origin/main`:

```
$ hyalo find --property type=iteration --format text >/tmp/h.out 2>/tmp/h.err
$ echo "exit=$?  stderr bytes: $(wc -c </tmp/h.err)"
exit=0  stderr bytes: 0
```

Zero. `hyalo find --file kb/iterations/iteration-84-dogfood-56-real-real-fixes.md
--format text` returns its `title` and `status`. The `ScalarBytes` budget is not
merely raised — a planted plan carrying a 23,979-byte block scalar parses without
a warning. **Iteration 84 was not edited by this branch.**

So Tasks A/B and ACs 1–2 of the absorbed plan were satisfied by a hyalo release
rather than by this work, and are ticked as *verified*, not as *delivered*.

Task C survives, and its answer turned out to be better than the plan assumed. The
detector the plan wanted — a skip that is visible without reading stderr — already
exists:

```
$ hyalo lint --rule HYALO005          # frontmatter-parse-error, severity: error
"errors": 0 … exit 0
# with a file whose frontmatter will not parse:
"errors": 1, "rule": "HYALO005", "file": "iterations/iteration-997-badyaml.md" … exit 1
```

hyalo's own stderr warning now points at it, too: *"skipped 1 file with unparsable
frontmatter (run hyalo lint --rule HYALO005 for details)"*. Disposition: document
the command in `CONTRIBUTING.md`, add no gate — hyalo is not installed on the CI
runners, and a local-only gate nothing runs is the exact shape iteration 162a
deleted six of. Full reasoning in DEC-052 Part B, including the honest limit: the
Part A sweep is **not** a substitute, because xtask read iteration 84 fine
throughout the months hyalo was skipping it.

### Premise corrections

- The plan cites "`kb/decision-log.md`'s record that advisory PR lanes and the
  ralph-loop do not mix". No DEC records that. The record is in
  `kb/iterations/iteration-117-release-prep-v0-3.md` Theme B and the
  `project_ralph_advisory_lane_gotcha` memory note; the reasoning is unaffected and
  DEC-052 cites the real source.
- The plan's Part A framing assumed the choice was "shell loop vs a new directory
  subcommand". It is neither: an existing subcommand's path argument now accepts a
  directory, which is why AC 3 ticks.

### Carry-over

None. No new work is filed by this iteration.
