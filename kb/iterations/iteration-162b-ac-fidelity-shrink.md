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

  # 1. Phase 3a: the four-way edit lands while the mirror gate is alive.
  cargo run -p xtask -- check-discipline-regression
  # → exit 0.  All four copies of ac-fidelity-check.sh identical, claims-vs-code.sh
  #   absent from all four.  THIS GREEN IS THE 4-OF-4 PROOF — capture it in the PR body.
  md5 -q ~/.claude/skills/ralph-loop/scripts/ac-fidelity-check.sh \
         ~/.claude/skills/new-ralph-loop/scripts/ac-fidelity-check.sh \
         tools/ralph-loop/scripts/ac-fidelity-check.sh \
         tools/new-ralph-loop/scripts/ac-fidelity-check.sh
  # → four identical hashes.
  ls tools/ralph-loop/scripts/claims-vs-code.sh tools/new-ralph-loop/scripts/claims-vs-code.sh
  # → both "No such file or directory".

  # 2. The shrunk gate still rejects a self-incriminating AC.
  bash tools/ralph-loop/scripts/ac-fidelity-check.sh --plan tools/tests/ac-fidelity-check/unrun-live-ac.md --range HEAD~1..HEAD
  # → exit 1, naming the offending AC.
  bash tools/ralph-loop/scripts/ac-fidelity-check.sh --plan tools/tests/ac-fidelity-check/allowed-wording-ac.md --range HEAD~1..HEAD
  # → exit 0.

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
title: "Iteration 162b: shrink ac-fidelity-check to one rule and delete claims-vs-code"
type: iteration
tags:
  - iteration
---

# Iteration 162b: shrink ac-fidelity-check to one rule and delete claims-vs-code

Phase 3 of the removal described in [[analysis-2026-08-13-what-ff-rdp-became]] §5,
split out from [[iteration-162a-discipline-removal-safe-phases]]. Also carries the
`live-sweep --emit` decision, which is only answerable once this runs.

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

1. **Shrink `ac-fidelity-check.sh` to a single rule** in all four copies. Which rule is the
   open decision — see [[#The `--emit` decision]]. Whichever way it goes, the following are
   deleted: the test-slug/symbol-in-diff evidence heuristics, the
   `[deferred — new plan: …]` accept that exists only to escape them, and the
   `[verified: <YYYY-MM-DD>, …]` requirement on `live_*` ACs.
   Keep the AC-folding machinery (multi-line continuation handling, `ac-fidelity-check.sh:90-145`)
   — every candidate rule needs it; that fold is what iter-154 built after the
   iteration-151 confession hid on a continuation line.
   Prune `tools/tests/ac-fidelity-check/` to the fixtures the surviving rule exercises and
   trim `crates/xtask/tests/ac_fidelity_check.rs` to match. The fixtures that stay under
   the text-rule option are `unrun-live-ac.md`, `iter151-prefix-ac.md`,
   `allowed-wording-ac.md`, `blank-line-continuation-ac.md`; `deferred-ac.md`,
   `deferral-mention-ac.md`, `evidenced-live-ac.md` and `unevidenced-live-ac.md` go with
   the heuristics they test.

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
   heuristics, which step 1 removes. Drop the replay
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
iteration there is one script left to mirror instead of two, at ~120 LOC instead of 652.

## The `--emit` decision

**Decide this here, with evidence in hand. Do not carry it forward again.**

By the time this iteration runs, `live-sweep` will have been run by hand four times — once
per iteration 158, 159, 160, 161 — with each `LIVE_SWEEP_SUMMARY executed=N skipped=M
total=T` line pasted into its PR body. That is the evidence base that did not exist when
[[analysis-2026-08-13-what-ff-rdp-became]] recommended deferring.

The single surviving `ac-fidelity` rule is one of two, and they are not interchangeable:

**Option A — the run-log rule.** *"A ticked AC naming a `live_*` test names a test that
resolves in the run log with status `ok`."* This is the honest rule: it reads execution,
not prose, and it is the only one in the family that could have caught iter-153. **It
requires `live-sweep --emit` to exist** — a machine-readable per-test run log
(test slug, status, timestamp, commit SHA) that the gate can query. `live-sweep` today
prints a summary line and nothing per-test.
*Cost*: `--emit` plus a log-reading rule ≈ 100 LOC, replacing the ~1,600 LOC of
plan-and-diff heuristics being deleted (4 copies × 464 LOC = 1,856 today; ~120 × 4 = 480
under the text rule; ~100 in xtask plus a thin reader under this one).
*Risk*: `live-sweep` is 845 LOC and was one day old when this plan was written;
[[iteration-157-live-sweep-classifier-drift]] already files a bug against its classifier.
Building a gate on top of it is how `check-pre-fix-repro` happened — 1,192 LOC on a
plausible theory, two iterations of flap-fixing, zero catches.

**Option B — the text rule.** *"A ticked AC whose own folded text admits the work was not
carried out fails outright."* No new machinery; keeps the current heuristic family's one
non-gameable member. It is the only rule that cannot be satisfied by moving a backticked
symbol onto the AC's first line.
*Cost*: ~120 LOC × 4 copies, all of it already written.
*Weakness, stated plainly*: its one demonstrated catch (PR #188, two ACs reading
*"implemented and compiled … not exercised end-to-end in this session's time budget"*) was
found by a **human**, who unticked them at `6273773`; the gate was taught the wording
afterwards, in iter-154. It catches a confession, not a lie.

**What the implementer must do:** pick one, and record in this plan's Notes *which way you
went and why*, citing the four `LIVE_SWEEP_SUMMARY` lines from 158–161. Two specific
questions those lines answer: did `executed=N` stay stable across the four runs (if it
swings, the classifier bug in 157 is real and Option A rests on sand), and did the sweep
actually get run all four times (if it did not, no gate reading its log has anything to
read). **A "keep both" answer is not available** — the point of this iteration is that the
gate has one rule.

If Option A is chosen, `--emit` is built **in this iteration**, not deferred to another:
deferring it is what leaves the text rule in place indefinitely under the label
"temporary."

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
| `ac-fidelity-check.sh`, one rule | *see the decision above* |

Every one of these except the last reads something other than acceptance-criteria prose.
That was the finding in [[analysis-2026-08-13-what-ff-rdp-became]] §5, and the decision
above determines whether the exception survives as an exception or joins the list.

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
| In-repo discipline LOC | ~8,200 | ~6,900 |
| Removed by this iteration (in-repo) | — | **~1,270 (≈15%)** |
| Removed by this iteration (`~/.claude/skills/`) | — | **~1,150** |
| Cumulative removal, 162a + 162b | — | **~4,770 (≈41% of 11,706)** |
| xtask subcommands | 9 | **8** |
| `cargo run -p xtask --` steps in the CI `discipline` job | 3 | **2** |
| Copies of `ac-fidelity-check.sh` | 4 × 464 | 4 × ~120 |
| Copies of `claims-vs-code.sh` | 4 × 188 | **0** |
| Gates that read acceptance-criteria text | 2 | 1, one rule |

In-repo arithmetic (verified with `wc -l`): 2 × 188 (claims mirrors) + 2 × ~344
(ac-fidelity shrink in the mirrors) + 208 (`check_discipline_regression.rs`) ≈ **1,272**.
Out-of-repo: the same two script edits in the two `~/.claude/skills/*/scripts/` copies ≈
1,150. If Option A is chosen, add ~100 LOC back for `live-sweep --emit`.

## Acceptance Criteria [0/11]

- [ ] `unit_162b_ac_fidelity_four_copies_identical`: `md5` of `ac-fidelity-check.sh` is byte-identical across `~/.claude/skills/ralph-loop/scripts/`, `~/.claude/skills/new-ralph-loop/scripts/`, `tools/ralph-loop/scripts/` and `tools/new-ralph-loop/scripts/`; the file is ≤ 150 lines in each; and `claims-vs-code.sh` is absent from all four directories.
- [ ] `check-discipline-regression` exits 0 on the Phase-3a commit — four-way mirror in sync, replay path removed — and the captured output is pasted into the PR body. This green is the 3-of-4-edit guard and must be recorded before Phase 3b deletes the check.
- [ ] `ac_fidelity_check::*`: the pruned `crates/xtask/tests/ac_fidelity_check.rs` passes, asserting the surviving rule fires on its designated failing fixture and stays silent on its designated passing one, and that a plan whose ticked AC names a test absent from the diff now exits 0 — the evidence heuristics are gone by design.
- [ ] `unit_162b_xtask_help_lists_eight`: `cargo run -q -p xtask -- --help` names exactly `check-iteration-plan`, `check-firefox-refs`, `check-actor-kb-sync`, `check-live-test-layout`, `check-dogfood-script`, `check-source-invariants`, `find-iteration-plan`, `live-sweep` — 8 subcommands — and `check-discipline-regression` is absent.
- [ ] `ci_162b_discipline_job_two_xtask_steps`: `grep -c 'cargo run -p xtask --' .github/workflows/ci.yml` returns 2, and both names — `check-live-test-layout`, `check-source-invariants` — resolve in `cargo run -q -p xtask -- --help`.
- [ ] `unit_162b_no_dangling_script_references`: `grep -rnE 'claims-vs-code|check-(iteration-ready|discipline-regression)' tools ~/.claude/skills/{ralph-loop,new-ralph-loop,create-pr} .github CLAUDE.md CONTRIBUTING.md crates` returns no matches — covering `run-iteration.sh:83,89,202,231,563,614,623`, `ralph.workflow.js:152`, and `tools/ralph-loop/README.md:9,16,18,32,47`.
- [ ] `smoke_162b_loop_still_starts`: `node ~/.claude/skills/new-ralph-loop/scripts/smoke.workflow.js` exits 0, and the review-phase prompt it emits contains no filesystem path ending in `claims-vs-code.sh`.
- [ ] `cargo build -p xtask`, `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace -q` each exit 0, with no unused-import or dead-code warning in `crates/xtask`.
- [ ] The `--emit` decision is recorded in this plan's `## Notes` naming the option chosen, the four `LIVE_SWEEP_SUMMARY executed=N skipped=M total=T` lines from iterations 158-161 that informed it, and whether `executed=N` held steady across them.
- [ ] If Option A was chosen: `live_162b_sweep_emits_run_log` asserts `cargo run -p xtask -- live-sweep --emit <path>` writes one record per test with slug, status and commit SHA, and that `ac-fidelity-check.sh` exits 1 for a ticked `live_*` AC whose named test carries a status other than `ok` in that log. If Option B was chosen: this AC is marked `[deferred — new plan: <path>]` with the follow-up plan filed before this PR merges.
- [ ] `CLAUDE.md` retains the four-copy mirror rule with the `check-discipline-regression` clause removed and an explicit statement that the mirror is now maintained by hand; `grep -c 'ac-fidelity-check.sh' CLAUDE.md` returns a non-zero count and no line claims automated drift detection.

## Notes

- **Record the `--emit` decision here.** Leave this bullet in place and fill it in during
  implementation: which option, the four `LIVE_SWEEP_SUMMARY` lines, and the reasoning.
  An empty bullet at merge time means the decision was skipped, which is the failure mode
  this iteration exists to end.

- **Evidence available at start (2026-08-14) — inputs to that decision, not the decision.**
  The batch ran and all four sweeps happened, each pasted into its PR body. With the env gates
  that produced them: 158 `executed=221 skipped=0 preexisting=9 total=230` (219/2, both gates);
  159 `executed=237 skipped=0 preexisting=0 total=237` (225/3, both gates); 160 `executed=209
  skipped=32 preexisting=8 total=249` (209/0, `FF_RDP_LIVE_TESTS` only); 161 `executed=225
  skipped=32 total=257` (225/0, `FF_RDP_LIVE_TESTS` only). Three findings bear on the choice:
  - **`executed=N` did not hold steady, but not from classifier drift.** Totals grew as each
    iteration added tests, and 160/161 ran one gate instead of two. The plan's "if it swings,
    Option A rests on sand" inference does not fire on this data.
  - **The stated risk is stale.** [[iteration-157-live-sweep-classifier-drift]], cited above as
    an open bug against the classifier, is `status: obsolete`.
  - **Option A's precondition became true during the batch.** iter-158's panic-flip is what
    makes a run log's `status ok` mean *reached Firefox* — before it, 152 call sites returned
    early on an unset gate and libtest scored that `ok`, so a log-reading rule would have
    inherited the false green it exists to prevent. If Option A is chosen, `--emit` must record
    **which env gates were set**; 160-vs-159 is the proof that a summary without them invites
    reading a shrunken corpus as an improvement.

- **[[iteration-163-ac-fidelity-reads-only-the-first-line]] was marked `obsolete` for this
  plan** (2026-08-14) — it repaired the evidence heuristics Phase 3a deletes. One fact carries
  forward: the slug regex `\b(live|test|bench)_[a-z0-9_]+` does not match `unit_*`, the prefix
  CLAUDE.md's AC convention produces for non-live tests. If the surviving rule checks that a
  named test exists, it must recognise `unit_*` or it checks nothing for most ACs.

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
