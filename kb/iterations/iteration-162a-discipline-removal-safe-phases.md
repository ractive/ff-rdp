---
branch: iter-162a/discipline-removal-safe-phases
date: 2026-08-13
depends_on: []
dogfood_path: |
  # 1. The workspace still builds and tests clean after the removal.
  cargo build -p xtask
  # → exit 0.
  cargo clippy --workspace --all-targets -- -D warnings
  # → exit 0, no unused-import or dead-code warning in crates/xtask.
  cargo test --workspace -q
  # → exit 0. No test binary named check_iteration_ready, live_check_pre_fix_repro,
  #   tools_branch_protection, claude_md_lists_new_gates or
  #   discipline_docs_mention_aggregator is linked.
  
  # 2. xtask advertises exactly the nine surviving subcommands.
  cargo run -q -p xtask -- --help
  # → lists: check-iteration-plan, check-firefox-refs, check-actor-kb-sync,
  #   check-live-test-layout, check-dogfood-script, check-source-invariants,
  #   check-discipline-regression, find-iteration-plan, live-sweep.  Nine.
  #   check-discipline-regression survives THIS iteration on purpose — 162b needs it.
  cargo run -q -p xtask -- --help | grep -cE 'check-(iteration-ready|pre-fix-repro|oneway-conformance|dead-primitives|todo-annotations|daemon-locks|error-envelope-paths|stderr-annotations)'
  # → 0
  
  # 3. The merged source-invariant gate covers what the three deleted ones covered.
  cargo run -p xtask -- check-source-invariants
  # → exit 0 on a clean tree; one named OK line per invariant
  #   (daemon-locks, error-envelope-paths, stderr-annotations).
  
  # 4. The CI discipline job invokes xtask exactly three times.
  grep -c 'cargo run -p xtask --' .github/workflows/ci.yml
  # → 3  (check-live-test-layout, check-discipline-regression, check-source-invariants)
  
  # 5. The dogfood apparatus still runs — this is the machinery with catches.
  #    FF_RDP_CURRENT_BRANCH=main because on an iter-* branch the *execution*
  #    stage hard-fails without FF_RDP_LIVE_TESTS=1 (by design, iter-85); the
  #    lint stage runs either way, which is the point being checked here.
  FF_RDP_CURRENT_BRANCH=main cargo run -p xtask -- check-dogfood-script \
    --plan kb/iterations/iteration-91-check-pre-fix-repro-perf-and-recoverability.md
  # → first line `lint-dogfood-script: PASS` (or SKIP for a plan with no
  #   dogfood_script), naming the linter rehosted out of the deleted aggregator.
  #   A FAIL naming a missing tools/lint-dogfood-script.sh means the rehost was
  #   not done.
  
  # 6. The loop harness is untouched and still works.
  cargo run -p xtask -- check-discipline-regression
  # → exit 0: all four script mirrors in sync, replay baselines 61v=FAIL / 61t=PASS hold.
  #   This iteration must NOT change that answer.
  
  # 7. No dangling *executable* reference to a deleted name.
  grep -rn -E 'cargo run -q? ?-p xtask -- check-(iteration-ready|pre-fix-repro|oneway-conformance|dead-primitives|todo-annotations)|bash .*branch-protection\.sh' \
    .github CLAUDE.md CONTRIBUTING.md tools crates
  # → matches only inside tools/ralph-loop/scripts/run-iteration.sh, which probes
  #   each subcommand with --help first and logs "xtask has no <name> — skipped".
  #   162b owns that file.  Bare mentions of the names elsewhere are deliberate:
  #   they are the prose recording why each gate was deleted (see AC7).
first_call_sites: []
status: done
title: "Iteration 162a: delete the zero-catch discipline gates (loop-safe phases)"
type: iteration
tags:
  - iteration
---

# Iteration 162a: delete the zero-catch discipline gates (loop-safe phases)

Phases 1, 2 and 4 of the removal described in
[[analysis-2026-08-13-what-ff-rdp-became]] §5. Phase 3 — shrinking
`ac-fidelity-check.sh` and deleting `claims-vs-code.sh` — is split out into
[[iteration-162b-ac-fidelity-shrink]] because it edits the loop's own scripts and must run
*after* the 158–161 batch, not before.

## Execute this one first, before iterations 158-161

**These phases touch xtask subcommands and CI steps only. They are safe to land before a
batch, because the loop adapts to a vanished subcommand on its own.** Verified at both call
sites:

- `tools/ralph-loop/scripts/run-iteration.sh:587` and `:598` probe first —
  `cargo run -q -p xtask -- check-dead-primitives --help` — and on failure log
  `"iter-N: xtask has no check-dead-primitives — skipped"`. The comment at `:582-586`
  states the intent outright: *"Probe each subcommand before running it — not every repo's
  xtask ships these … Without the probe, 'unrecognized subcommand' is logged as a FAIL, and
  Phase 2 then wastes review-model effort trying to 'fix' a check that doesn't exist."*
- `tools/new-ralph-loop/scripts/ralph.workflow.js:131` and `:154` enumerate dynamically —
  *"list its actual subcommands (`cargo run -p xtask -- --help`), run every `check-*` gate
  it offers … Do NOT invent subcommand names not in the help output."*

Neither needs editing. **Nothing in this iteration is invoked by path without a probe** —
that property belongs entirely to 162b, which is why the two are separate.

**Still hand-drive it.** Phase 1 edits `~/.claude/skills/create-pr/SKILL.md:34-43`, and
`CLAUDE.md:170-172` says *"Skill-edit iterations (those that modify `~/.claude/skills/`)
cannot run through ralph-loop itself — drive them by hand in a regular Claude session."*
The distinction that matters: this iteration **cannot be run by the loop**, but it also
**cannot break a loop already running**.

`check-discipline-regression` deliberately survives this iteration. 162b needs it alive at
the moment it edits all four script copies — it is the only thing that catches a 3-of-4
edit.

## The evidence

11,706 LOC of machinery (`crates/xtask/src` 5,907 + `crates/xtask/tests` 2,165 +
`tools/**/*.{sh,js}` 3,634) against ~52,000 lines of non-test product source — **22%**.
Across ~20 gates there are **two** real catches, both documented in [[gate-forensics]]:

- `lint-dogfood-script.sh` — iter-86's `grep -qi 'headless'`, which false-passes against
  the note text *"…regardless of headless mode…"*. A genuine false-green stopped. **This
  iteration keeps it, and Phase 1 exists partly to keep it running.**
- `check-firefox-refs` — two false Firefox spec citations (`7ed0852`: a plan cited
  `devtools/server/actors/performance.js`, which does not exist at that path). **Kept,
  locally.**

Against that:

- **Two CI "required checks" whose own step names say `(no-op in CI)`.** `ci.yml:157` —
  *"Check firefox_refs (no-op in CI — no Firefox checkout)"*; `ci.yml:161` — *"Check
  oneway conformance (no-op in CI — no Firefox checkout)"*. `check-oneway-conformance` is
  291 LOC that has never executed a real check.
- **A gate that induced a decoy and then missed the real thing.** A `DemuxReader::new()`
  was constructed in `daemon/server.rs` purely to satisfy `check-dead-primitives`; 425
  lines of dead public API shipped and survived every CI run until a human review found
  it. The removal comment is still in the tree at `daemon/server.rs:750-756`.
- **`check-todo-annotations` guards an empty set.** 0 `TODO`/`FIXME`/`XXX` in
  `crates/ff-rdp-{core,cli}/src`. The only `allow-todo:` in the tree is inside xtask's own
  test fixture (`crates/xtask/tests/run_agent_fixtures.rs:17`). 261 LOC + a 91-line
  pre-commit hook + a CI step, all for nothing.
- **`check-pre-fix-repro` is 1,192 LOC — 20% of xtask — with zero catches**, and consumed
  iterations 91 and 96 plus a dedicated bug file to stop flapping.
- **`branch-protection.sh` is self-defeating, not merely useless.** `:24` sets
  `REQUIRED_CHECK="live-tests"`, but `.github/workflows/live.yml:3-13` states in its own
  header that the lane *"no longer runs per-PR"* since iter-117. If that protection config
  were ever applied, **no PR could merge** — the required context can never report.
- **Two tests exist whose only job is asserting that CLAUDE.md contains certain strings.**
- **On 2026-08-13 every gate was green** while four user-visible defects sat in main — the
  four that iterations 158–161 exist to fix.

## Phases

### Phase 1 — the aggregator first

**Why first.** `crates/xtask/tests/check_iteration_ready.rs` hard-codes the sub-check
count in four places: the `"12/12 PASS"` assertion at `:153,161,162`, and two `[N/12]`
non-short-circuit loops at `:222-230` and `:285-288`. Removing any sub-check while the
aggregator lives costs a count bump plus assertion-text edits on every later deletion —
churn already paid twice (`17574a6`, `97a9ed9`, as the count went 10→11→12).

Delete:
- `crates/xtask/src/check_iteration_ready.rs` (364 LOC)
- `crates/xtask/tests/check_iteration_ready.rs` (326 LOC)
- `crates/xtask/tests/discipline_docs_mention_aggregator.rs` (70 LOC) — asserts that
  CLAUDE.md **and** CONTRIBUTING.md **and** `~/.claude/skills/create-pr/SKILL.md` all
  contain the literal `"check-iteration-ready"` (`:20`). Editing documentation currently
  turns the build red.
- `main.rs` wiring: `mod check_iteration_ready;` (`:9`), the `CheckIterationReady`
  variant (`:52`), and its match arm (`:89`).

**Rehost `run_lint_dogfood_script` — load-bearing, do not skip it.**
`check_iteration_ready.rs:74-165` is the **only** caller of `tools/lint-dogfood-script.sh`
outside its own integration test. It holds `LINT_DOGFOOD_SCRIPT_PATH` (`:74`) and the whole
parse-plan → locate-script → `bash` invocation. Move it into
`crates/xtask/src/check_dogfood_script.rs` and run it from `check-dogfood-script` — the two
already share `check_iteration_plan::parse_plan` and the same `dogfood_script` frontmatter
field, so this is a natural home.

The stakes: `lint-dogfood-script.sh` is one of the two gates with a real catch, and its
only other invocation path is already dead. `.github/workflows/live.yml:48-59` records that
the dogfood CI step *"was silently dead"* and was dropped, reasoning that *"the
dogfood-script gate is already enforced pre-merge locally by `check-iteration-ready`'s
`lint-dogfood-script` sub-check."* Delete the aggregator without rehosting and the linter
has no caller at all.

Also fix `crates/xtask/tests/bin_exit_codes.rs:154-190` —
`xtask_check_iteration_ready_calls_dogfood_script` drives the aggregator with seven
`--skip` flags. Rewrite it against `check-dogfood-script` directly, or delete it.

Documentation updates (every one verified present):
- `CLAUDE.md:138-149` — the "one-shot pre-PR gate" block and its 6-item sub-check list.
- `CONTRIBUTING.md:28` (`check-live-test-layout` described as *"wired into
  `check-iteration-ready`"*), `:169` (*"the 7th sub-check run by `check-iteration-ready`"*
  — **also correct the claim on the same line that it is a required CI step in
  `live.yml`; `live.yml:48-59` says that step was removed**), `:226-263` (the whole
  `### One-shot pre-PR discipline gate` section).
- `~/.claude/skills/create-pr/SKILL.md:34-43` — replace the aggregator invocation with
  the dynamic-enumeration pattern `ralph.workflow.js:131` already uses: list xtask's
  actual `check-*` subcommands from `--help` and run each. `create-pr` is a global skill
  with **no in-repo mirror**, so `check-discipline-regression` will not catch a
  half-applied edit here — check it by hand.
- `.github/workflows/live.yml:56` — comment referencing the aggregator.
- `crates/xtask/src/check_live_test_layout.rs:32` — doc comment.

`tools/ralph-loop/scripts/run-iteration.sh:202` and `:231` (and the identical
`~/.claude/skills/ralph-loop/scripts/` copy) embed `check-iteration-ready` in the
`PROMPT_IMPLEMENT` / `PROMPT_REVIEW` agent-prompt strings. **Leave these to 162b.** They are
prompt text, not executed calls: an agent told to run a nonexistent subcommand will see
clap's error and move on, whereas editing them here would touch the mirrored script pair
and drag 162b's four-way-edit problem into this iteration. Record the known-stale prompt in
Notes so 162b picks it up.

### Phase 2 — zero-catch deletions, any order

**Every deletion here must remove its `ci.yml` step in the same commit, or main goes red.**

**`check-pre-fix-repro`** — `crates/xtask/src/check_pre_fix_repro.rs` (1,192),
`crates/xtask/tests/live_check_pre_fix_repro.rs` (114),
`kb/backlog/issues/check-pre-fix-repro-worktree-flap.md` (67). No `ci.yml` step — it was
only ever wired into the aggregator, at `check_iteration_ready.rs:284-288`.

Two consequences to handle:
- `crates/ff-rdp-cli/tests/live/live_90_daemon_lifecycle.rs:231-246` — the
  `// allow-ungated-live:` annotation's entire justification is *"`xtask
  check-pre-fix-repro` runs this via `cargo test --exact` WITHOUT `--include-ignored`, so
  `#[ignore]` would make the pre-fix-repro gate unable to see it."* With the gate gone,
  re-gate the test as `#[ignore = "requires a live Firefox instance — set
  FF_RDP_LIVE_TESTS=1"]` and drop the annotation. Net **gain**: `live-sweep` then
  classifies it like every other live test instead of counting it as a bare pass.
- `CONTRIBUTING.md:39` cites a `check-pre-fix-repro` target as the canonical reason an
  ungated live test may exist. Rewrite that sentence.
- `crates/xtask/src/check_live_test_layout.rs:17` — same doc-comment reference.

**`check-oneway-conformance`** — `crates/xtask/src/check_oneway_conformance.rs` (291),
`main.rs` wiring, `ci.yml:161-164`.
Two kb documents make claims about it that are **false today**. Correct them; do not just
unlink:
- `kb/rdp/from-our-codebase/open-gaps.md:213` — *"The xtask `check-oneway-conformance` CI
  gate prevents regression."* It has never run in CI: `ci.yml:162-163` says it *"requires
  `FF_RDP_FIREFOX_PATH`; it skips gracefully when neither the env var nor `~/devel/firefox`
  are present"*, which on a GitHub runner is always.
- `kb/rdp/protocol/message-format.md:169` — *"The xtask `check-oneway-conformance` gates
  CI."* Same falsehood.
  Replacement text should say what is true: the oneway routing was fixed in iter-74 and the
  invariant is documented, not enforced.

**`check-dead-primitives`** — `crates/xtask/src/check_dead_primitives.rs` (355), `main.rs`
wiring, `ci.yml:133-134`, `CLAUDE.md:93-94,141,150`, `CONTRIBUTING.md:79-93,241,269`,
`crates/xtask/tests/bin_exit_codes.rs:176`. Leave the historical comment at
`crates/ff-rdp-cli/src/daemon/server.rs:750-756` — it documents the decoy's removal and is
the primary evidence for this deletion.
`~/.claude/skills/ralph-loop/SKILL.md:250,254,271` also describes it; updating it is
optional (the `run-iteration.sh:587` probe degrades safely either way), but preferred.

**`check-todo-annotations`** — `crates/xtask/src/check_todo_annotations.rs` (261),
`main.rs` wiring, `ci.yml:135-136`, `CLAUDE.md:95-96,142,150`,
`CONTRIBUTING.md:94-106,242,269`, `crates/xtask/tests/bin_exit_codes.rs:178`,
`~/.claude/skills/ralph-loop/SKILL.md:251,257`.

**On the pre-commit hook:** `.githooks/pre-commit` is 91 lines and its *entire* body is the
TODO/FIXME/XXX scanner — diff parsing, word-boundary matching, the three allow-forms.
There is no isolated "hook line" to remove. Delete the whole file, plus
`CONTRIBUTING.md:189-208` (`## Pre-commit hook`, which carries the
`git config core.hooksPath .githooks` install instruction) and `CLAUDE.md:172`
(*"install instructions for the pre-commit hook"*). After this the repo has no pre-commit
hook at all; that is the intended outcome. `crates/xtask/tests/run_agent_fixtures.rs:17`'s
`allow-todo:` becomes inert but harmless — leave it.

**`tools/branch-protection.sh`** (102) — plus `crates/xtask/tests/tools_branch_protection.rs`
(118), `tools/tests/branch-protection/has-live-tests.json` (25),
`tools/tests/branch-protection/missing-live-tests.json` (23), and
`CONTRIBUTING.md:326-358` (`## Branch protection — \`live-tests\` required check`, to end
of file). No CI step.

Beyond being self-defeating (see [[#The evidence]]), it shells to `python3` at `:53` and
`:66`, in a repo whose CLAUDE.md says *"All code stays in Rust — no polyglot tooling (no
Bun, Node, Python scripts)."* And it was falsified in the field: `d6f31c4` records *"main
was in fact unprotected the whole time and nothing revealed it."*

**`crates/xtask/tests/claude_md_lists_new_gates.rs`** (31) — asserts the literals
`check-firefox-refs` and `check-actor-kb-sync` appear in CLAUDE.md. Together with
`discipline_docs_mention_aggregator.rs` (Phase 1) these are the purest instance of
machinery inspecting machinery: they make editing prose a build failure.

### Phase 4 — CI consolidation

**Fold three source scanners into one subcommand and one CI step.** They already share
`crates/xtask/src/stderr_scan.rs` (70 LOC) and all three do the same thing: regex-scan
product source for a specific defect shape.

| merged in | file | LOC | `ci.yml` step |
|---|---|---|---|
| `check-daemon-locks` | `check_daemon_locks.rs` | 187 | `:137-138` |
| `check-error-envelope-paths` | `check_error_envelope_paths.rs` | 246 | `:165-169` |
| `check-stderr-annotations` | `check_stderr_annotations.rs` | 178 | `:170-174` |

→ `crates/xtask/src/check_source_invariants.rs`, one CI step. Each invariant keeps its own
named result line so a failure still says which one fired. Expected ~450 LOC merged, from
611. Keep every existing escape hatch working: the 37 `// stderr-ok:` annotations in
`crates/`, and the `#[cfg(test)]` exclusion that makes the 19 `.lock().expect(` sites under
`crates/ff-rdp-cli/src/daemon` legitimate.

**Drop two gates from CI, keep both runnable locally:**
- `check-firefox-refs` — remove `ci.yml:155-160`. Its own step name says
  `(no-op in CI — no Firefox checkout)` and the comment explains it runs against iter-73's
  plan *"which has no firefox_refs"*. The subcommand stays; it is the best gate in the repo
  (see [[#What survives]]).
- `check-actor-kb-sync` — remove `ci.yml:150-153`. It fired ≥3 times (`18146ff`,
  `e5e58e3`, `36f1c63`) and every response was *write the missing doc*. It is a docs-sync
  reminder with 8 `// allow-actor-kb-skip:` escape hatches already. Local only.
  Deleting its CI step also requires deleting `crates/xtask/tests/claude_md_lists_new_gates.rs`
  (Phase 2), which asserts CLAUDE.md still advertises it as a required check.

**Resulting CI `discipline` job**: three `cargo run -p xtask --` invocations —
`check-live-test-layout`, `check-discipline-regression`, `check-source-invariants` — down
from 10. 162b takes it to 2.

*Correction to the source analysis:* [[analysis-2026-08-13-what-ff-rdp-became]] §5 says the
job goes *"from 10 steps to 4."* Counted as `cargo run -p xtask --` invocations in
`.github/workflows/ci.yml`, the real end state across 162a+162b is **10 → 2**; the "4"
predates the decision to fold the three source scanners into one and to drop
`check-firefox-refs` from CI. This plan asserts 3 (its own end state).

## What survives

The property every survivor shares: **none of them reads acceptance-criteria text.** That
is the finding, not a coincidence.

| Kept | LOC | Why |
|---|---|---|
| `live-sweep` | 845 | Every defect found on 2026-08-13 came from running the product. `LIVE_SWEEP_SUMMARY executed=N` is the only signal in the repo derived from execution. |
| `check-live-test-layout` | 387 | Guards a demonstrated, expensive failure (`abe759b`: ungated live tests hung the Firefox-less Windows runner to a 10-min job timeout) and is *structurally required* by `live-sweep` — 45 separate live targets instead of one is the difference between a sweep that runs and one that doesn't. |
| dogfood apparatus | 251 + 191 + skill | `lint-dogfood-script.sh` + `check-dogfood-script` + the `/dogfood` skill. The only machinery with a track record of finding **product** bugs: `iteration-85:68-73` (iter-84's dogfood_path cited a `--debug-events` flag that does not exist on `navigate`), iter-87 Theme E, and the four defects of 2026-08-13. |
| `check-firefox-refs` (local) | 216 | The **only** gate that checks a claim against ground truth *outside the repository* — the actual Firefox checkout — rather than against the repo's own prose. Both its catches were false claims stopped before merge. Keep it as the template for whatever eventually replaces AC checking. |
| `check-source-invariants` (merged) | ~450 | Scans product source for a real defect shape — a command writing to stderr and exiting, bypassing the JSON envelope — which the 2026-08-13 dogfooding hit twice. |
| `check-iteration-plan` | 367 | Schema hygiene. Its 142/142 failure was a one-word vocabulary mismatch (`completed` vs `done`), fixed 2026-08-12. |
| `check-actor-kb-sync` (local) | 200 | A working docs-sync reminder. Call it that, not a defect gate. |
| `find-iteration-plan` | 194 | A branch→path resolver, not a gate. |
| `check-discipline-regression` | 208 | **Survives this iteration only.** 162b needs it alive to catch a 3-of-4 script edit, and deletes it afterwards. |

**Honest note on `check-iteration-plan`'s justification.** The stated reason to keep it is
that `parse_plan` is a library dependency of three other tools. True *today* —
`check_iteration_ready.rs:89`, `check_dogfood_script.rs:88`, `check_pre_fix_repro.rs:629` —
but two of those three are deleted by this very iteration. Afterwards exactly one in-crate
consumer remains (`check_dogfood_script.rs:88`). Keep it anyway: it is a working frontmatter
validator with its own CLI, invoked by hand and by `/create-pr`. But do not repeat the
three-consumers argument after this lands; it stops being true here.

## Out of scope

- **`ac-fidelity-check.sh` and `claims-vs-code.sh`** — [[iteration-162b-ac-fidelity-shrink]].
  Deliberately excluded so this iteration can land *before* the 158–161 batch without
  touching anything the running loop invokes by path.
- **`check-discipline-regression`** — kept alive on purpose; 162b deletes it.
- **`live-sweep --emit`** — the run-log store. 162b decides it, with four hand-run sweeps
  of evidence in hand.
- **The four product defects** in [[analysis-2026-08-13-what-ff-rdp-became]] §3. They
  belong to iterations 158–161. This iteration touches no file under
  `crates/ff-rdp-core/src` or `crates/ff-rdp-cli/src`, with the single exception of
  re-gating `live_90_daemon_lifecycle.rs` (a test file).

## Expected outcome

| Measurable | Before | Planned | Measured after 162a |
|---|---|---|---|
| In-repo discipline LOC (`crates/xtask/src` + `crates/xtask/tests` + `tools/**/*.{sh,js}`) | 11,706 | ~8,200 | **8,676** |
| Removed by this iteration | — | ~3,500 (≈30%) | **3,030 (25.9%)** |
| xtask subcommands | 16 | 9 | **9** |
| `cargo run -p xtask --` steps in the CI `discipline` job | 10 | 3 | **3** |
| xtask integration-test files | 12 | 8 | **8** |
| Repo pre-commit hooks | 1 | 0 | **0** |

Planned deletion arithmetic: Phase 1 ≈ 670 net (364 + 326 + 70, less ~90 LOC of
`run_lint_dogfood_script` rehosted); Phase 2 = 2,670 (1,192 + 114 + 67 + 291 + 355 + 261 +
91 + 102 + 118 + 48 + 31); Phase 4 ≈ 160 net (611 → ~450). Total ≈ 3,500.

Measured: **3,030**. The 470-line shortfall is real and is not being papered over. Two
line items were optimistic. The Phase 1 rehost cost ~200 lines, not ~90: once
`lint-dogfood-script` runs *before* the `FF_RDP_LIVE_TESTS` gate (so it reports on any
branch), it needs its own outcome type and result-line reporting, and three existing
`bin_exit_codes.rs` fixtures had to become lint-clean scripts to still reach the
execution stage they were testing. And the arithmetic never counted
`crates/xtask/tests/check_source_invariants.rs` (135 lines) — a file AC6 of this same
plan requires. The merged `check_source_invariants.rs` itself landed at ~430, inside its
~450 estimate.

## Acceptance Criteria [12/13]

- [x] `unit_162a_xtask_help_lists_survivors`: `cargo run -q -p xtask -- --help` names exactly `check-iteration-plan`, `check-firefox-refs`, `check-actor-kb-sync`, `check-live-test-layout`, `check-dogfood-script`, `check-source-invariants`, `check-discipline-regression`, `find-iteration-plan`, `live-sweep` — 9 subcommands — and `grep -cE 'check-(iteration-ready|pre-fix-repro|oneway-conformance|dead-primitives|todo-annotations|daemon-locks|error-envelope-paths|stderr-annotations)'` over that output returns 0. [measured 2026-08-13: 9 subcommands, exactly that set; forbidden-name count 0]
- [x] `cargo build -p xtask` exits 0 and `cargo clippy --workspace --all-targets -- -D warnings` exits 0 with no unused-import or dead-code warning in `crates/xtask`. [measured 2026-08-13: both exit 0]
- [x] `cargo test --workspace -q` exits 0, and `crates/xtask/tests/` contains exactly 8 files — `ac_fidelity_check.rs`, `bin_exit_codes.rs`, `check_actor_kb_sync.rs`, `check_firefox_refs.rs`, `check_source_invariants.rs`, `find_iteration_plan.rs`, `lint_dogfood_script.rs`, `run_agent_fixtures.rs`. [measured 2026-08-13: exit 0, 0 failures across all targets; `ls crates/xtask/tests/` = those 8 files]
- [x] `ci_162a_discipline_job_three_xtask_steps`: `grep -c 'cargo run -p xtask --' .github/workflows/ci.yml` returns 3, and each of `check-live-test-layout`, `check-discipline-regression`, `check-source-invariants` appears in `cargo run -q -p xtask -- --help`. [measured 2026-08-13: 3; all three present in help]
- [x] `unit_162a_lint_dogfood_rehosted`: `crates/xtask/tests/lint_dogfood_script.rs` passes unchanged, and `cargo run -p xtask -- check-dogfood-script --plan <a plan carrying a dogfood_script field>` emits a `lint-dogfood-script:` result line, proving the linter kept a caller after the aggregator's deletion. Covered by `xtask_check_dogfood_script_runs_lint` and `xtask_check_dogfood_script_fails_on_lint_error` in `crates/xtask/tests/bin_exit_codes.rs`, plus `lint_dogfood_script_passes_on_clean_script` / `lint_dogfood_script_fails_on_dirty_script` in `check_dogfood_script.rs`. [measured 2026-08-13: against iteration-91's plan → `lint-dogfood-script: PASS`, `[lint-dogfood-script] OK: …iteration-91-….dogfood.sh`]
- [x] `unit_162a_source_invariants_covers_three`: a new `crates/xtask/tests/check_source_invariants.rs` asserts `check-source-invariants` reports a distinct named failure for each of three synthetic fixtures — a `.lock().unwrap()` under `daemon/`, an `eprintln!` followed by `AppError::Exit(N)`, and an unannotated `eprintln!` in `commands/` — and that the subcommand exits 0 against the real tree. [measured 2026-08-13: 5 tests pass — `daemon_lock_unwrap_fails_named_invariant`, `eprintln_then_exit_bypass_fails_named_invariant`, `unannotated_eprintln_fails_named_invariant`, `clean_tree_passes_all_three`, `real_tree_passes`]
- [x] `unit_162a_no_dangling_gate_references`: no *executable* reference to a deleted gate remains — no `cargo run … check-<deleted>` invocation, no script path, no `mod`/match arm. `grep -rnE 'check-(iteration-ready|pre-fix-repro|oneway-conformance|dead-primitives|todo-annotations)|branch-protection\.sh' .github CLAUDE.md CONTRIBUTING.md .githooks tools crates` returns only: explanatory prose recording why each gate went (`ci.yml:132-134`, `CLAUDE.md:94,99,142,181`, `CONTRIBUTING.md:82,87,229,327`, `check_dogfood_script.rs:28`, `bin_exit_codes.rs:177`, `live_90_daemon_lifecycle.rs:232`, `daemon/server.rs:752`) and `run-iteration.sh`'s two agent-prompt strings (`:202`, `:231`) plus its two probe blocks (`:583-606`), which 162b owns. [measured 2026-08-13]

  **The original predicate ("returns no matches") was wrong and is corrected here, not routed around.** It contradicted this plan's own Phase 2 instruction to *correct* the two false kb claims rather than unlink them, and its own [[#The evidence]] section, which is the reason each deletion is defensible. A grep that forbids naming a deleted gate forbids explaining why it was deleted. `daemon/server.rs:752` is explicitly protected by Phase 2 ("Leave the historical comment … it is the primary evidence for this deletion") and would have failed the original grep on its own.
- [x] `check-discipline-regression` exits 0 at this iteration's HEAD — all four script mirrors in sync, replay baselines `61v=FAIL` / `61t=PASS` still holding — confirming 162a left the loop harness intact for the 158–161 batch. Paste the output into the PR body. [measured 2026-08-13: ralph-loop mirror in sync (3 files), new-ralph-loop mirror in sync (5 files), replay baselines OK (61v=FAIL, 61t=PASS), exit 0]
- [x] `check-live-test-layout` exits 0 after `live_90_daemon_lifecycle.rs`'s `pre_fix_repro_daemon_state_sharing_red_then_green` is re-gated to `#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]` with its `// allow-ungated-live:` annotation removed, and `cargo run -p xtask -- live-sweep` counts that test in its `total=T`. [verified: 2026-08-13, check-live-test-layout exit 0; `LIVE_SWEEP_SUMMARY executed=0 skipped=223 total=223` on this branch vs `total=222` on main — the +1 is the re-gated test, now classified instead of counted as a bare pass]
- [x] `kb/rdp/from-our-codebase/open-gaps.md` and `kb/rdp/protocol/message-format.md` no longer assert that `check-oneway-conformance` enforces anything in CI; each instead records that the iter-74 oneway routing fix is documented rather than gated. Verify with `hyalo find "check-oneway-conformance"` returning only historical and analysis documents. [measured 2026-08-13: 7 documents — the two corrected reference docs, iteration-74 and iteration-162a plans, iteration-104, the 2026-08-13 analysis, and a 2026-07 research doc; none claims CI enforcement]
- [x] `.githooks/pre-commit` is deleted, `CONTRIBUTING.md` contains no `## Pre-commit hook` section and no `core.hooksPath` instruction, and `git commit` on a scratch branch adding a line containing a bare `FIXME` succeeds — confirming the hook is gone rather than merely bypassed. [measured 2026-08-13: `.githooks/` no longer exists; scratch branch commit `3d96718` of a file containing a bare `FIXME` exited 0, branch deleted afterwards]
- [x] `~/.claude/skills/create-pr/SKILL.md` contains no reference to `check-iteration-ready`; its gate block instead enumerates xtask's `check-*` subcommands from `cargo run -q -p xtask -- --help`, and invoking `/create-pr` on a scratch `iter-*` branch reaches the quality-gates step without a clap "unrecognized subcommand" error. [measured 2026-08-13: `grep -c check-iteration-ready` → 0; the block now runs `--help` and instructs "Do NOT invent subcommand names that are not in the help output"; exercised by this iteration's own `/create-pr` run on `iter-162a/discipline-removal-safe-phases`]
- [ ] `unit_162a_loc_reduction`: `find crates/xtask/src crates/xtask/tests -name '*.rs' | xargs wc -l` plus `find tools -name '*.sh' -o -name '*.js' | xargs wc -l` totals ≤ 8,400 lines, down from the measured 11,706 — a reduction of ≥ 3,300 lines (≥ 28%).

  **NOT MET, and left unticked rather than reworded to fit.** Measured 2026-08-13 at this branch's HEAD: xtask 5,144 + tools 3,532 = **8,676**, down **3,030** lines (**25.9%**) — 276 lines above the ceiling, 270 short of the reduction target. The estimate was optimistic in two places. Phase 1's `run_lint_dogfood_script` rehost cost ~200 lines rather than the ~90 assumed: once the linter runs *before* the `FF_RDP_LIVE_TESTS` gate (so it reports on any branch) it needs its own outcome type and result-line reporting, and three `bin_exit_codes.rs` fixtures had to become lint-clean scripts to still reach the execution stage they were testing. And the arithmetic never counted the 135-line `crates/xtask/tests/check_source_invariants.rs` that AC6 of this same plan *requires*. Deleting a required test or a working lint path to reach an LOC figure would be the exact behaviour this iteration exists to stop, so neither was done. Every structural target — 9 subcommands, 3 CI steps, 8 test files, 0 pre-commit hooks — was met exactly.

## Notes

- **Known-stale references handed to 162b.**
  `tools/ralph-loop/scripts/run-iteration.sh:202` (`PROMPT_IMPLEMENT`, step 6) and `:231`
  (`PROMPT_REVIEW`, step 0) instruct an agent to run `cargo run -p xtask --
  check-iteration-ready`, which this iteration deletes. Same text in
  `~/.claude/skills/ralph-loop/scripts/run-iteration.sh`. These are prompt strings, not
  executed calls — an agent hits clap's error and continues — so they are left for 162b,
  which is already editing that mirrored pair and can fix all four copies at once.
  `~/.claude/skills/ralph-loop/SKILL.md:250-271` likewise describes deleted gates.

- **Optional cleanup, explicitly not required.**
  `run-iteration.sh:587-596` and `:598-607` are the probe blocks for `check-dead-primitives`
  and `check-todo-annotations`. After this iteration both log `"xtask has no <name> —
  skipped"` forever. Deleting them is tidy; leaving them is safe and keeps this iteration
  out of the mirrored-script problem entirely. Prefer leaving them; 162b can remove them.
  **Left in place, as preferred.**

- **Executed by hand on 2026-08-13, in three commits** — one per phase, each removing its
  own `ci.yml` steps so `main` never sees a job referencing a deleted subcommand.
  `~/.claude/skills/ralph-loop/SKILL.md` was also updated (optional per Phase 2): its
  hard-coded two-gate list became the same dynamic-enumeration instruction, with a note
  that a fixed list rots into "unrecognized subcommand" errors the review phase then
  wastes effort trying to fix. It has no in-repo mirror, so `check-discipline-regression`
  cannot see that edit — it was checked by hand, like the `create-pr` one.

- **`ripgrep` is no longer a CI dependency.** `check-dead-primitives` was the only
  consumer of the `Install ripgrep` step in the `discipline` job; it went with the gate.

- **What this iteration does not claim.** It does not claim the removed gates were
  worthless in principle. `check-dead-primitives` is a reasonable idea; it was gamed. The
  claim is narrower and measurable: across ~20 gates and ~2.5 months, two real catches,
  against 28 reword commits, two self-declared CI no-ops, one induced decoy, and four
  user-visible defects that all shipped green.

- **The process rule this iteration serves.** From
  [[analysis-2026-08-13-what-ff-rdp-became]]: *"an iteration that changes zero product
  source is not an iteration."* This one changes zero product source and is the last of its
  kind alongside 162b — it exists to stop the 154 → 155 → 156 → 157 chain, not to extend
  it. If a later iteration proposes new discipline machinery, the burden is a named defect
  it would have caught, on a real commit.

- **Related plans to close.** [[iteration-156-ac-fidelity-names-its-test]] and
  [[iteration-157-live-sweep-classifier-drift]] should be marked `obsolete` alongside this
  work: 156 addresses friction in a gate 162b shrinks to one rule, and 157 files a
  classifier bug against `live-sweep` when the sweep's real defect is the silent-skip path
  in the test harness, not the classifier.

## Links

[[iteration-162b-ac-fidelity-shrink]] · [[analysis-2026-08-13-what-ff-rdp-became]] ·
[[step-back-2026-08-13]] · [[decision-log]] (DEC-030, DEC-031) ·
[[iteration-61y-iteration-discipline-tooling]] · [[iteration-73-spec-fidelity-gates]] ·
[[iteration-74-protocol-correctness-oneway-events-lifecycle]] ·
[[iteration-75b-pre-create-pr-discipline-gate]] ·
[[iteration-87-gate-hardening-required-checks-and-dogfood-linter]] ·
[[iteration-91-check-pre-fix-repro-perf-and-recoverability]] ·
[[iteration-101-daemon-session-correctness]] · [[iteration-155-live-skip-reports-green]] ·
[[iteration-156-ac-fidelity-names-its-test]] ·
[[iteration-157-live-sweep-classifier-drift]] ·
[[iteration-158-launch-lifecycle-and-harness-honesty]]
