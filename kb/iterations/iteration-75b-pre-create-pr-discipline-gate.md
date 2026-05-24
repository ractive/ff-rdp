---
title: "Iteration 75b: Pre-/create-pr discipline gate — check-iteration-ready aggregator"
type: iteration
date: 2026-05-24
status: done
branch: iter-75b/pre-create-pr-discipline-gate
depends_on:
  - iteration-73-spec-fidelity-gates
  - iteration-74-protocol-correctness-oneway-events-lifecycle
first_call_sites:
  - primitive: xtask::check_iteration_ready::run
    site: crates/xtask/src/main.rs (new subcommand `check-iteration-ready` dispatch)
  - primitive: xtask::find_iteration_plan::run
    site: >-
      crates/xtask/src/main.rs (new subcommand `find-iteration-plan` dispatch —
      consumed by run-iteration.sh and the /create-pr skill)
dogfood_path: |
  # 1. One-shot pre-PR gate against the iter-77 plan (latest pending).
  FF_RDP_FIREFOX_PATH=/Users/james/devel/firefox \
    cargo run -p xtask -- check-iteration-ready \
      --plan kb/iterations/iteration-77-spec-drift-and-windows-reparse-points.md \
      --base origin/main
  # Expected: 6 sub-checks reported sequentially, final line:
  # "check-iteration-ready: 6/6 PASS"
  
  # 2. Branch → plan resolver used by run-iteration.sh and /create-pr.
  cargo run -p xtask -- find-iteration-plan --branch iter-77/spec-drift-and-windows-reparse-points
  # Expected: prints absolute path to kb/iterations/iteration-77-*.md
  
  # 3. Simulate the iter-74-style failure mode: a synthetic plan with a
  #    ticked AC naming a non-existent test must fail the aggregator and
  #    *not* reach the push step in /create-pr.
  cargo test -p xtask --test check_iteration_ready -- regression_iter74_failure_mode
tags:
  - iteration
  - process
  - gates
---

iter-74 surfaced a hole in the iteration-discipline workflow: the child
agent ran `cargo fmt && cargo clippy && cargo test`, reported "all gates
clean", and called `/create-pr` — only for `run-iteration.sh`'s Phase 2
to *then* trip on `check-dead-primitives` (a real unused `pub fn`) and
on `ac-fidelity-check.sh` (a false positive plus a real missing test).
By the time the failures fired, PR #106 was already open. iter-75b
closes the gap by making "is this iteration ready to ship?" answerable
with one command — `cargo xtask check-iteration-ready --plan <path>` —
and by hooking that command into both `run-iteration.sh`'s Phase 1
prompt and the `/create-pr` skill's pre-flight, so a discipline failure
blocks PR creation instead of trailing it.

This is a pure-process iter — no `ff-rdp-core` / `ff-rdp-cli` changes.

## Themes

- **A — `check-iteration-ready` aggregator xtask.** One subcommand,
  runs every existing discipline gate (`check-dead-primitives`,
  `check-todo-annotations`, `check-actor-kb-sync`, `check-firefox-refs`,
  `check-discipline-regression`) plus the canonical
  `ac-fidelity-check.sh`. Aggregates results — does *not* short-circuit
  on first failure so the agent sees every issue in one run.
- **B — `find-iteration-plan` resolver.** Small companion xtask:
  given a branch name `iter-<N>/<slug>` or `iter-<N>[a-z]/<slug>`,
  resolve to the on-disk plan path via the same `--glob` rule the
  ralph-loop skill uses. Lets the create-pr skill and
  `run-iteration.sh` discover the plan without re-implementing the
  glob walk.
- **C — `/create-pr` skill + CLAUDE.md + CONTRIBUTING.md wire-up.**
  Update the canonical create-pr skill, the project CLAUDE.md, and
  CONTRIBUTING.md so the aggregator runs *before* the push and PR
  creation. Mirror the create-pr skill change to a project-side copy
  if one exists.
- **D — `run-iteration.sh` Phase 1 prompt.** Both mirrors
  (`~/.claude/skills/ralph-loop/scripts/` canonical and
  `tools/ralph-loop/scripts/` in-repo) get one new sentence in the
  Phase 1 prompt telling the agent to run `check-iteration-ready`
  before `/create-pr`. Phase 2 logic stays as the belt-and-suspenders
  sanity check.

## Tasks

### A. `check-iteration-ready` aggregator

- [ ] Add `crates/xtask/src/check_iteration_ready.rs` with `pub fn run(args: Args)` and a `SubcheckResult { name, status, output }` struct. Args: `--plan <path>` (required), `--base <ref>` (default `origin/main`).
- [ ] Internally invoke the five Rust xtasks via direct function calls (not subprocesses) to keep output coherent and avoid `cargo run` startup overhead. Use the existing module APIs (`check_dead_primitives::run`, `check_todo_annotations::run`, `check_actor_kb_sync::run`, `check_firefox_refs::run`, `check_discipline_regression::run`). For `check-firefox-refs`, pass the same `--plan` argument; for the others, pass `--since <base>`.
- [ ] Shell out to `tools/ralph-loop/scripts/ac-fidelity-check.sh --plan <plan> --base <base>` (prefer the in-repo mirror for hermetic execution; fall back to `$HOME/.claude/skills/ralph-loop/scripts/ac-fidelity-check.sh` if the mirror is absent on a stripped checkout).
- [ ] Print a one-line header per sub-check (`[N/6] <name>: PASS|FAIL`); print the full sub-check output indented under each header on failure (suppress on PASS to keep output scannable). Final summary line: `check-iteration-ready: <pass>/<total> PASS` or `check-iteration-ready: <fail> sub-check(s) FAILED — fix above issues before /create-pr`.
- [ ] Aggregate: continue past the first failure, return `Err` at the end if any sub-check failed.
- [ ] Wire `CheckIterationReady` variant into `crates/xtask/src/main.rs` `Commands` enum + dispatch arm.
- [ ] Tests in `crates/xtask/tests/check_iteration_ready.rs`:
  - happy path: synthetic plan + clean diff aggregates to 6/6 PASS;
  - failure aggregation: 2 failing sub-checks both reported (not just first);
  - `regression_iter74_failure_mode`: plan with a ticked AC naming `live_nonexistent_test` exits 1 with `ac-fidelity-check.sh` failure surfaced;
  - missing `--plan`: clear error.

### B. `find-iteration-plan` resolver

- [ ] Add `crates/xtask/src/find_iteration_plan.rs` with `pub fn run(args: Args)`. Args: `--branch <name>` (required); optionally `--repo-root <path>` (default cwd).
- [ ] Parse branch into iteration ID via regex `^iter-([0-9]+[a-z]?)/`. If no match, exit non-zero with `not an iter-* branch: <name>`.
- [ ] Glob `kb/iterations/iteration-<id>-*.md` from repo root; exactly one match expected. Multiple → error listing them; zero → error with hint.
- [ ] Print the absolute path on success.
- [ ] Wire `FindIterationPlan` variant into `main.rs`.
- [ ] Tests in `crates/xtask/tests/find_iteration_plan.rs`: pure-integer ID (`iter-77/...`), letter suffix (`iter-75b/...`), no match, multiple matches, non-iter branch.

### C. CLAUDE.md + CONTRIBUTING.md + create-pr SKILL

- [ ] Update `CLAUDE.md` "Iteration discipline" section: replace the existing two-bullet "Run before commit/PR" list with a single bullet pointing at `cargo xtask check-iteration-ready --plan <plan>` and a sub-list of what it runs. Note that CI still runs the individual gates as required checks.
- [ ] Mirror to `CONTRIBUTING.md`.
- [ ] Edit `~/.claude/skills/create-pr/SKILL.md` step 2 ("Pre-flight checks") to insert a new sub-step *before* the existing "Quality gates" step: "Run `BRANCH=$(git branch --show-current); PLAN=$(cargo run -q -p xtask -- find-iteration-plan --branch \"$BRANCH\" 2>/dev/null); if [ -n \"$PLAN\" ]; then cargo run -p xtask -- check-iteration-ready --plan \"$PLAN\" --base origin/main; fi`. If the aggregator fails, stop and fix; do not push." Skip cleanly for non-iter branches (no plan resolvable).
- [ ] Add a small note explaining that `check-iteration-ready` is a strict superset of the existing `fmt+clippy+test` gates *plus* the discipline gates, so on iter branches it replaces the existing CLAUDE.md "Quality gates" pointer; on non-iter branches the existing flow stands.
- [ ] Doc-test in `crates/xtask/tests/discipline_docs_mention_aggregator.rs`: greps `CLAUDE.md`, `CONTRIBUTING.md`, and `$HOME/.claude/skills/create-pr/SKILL.md` for the string `check-iteration-ready` and asserts each contains it at least once.

### D. `run-iteration.sh` Phase 1 prompt

- [ ] Edit both `~/.claude/skills/ralph-loop/scripts/run-iteration.sh` and `tools/ralph-loop/scripts/run-iteration.sh`. Locate the Phase 1 prompt string (the one passed to the cmux child) and insert one sentence before the "/create-pr" instruction: "Before invoking /create-pr, run `cargo xtask check-iteration-ready --plan <plan-path> --base origin/main` and fix every reported failure. Do not call /create-pr until that command exits 0."
- [ ] Confirm `check-discipline-regression` still passes after the dual edit (the mirror-sync gate this iter introduced in iter-73 will catch a one-sided change).

## Acceptance Criteria [0/8]

- [x] `check_iteration_ready_happy_path`: `crates/xtask/tests/check_iteration_ready.rs::check_iteration_ready_happy_path` — synthetic plan + clean repo, aggregator exits 0 with `6/6 PASS` final line.
- [x] `check_iteration_ready_aggregates_failures`: `crates/xtask/tests/check_iteration_ready.rs::check_iteration_ready_aggregates_failures` — two sub-checks failing, both surfaced in output, exit 1.
- [x] `regression_iter74_failure_mode`: `crates/xtask/tests/check_iteration_ready.rs::regression_iter74_failure_mode` — plan with ticked AC naming `fn live_nonexistent` exits 1 with the ac-fidelity failure cited; previously this only fired in Phase 2 *after* PR creation.
- [x] `find_iteration_plan_pure_integer`: `crates/xtask/tests/find_iteration_plan.rs::find_iteration_plan_pure_integer` — branch `iter-77/spec-drift-and-windows-reparse-points` resolves to `kb/iterations/iteration-77-spec-drift-and-windows-reparse-points.md`.
- [x] `find_iteration_plan_letter_suffix`: same test file — branch `iter-75b/pre-create-pr-discipline-gate` resolves to this file's path.
- [x] `find_iteration_plan_no_match_errors`: same test file — branch `iter-99/nonexistent` exits non-zero with actionable error.
- [x] `discipline_docs_mention_aggregator`: `crates/xtask/tests/discipline_docs_mention_aggregator.rs::discipline_docs_mention_aggregator` — `CLAUDE.md`, `CONTRIBUTING.md`, and `~/.claude/skills/create-pr/SKILL.md` each contain `check-iteration-ready`.
- [x] `cargo fmt && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace -q` clean; `cargo run -p xtask -- check-discipline-regression` still green (mirror in sync, replay baselines unchanged).

## Design notes

**Implementation note: subprocess invocations, not direct function calls.**
The plan originally called for direct Rust function calls to avoid `cargo run`
startup overhead and to capture `anyhow::Error` instances directly. After
implementation, subprocess invocations via the compiled binary were chosen
instead. Rationale: each sub-check writes its diagnostic output to stdout/stderr
using `println!`/`eprintln!` — capturing those streams from a direct function
call would require either (a) a global output redirect (fragile, not thread-safe)
or (b) refactoring every sub-check to accept a `&mut dyn Write` (large blast
radius). The already-compiled xtask binary is re-exec'd for each sub-check;
since the binary is in the OS page cache from the first launch, the overhead is
<10ms per sub-check — negligible in practice. The `cargo run` fallback is used
only when invoked from a test runner or `cargo run` context, not from the binary
itself. The only sub-check that *must* shell out is `ac-fidelity-check.sh` (a
bash script).

**Why `find-iteration-plan` is a separate command rather than logic
embedded in `check-iteration-ready`?** Both the `/create-pr` skill and
`run-iteration.sh` need branch → plan resolution. Embedding in the
aggregator would force them to special-case parsing the aggregator's
output; a dedicated one-line subcommand is the cleaner contract.

**Why the doctest greps `~/.claude/skills/create-pr/SKILL.md` (a
user-global file) rather than only project files?** The create-pr skill
is the canonical location; the project does not (currently) ship its
own copy. The test will silently skip if the file is absent (CI
runners) and assert presence locally. Same skip-on-absent pattern as
the existing live-test gates.

**Replacement vs addition.** The aggregator does *not* delete or rename
the five existing xtasks — they still run in CI as required checks.
This iter only adds the one-shot wrapper plus the wiring that makes
`/create-pr` and `run-iteration.sh` call it. A future iter could
collapse the CI matrix to a single `check-iteration-ready` step, but
that's a separate decision: keeping the gates as discrete CI steps
gives clearer GitHub-side failure attribution.

## Out of scope

- Replacing any existing discipline gate. iter-75b only *aggregates*.
- Touching the AC text in existing plans. The ac-fidelity-check.sh
  precision fix from iter-74's recovery (filename-stem filter) is the
  right level — the AC convention `crates/...rs::fn_name` stays.
- Collapsing CI to a single `check-iteration-ready` step. Out of scope
  for the reasons in Design notes.
- Live tests — no protocol changes in this iter.

## References

- [[iteration-73-spec-fidelity-gates]] — introduced the two newer
  discipline xtasks this iter aggregates.
- [[iteration-74-protocol-correctness-oneway-events-lifecycle]] —
  surfaced the gap; the recovery commit `df17336` fixed the
  ac-fidelity false positive but did not close the process gap.
- `~/.claude/skills/ralph-loop/SKILL.md` "Iteration discipline" section
  — points at `crates/xtask` and explicitly lists which checks belong
  before `/create-pr`. This iter extends that list.
- `CLAUDE.md` "Iteration discipline" — the canonical reference.
