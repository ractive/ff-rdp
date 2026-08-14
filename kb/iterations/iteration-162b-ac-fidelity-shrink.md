---
branch: iter-162b/ac-fidelity-shrink
date: 2026-08-13
depends_on:
  - kb/iterations/iteration-162a-discipline-removal-safe-phases.md
  - kb/iterations/iteration-161-eval-and-flag-strictness.md
dogfood_path: |
  # 0. PRECONDITION — the 158-161 batch has merged and no loop is running.
  gh pr list --state open --search 'head:iter-' --json number,headRefName
  # → must be empty.  Any open iter-* PR means a loop may still invoke
  #   ac-fidelity-check.sh / claims-vs-code.sh by path; do not start.

  # -1. Cross-repo: magnificient hard-fails when the script vanishes.  Do this FIRST.
  rg -n 'ac-fidelity' ~/devel/magnificient/xtask/src/main.rs
  # → no matches, after its `ac-fidelity` subcommand is removed.  Before the fix,
  #   locate_ac_script() returns None and ac_fidelity() returns (false, …).

  # 1. Phase 3a: the four-way delete lands while the mirror gate is alive.
  cargo run -p xtask -- check-discipline-regression
  # → exit 0.  THIS GREEN IS THE 4-OF-4 PROOF — capture it in the PR body.
  ls ~/.claude/skills/ralph-loop/scripts/ ~/.claude/skills/new-ralph-loop/scripts/ \
     tools/ralph-loop/scripts/ tools/new-ralph-loop/scripts/
  # → four non-empty listings (run-iteration.sh, ralph.workflow.js, …) with neither
  #   ac-fidelity-check.sh nor claims-vs-code.sh in any of them.  Assert the
  #   directories are non-empty first — an empty listing is not proof of deletion.

  # 2. Nothing invokes the deleted scripts any more.
  rg -n 'ac-fidelity|claims-vs-code' tools ~/.claude/skills/{ralph-loop,new-ralph-loop} \
     .github CLAUDE.md CONTRIBUTING.md crates
  # → no matches.

  # 3. Phase 3b: the mirror gate itself goes.
  cargo run -q -p xtask -- --help
  # → 8 subcommands; check-discipline-regression is gone.
  grep -c 'cargo run -p xtask --' .github/workflows/ci.yml
  # → 2  (check-live-test-layout, check-source-invariants)

  # 4. Nothing in the loop harness references a deleted script or subcommand.
  grep -rn -E 'claims-vs-code|check-(iteration-ready|discipline-regression)' \
    tools ~/.claude/skills/ralph-loop ~/.claude/skills/new-ralph-loop ~/.claude/skills/create-pr \
    .github CLAUDE.md CONTRIBUTING.md crates
  # → no matches.

  # 5. The loop still starts.  Smoke it, do not assume it.
  node ~/.claude/skills/new-ralph-loop/scripts/smoke.workflow.js
  # → exits 0; the review-phase prompt it prints contains no path to claims-vs-code.sh.

  cargo build -p xtask && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace -q
  # → all three exit 0.
first_call_sites: []
status: planned
title: "Iteration 162b: delete ac-fidelity-check and claims-vs-code"
type: iteration
tags:
  - iteration
---

# Iteration 162b: delete ac-fidelity-check and claims-vs-code

Phase 3 of the removal described in [[analysis-2026-08-13-what-ff-rdp-became]] §5,
split out from [[iteration-162a-discipline-removal-safe-phases]].

> **Scope changed 2026-08-14, after the 158–161 batch.** This plan previously shrank
> `ac-fidelity-check.sh` to one surviving rule and carried an open `live-sweep --emit`
> decision. Both are dropped: the script is **deleted outright**, and no rule replaces it.
> The batch supplied the missing evidence, and the owner supplied the missing fact — see
> [[#Why nothing replaces it]]. Deleting instead of shrinking removes ~1,860 LOC instead of
> ~1,270, eliminates the `--emit` decision, and leaves nothing to mirror for this file.

## Run this AFTER the 158-161 batch — never before, never during

**162a was safe to land before a batch. This is not.** The difference is exact and
verified: 162a touched only xtask subcommands, and both loop drivers probe or enumerate
before invoking those. This iteration touches the two shell scripts the loop calls **by
path, with no probe and no fallback**:

| Call site | Line | Invocation |
|---|---|---|
| `tools/ralph-loop/scripts/run-iteration.sh` | `:83` | `"$SCRIPT_DIR/claims-vs-code.sh" --range "$RANGE"` (replay path) |
| `tools/ralph-loop/scripts/run-iteration.sh` | `:89` | `"$SCRIPT_DIR/ac-fidelity-check.sh" --plan … --range …` (replay path) |
| `tools/ralph-loop/scripts/run-iteration.sh` | `:614` | `"$script_dir/claims-vs-code.sh" --branch … --base main` |
| `tools/ralph-loop/scripts/run-iteration.sh` | `:623` | `"$script_dir/ac-fidelity-check.sh" --plan … --branch …` (hard gate) |
| `tools/new-ralph-loop/scripts/ralph.workflow.js` | `:152` | `${skillDir}/scripts/claims-vs-code.sh …` in the review prompt |
| `tools/new-ralph-loop/scripts/ralph.workflow.js` | `:153` | `${skillDir}/scripts/ac-fidelity-check.sh …` in the review prompt |

Deleting or moving either script mid-batch breaks the harness running the batch. **Phase 3
must update those six call sites in the same change**, in both the skill copies and the
in-repo mirrors.

**Also hand-driven.** It edits `~/.claude/skills/ralph-loop/scripts/` and
`~/.claude/skills/new-ralph-loop/scripts/`, and `CLAUDE.md:170-172` says *"Skill-edit
iterations (those that modify `~/.claude/skills/`) cannot run through ralph-loop itself —
drive them by hand in a regular Claude session."* Here both reasons apply at once.

**Precondition to check before starting:** no open `iter-*` PR, no running loop. If 158–161
are still in flight, stop.

## The evidence

`ac-fidelity-check.sh`'s own header states the limit (`tools/ralph-loop/scripts/ac-fidelity-check.sh:8-15`):

> **SCOPE** — read this before trusting a green result. This script only ever sees a plan
> file and a diff. It CANNOT and DOES NOT verify that any test was executed, or that it
> passed.

Measured against that:

- **28 commits whose entire content is rewording an AC so the gate stops firing.** The
  complete list is in [[gate-forensics]] §5b. The clearest is `c51656d` (iter-86): 1 file,
  +12/−13, all in the kb plan, changing the AC from `perf audit`/non-headless to `perf
  vitals`/headless — different command, different mode — to match whichever test existed.
  Gate: 11/11 PASS. The commit's own subject is *"fix AC names to match actual test
  slugs."*
- **The evidence heuristics are satisfied by a text edit.** Eight of the 28 commits say so
  in their subject lines: *"backtick test slugs on same line for ac-fidelity heuristic"*,
  *"put a gate-resolvable symbol on AC4's first line"*, *"unwrap unit_145 AC line so
  ac-fidelity-check finds it"*.
- **The `[verified: …]` requirement was falsified on 2026-08-13.** iter-153's ticked AC
  carried `[verified: 2026-08-13, … 3 passed / 0 failed]`. The annotation is **truthful** —
  that run happened and did pass. It certified a broken feature anyway, because
  `live_153_replace_emits_single_envelope` passes in isolation and fails under contention.
  Isolated verification of a `live_*` AC is not evidence.
- **`claims-vs-code.sh` has never fired.** Advisory from day one; the plan to promote it to
  a hard gate ([[iteration-61aa-claim-miss-hard-gate]]) is `status: obsolete`; there is
  exactly **one** `// allow-claim-miss:` in the whole tree
  (`crates/ff-rdp-cli/src/page_map/mod.rs:166`).
- **The quadruplication is self-inflicted.** `ac-fidelity-check.sh` exists in four
  byte-identical copies (md5 `daec26b5…`), `claims-vs-code.sh` in four (md5 `fbeb76c3…`),
  and `check-discipline-regression` (208 LOC) exists solely to keep them in sync — a
  maintenance obligation created entirely by the deployment shape.

## Phase 3 — one atomic change; the order is non-invertible

The four copies:

```
~/.claude/skills/ralph-loop/scripts/
~/.claude/skills/new-ralph-loop/scripts/
tools/ralph-loop/scripts/          (mirror)
tools/new-ralph-loop/scripts/      (mirror)
```

### 3a — edit all four copies in one commit, while `check-discipline-regression` still runs

**This ordering is the whole reason 162a left `check-discipline-regression` alive.** It is
the only thing that would catch a 3-of-4 edit, and a 3-of-4 edit is exactly what happened
in iter-140/146 (`3dc5330`): an `ac-fidelity-check.sh` fix landed in the ralph-loop mirror,
was silently missed in the unmirrored new-ralph-loop copy, produced a false failure, and
the response was to reword the iteration plan rather than fix the tool. That incident is
why `CLAUDE.md:159-169` carries the mirror rule at all.

In this one commit:

1. **Delete `ac-fidelity-check.sh`** (464 LOC each) in all four copies. Nothing survives it:
   not the evidence heuristics, not the `[verified: <YYYY-MM-DD>, …]` requirement on `live_*`
   ACs, not the non-execution wording scan, not the AC-folding machinery iter-154 built to
   feed them. Delete `tools/tests/ac-fidelity-check/` entirely and
   `crates/xtask/tests/ac_fidelity_check.rs` with it.

2. **Delete `claims-vs-code.sh`** in all four copies (188 LOC each).

3. **Edit its call sites** — the six in the table above, plus:
   - `run-iteration.sh:563-564` — the comment introducing both scripts.
   - `run-iteration.sh:202` (`PROMPT_IMPLEMENT`, step 6) and `:231` (`PROMPT_REVIEW`,
     step 0) — both instruct an agent to run `cargo run -p xtask -- check-iteration-ready`,
     deleted in 162a, and `:231` additionally tells it to append a claims report to the PR
     body. **162a deliberately left these to this iteration** because fixing them means
     touching the mirrored pair. Rewrite both.
   - `tools/ralph-loop/README.md:16` (the `claims-vs-code.sh` table row), `:18`, and the
     `check-discipline-regression` references at `:9`, `:32`, `:47`.
   - Optional, and now convenient: delete the dead probe blocks at
     `run-iteration.sh:587-596` and `:598-607`, which log
     `"xtask has no check-dead-primitives — skipped"` forever after 162a.

4. **Relax `check-discipline-regression` to its mirror-sync half in the same commit.**
   This is forced, not a preference. The check's replay half shells
   `run-iteration.sh --replay 61v/61t`, and `run-iteration.sh:83` calls `claims-vs-code.sh`
   unconditionally — so the moment step 2 lands, the replay cannot run at all. Independently,
   the pinned `61v=FAIL` baseline cannot survive step 1: 61v fails on the evidence
   heuristics, which step 1 deletes. Drop the replay
   (`check_discipline_regression.rs:29,45,111` and the `--replay` code path at
   `run-iteration.sh:70-96`), keep the four-way mirror diff, and run
   `cargo run -p xtask -- check-discipline-regression` before committing.
   **That green is the 4-of-4 proof, and it is the entire reason this phase is ordered.**

**Do not invert steps 2 and 4.** Deleting `claims-vs-code.sh` while the replay is still
wired makes `check-discipline-regression` fail on its own removal path — the gate breaks
while executing the change that removes it, and you lose the only mirror check at exactly
the moment you need it.

### 3b — only then delete `check-discipline-regression`

`crates/xtask/src/check_discipline_regression.rs` (208), `main.rs` wiring
(`mod` at `:4`, variant at `:39`, match arm at `:81`), `ci.yml:144-149`,
`CLAUDE.md:145,151,157,161`, `CONTRIBUTING.md:245`,
`tools/ralph-loop/README.md:9,18,32,47`, `crates/xtask/tests/bin_exit_codes.rs:184`.

The four-copy mirror obligation **survives this iteration as a manual discipline**. Say so
in `CLAUDE.md` rather than pretending it went away: the mirror rule at `CLAUDE.md:159-169`
stays, minus its *"`check-discipline-regression` catches drift"* clause. This is a real
regression in safety and the plan should not hide it — the mitigation is that after this
iteration there are **no** copies of either script left to mirror; `run-iteration.sh` and
`ralph.workflow.js` remain mirrored, and their drift risk is unchanged.

## Why nothing replaces it

This plan originally kept one rule and weighed two candidates (a `live-sweep --emit` run-log
rule, and the non-execution wording scan). Both are dropped. The deciding facts, in the order
they matter:

**1. The artifact has no reader.** The repository owner, 2026-08-14: *"I lost the overview how
it works and I never actually looked at the ACs anyway."* Every rule in this family polices the
accuracy of a record that no human reads. Ticked ACs are not consumed downstream either — the
next iteration's agent reads the diff, the PR and the previous plan's prose, not its checkboxes.

**2. It has a negative net record, now including an induced falsification.** 28 commits whose
entire content is rewording an AC so the gate stops firing ([[gate-forensics]] §5b). One
demonstrated catch (PR #188), found by a human who unticked the ACs at `6273773`; the gate was
taught the wording afterwards. Across the 158–161 batch: **zero catches, seven false positives**
(five in 158, two in 160), and one case where the gate actively corrupted the record — five
iter-158 ACs merged carrying `[x]` *and* `[deferred — new plan: …]` simultaneously, because a
review agent needed the gate green. That contradiction was removed post-merge at `7092fba`. A
gate whose failure mode is *making the plan lie* cannot be justified by the plan's accuracy.

**3. The real problem it was introduced for is solved elsewhere, and better.** The original
motivation was that an iteration would leave planned work undone and the next would build on
top without checking. The defence against that is the loop's **next-plan adaptation** step
(`ralph.workflow.js`, review prompt step 4), which fired three times in the 158–161 batch —
159's plan gained notes on 158's landed launch fixes, 160's was adapted for 159's
`--with-network` change and `network.rs` line shifts. That is a prompt, not a gate, and it
works. Strengthening it is [[#Theme D — replace the gate with the thing that actually worked]].

**4. What replaced the `live_*` half is a habit, not code.** The 158–161 batch required a real
`FF_RDP_LIVE_TESTS=1 cargo run -p xtask -- live-sweep` in every PR body. That caught three
genuine failures on iter-160's branch *before the PR opened*, one of which would have broken
every cross-origin frame click. It reads execution rather than prose, which is exactly what
Option A promised, and it needs no `--emit`, no log format and no gate.

**5. Option A would not have caught iter-153 anyway** — the case cited for it. That AC's
`[verified: 2026-08-13, … 3 passed / 0 failed]` annotation was **truthful**: a real isolated run
that really passed, certifying a feature that fails under contention. An isolated run emits a
log too. A run-log rule beats the annotation only if the log pins the commit SHA, the env gates
and full-sweep completeness — at which point it is a worse-ergonomics version of point 4.

**ACs are load-bearing as instructions, not as records.** They remain the specification an
implementing agent works against, and plans keep them. What ends is machine-checking them
afterwards. Ticks become advisory; trust the diff, the PR and the sweep.

## What survives

After 162a + 162b, the gates that remain are:

| Kept | Reads |
|---|---|
| `live-sweep` | test execution |
| `check-live-test-layout` | test file layout |
| `check-dogfood-script` + `lint-dogfood-script.sh` | a runnable command sequence |
| `check-firefox-refs` (local) | the Firefox checkout — ground truth outside this repo |
| `check-source-invariants` | product source |
| `check-iteration-plan` | frontmatter schema |
| `check-actor-kb-sync` (local) | which files a diff touched |
| `find-iteration-plan` | a branch name (a resolver, not a gate) |

**Every remaining gate reads something other than acceptance-criteria prose.** That was the
finding in [[analysis-2026-08-13-what-ff-rdp-became]] §5; after this iteration there is no
exception to it. `check-iteration-plan` still reads a plan, but only its frontmatter schema —
it makes plans findable and well-formed, and asserts nothing about whether the work was done.

## Theme D — replace the gate with the thing that actually worked

Removing the gate without strengthening its replacement is how the original problem returns.
Two prompt-level changes, no new code:

1. **Sharpen next-plan adaptation.** `ralph.workflow.js`'s review prompt step 4 currently says
   *"adapt its scope if needed"* — advisory and vague. Rewrite it as an explicit sweep: list
   every AC left unticked, every deferral, and every finding the implement agent flagged; for
   each, either fold it into the next plan or file a new one, and say in the PR body which.
   Mirror into `run-iteration.sh`'s `PROMPT_REVIEW`. This is the mechanism that caught 158→159
   and 159→160 in the batch; make it a checklist rather than a sentence.
2. **Make the per-PR live sweep standing policy.** CLAUDE.md gains what the 158–161 batch was
   told ad hoc: an iteration touching product source pastes a real
   `FF_RDP_LIVE_TESTS=1 cargo run -p xtask -- live-sweep` result into its PR body, quoting the
   env gates that produced it (already required by the CLAUDE.md live-tests section as of
   `9398086`). This is the honest half of what `ac-fidelity`'s `live_*` rule pretended to do.

Also update the AC-checkbox convention in `CLAUDE.md` (currently the `[verified: …]` /
`[deferred — new plan: …]` / `[allow-ac-wording: …]` machinery): keep "name the test and the
post-condition" as authoring guidance, and state plainly that nothing checks tick state — ACs
are the spec you implement against, not a record anything verifies.

## Out of scope

- **Everything in [[iteration-162a-discipline-removal-safe-phases]]** — the aggregator, the
  zero-catch subcommand deletions, the CI consolidation. Land 162a first; this plan assumes
  its end state (9 xtask subcommands, 3 CI discipline steps).
- **The four product defects** in [[analysis-2026-08-13-what-ff-rdp-became]] §3 —
  iterations 158–161, which must have merged before this starts.
- **Replacing the loop's advisory PR-body reporting.** `claims-vs-code.sh`'s output
  currently gets appended to the PR body. It goes away with no replacement; that is
  intended, not an omission to fix later.

## Expected outcome

| Measurable | After 162a | After 162b |
|---|---|---|
| In-repo discipline LOC | ~8,200 | **~6,500** |
| Removed by this iteration (in-repo) | — | **~1,700 (≈21%)** |
| Removed by this iteration (`~/.claude/skills/`) | — | **~1,300** |
| Cumulative removal, 162a + 162b | — | **~5,200 (≈44% of 11,706)** |
| xtask subcommands | 9 | **8** |
| `cargo run -p xtask --` steps in the CI `discipline` job | 3 | **2** |
| Copies of `ac-fidelity-check.sh` | 4 × 464 | **0** |
| Copies of `claims-vs-code.sh` | 4 × 188 | **0** |
| Gates that read acceptance-criteria text | 2 | **0** |
| New code added | — | **0** |

In-repo arithmetic (`wc -l`): 2 × 464 (ac-fidelity mirrors) + 2 × 188 (claims mirrors) + 208
(`check_discipline_regression.rs`), plus `tools/tests/ac-fidelity-check/` and
`crates/xtask/tests/ac_fidelity_check.rs` ≈ **1,700**. Out-of-repo: the same two scripts in
the two `~/.claude/skills/*/scripts/` copies = 2 × 652 = **1,304**.

## Acceptance Criteria [0/10]

Every grep-based AC below must assert its **scanned set is non-empty before** asserting the
count. A wrong path or glob satisfies "no matches" trivially — the bug CI caught in iter-158,
where `unit_158_source_scan_covers_the_live_suites` matched a literal `tests/live` that never
appears in a Windows path and so scanned zero files. Match on path components, not substrings.

- [ ] `unit_162b_both_scripts_absent_from_all_four`: neither `ac-fidelity-check.sh` nor `claims-vs-code.sh` exists in `~/.claude/skills/ralph-loop/scripts/`, `~/.claude/skills/new-ralph-loop/scripts/`, `tools/ralph-loop/scripts/` or `tools/new-ralph-loop/scripts/`; the test first asserts all four directories exist and are non-empty, so a mistyped path cannot pass it.
- [ ] `check-discipline-regression` exits 0 on the Phase-3a commit — four-way mirror in sync, replay path removed — and the captured output is pasted into the PR body. This green is the 3-of-4-edit guard and must be recorded before Phase 3b deletes the check.
- [ ] `crates/xtask/tests/ac_fidelity_check.rs` and `tools/tests/ac-fidelity-check/` are deleted, and `cargo test --workspace -q` passes without them.
- [ ] `unit_162b_xtask_help_lists_eight`: `cargo run -q -p xtask -- --help` names exactly `check-iteration-plan`, `check-firefox-refs`, `check-actor-kb-sync`, `check-live-test-layout`, `check-dogfood-script`, `check-source-invariants`, `find-iteration-plan`, `live-sweep` — 8 subcommands — and `check-discipline-regression` is absent.
- [ ] `ci_162b_discipline_job_two_xtask_steps`: `grep -c 'cargo run -p xtask --' .github/workflows/ci.yml` returns 2, and both names — `check-live-test-layout`, `check-source-invariants` — resolve in `cargo run -q -p xtask -- --help`.
- [ ] `unit_162b_no_dangling_script_references`: `grep -rnE 'ac-fidelity|claims-vs-code|check-(iteration-ready|discipline-regression)' tools ~/.claude/skills/{ralph-loop,new-ralph-loop,create-pr} .github CLAUDE.md CONTRIBUTING.md crates` returns no matches, after asserting the scan covered a non-zero file count — covering `run-iteration.sh:83,89,202,231,563,614,623`, `ralph.workflow.js:152,153`, and `tools/ralph-loop/README.md:9,16,18,32,47`.
- [ ] `smoke_162b_loop_still_starts`: `node ~/.claude/skills/new-ralph-loop/scripts/smoke.workflow.js` exits 0, and the review-phase prompt it emits contains no filesystem path ending in `ac-fidelity-check.sh` or `claims-vs-code.sh`.
- [ ] `cargo build -p xtask`, `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace -q` each exit 0, with no unused-import or dead-code warning in `crates/xtask`.
- [ ] **Theme D landed:** `ralph.workflow.js`'s review prompt step 4 and `run-iteration.sh`'s `PROMPT_REVIEW` both instruct an explicit carry-over sweep (enumerate unticked ACs, deferrals and flagged findings; fold each into the next plan or file a new one; state which in the PR body), identically in both mirrored copies; and `CLAUDE.md` states the standing per-PR live-sweep requirement.
- [ ] `CLAUDE.md`'s AC-checkbox convention is rewritten: ACs are authoring guidance (name the test and the post-condition), **nothing checks tick state**, and the `[verified: …]` / `[deferred — new plan: …]` / `[allow-ac-wording: …]` machinery is gone. The four-copy mirror rule survives for `run-iteration.sh` and `ralph.workflow.js`, stated as a manual discipline with its `check-discipline-regression` clause removed.

## Notes

- **The `--emit` decision is closed: no `--emit`, no rule.** It was a choice between two
  surviving rules; there is no surviving rule. The four sweep lines it was to be decided on,
  with the env gates that produced them, are kept here because they are the record of the habit
  that replaces the gate: 158 `executed=221 skipped=0 preexisting=9 total=230` (219/2, both
  gates); 159 `executed=237 skipped=0 preexisting=0 total=237` (225/3, both gates); 160
  `executed=209 skipped=32 preexisting=8 total=249` (209/0, `FF_RDP_LIVE_TESTS` only); 161
  `executed=225 skipped=32 total=257` (225/0, `FF_RDP_LIVE_TESTS` only). `executed=N` swings,
  but from tests being added and from 160/161 running one gate instead of two — not classifier
  drift. [[iteration-157-live-sweep-classifier-drift]] is `obsolete`. If a run-log gate is ever
  revisited, the one thing this batch proved it would need is a record of **which env gates were
  set**: 160's `0 failed` over a 32-test-smaller corpus reads better than 159's `3 failed` and
  is worth less.

- **[[iteration-163-ac-fidelity-reads-only-the-first-line]] is `obsolete`** (2026-08-14) — it
  repaired heuristics this plan deletes. Nothing carries forward now that no rule survives; its
  reproduction data remains the measured case for deleting rather than repairing them.

- **Consumers outside this repo** (found 2026-08-14 by recursive search of `~/devel`). Handle
  these or the deletion breaks them:
  - `magnificient/xtask/src/main.rs:196-216` shells out to `ac-fidelity-check.sh` in
    `tools/ralph-loop/scripts/` or `~/.claude/skills/ralph-loop/scripts/`, and returns
    `(false, …)` — a **hard failure** — when it is not found. Its `ac-fidelity` subcommand must
    go **before** the script is deleted.
  - `hyalo/crates/xtask/src/ac_fidelity.rs` (571 LOC) is an independent Rust reimplementation
    wired into CI at `.github/workflows/quality-gates.yml:21`. It keyword-matches AC prose
    against the whole workspace. Measured 2026-08-14: it passes all 191 plans, and it passes a
    ticked AC reading *"zzqqxx_nonexistent_widget_flurb resolves the frobnicator quuxbaz"* — it
    cannot fail. Removed separately, in that repo.
  - `comparis/neon/kb/refactor-*.md` mention the gate in prose only. Nothing to do.

- **Do not let this plan's own grep ACs pass vacuously.** Several ACs assert a `grep -c` count
  or "no matches". A wrong path or glob satisfies "no matches" trivially — which is the bug CI
  caught in iter-158, where `unit_158_source_scan_covers_the_live_suites` matched a literal
  `tests/live` that never appears in a Windows path and so scanned zero files. Of the repo's
  source-scan tests only `iter_158_harness_honesty.rs` asserts a non-empty corpus. Each grep AC
  here must assert the scanned set is non-empty *before* asserting the count, and match on path
  components rather than substrings.

- **The safety regression this iteration accepts.** After 3b nothing detects mirror drift
  between `~/.claude/skills/*/scripts/` and `tools/*/scripts/`. That is a real loss, and
  the 2026-08-13 analysis is what justifies it: the drift gate's only "catch" was a defect
  *in the gate machinery*, not in the product, and it exists to guard an obligation created
  by the machinery's own deployment shape. Removing the machinery removes most of the
  obligation — one ~120-line script instead of two totalling 652.

- **Why the ordering is stated three times.** 3a-before-3b, four-copies-together, and
  step-2-before-step-4 are each an ordering someone reading only the file list would get
  wrong, and each failure is silent: a 3-of-4 edit produces a false gate failure on the
  *next* iteration (iter-140's actual history), and deleting `claims-vs-code.sh` first
  makes `check-discipline-regression` fail on its own removal path.

- **What this iteration does not claim.** It does not claim AC checking is worthless. The
  replay experiment in `kb/research/iter-66-ac-fidelity-replay-iter61w.md` is real: the
  strengthened script would have rejected three of four false **security** ACs in iter-61w.
  But the version live at the time passed all four — it proves the concept, not the
  deployed gate. The claim is narrower: this *implementation*, over 2.5 months, produced
  two qualified catches and 28 reword commits, and was green on 2026-08-13 while four
  user-visible defects shipped.

## Links

[[iteration-162a-discipline-removal-safe-phases]] ·
[[analysis-2026-08-13-what-ff-rdp-became]] · [[step-back-2026-08-13]] ·
[[decision-log]] (DEC-030, DEC-031) · [[iteration-61z-discipline-skill-integration]] ·
[[iteration-61aa-claim-miss-hard-gate]] ·
[[iteration-66-backfill-iter61w-security-tests]] ·
[[iteration-153-launch-replace-double-envelope]] ·
[[iteration-154-ac-fidelity-evidence]] · [[iteration-155-live-skip-reports-green]] ·
[[iteration-156-ac-fidelity-names-its-test]] ·
[[iteration-157-live-sweep-classifier-drift]] ·
[[iteration-161-eval-and-flag-strictness]]
