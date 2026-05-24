---
title: "Iteration 61z: Discipline skill integration (Phase 2 claims-vs-code + AC fidelity gate)"
type: iteration
date: 2026-05-24
status: in-review
branch: iter-61z/discipline-skill-integration
depends_on:
  - iteration-61y-iteration-discipline-tooling
tags:
  - iteration
  - tooling
  - process
  - ralph-loop
  - stability-roadmap
first_call_sites: []
dogfood_path: |
  # Replay iter-61v's branch through the updated skill
  cd $(mktemp -d) && git clone -b iter-61v/navigate-screenshot-completion .../ff-rdp .
  $HOME/.claude/skills/ralph-loop/scripts/run-iteration.sh --replay iter-61v
  # Expected: PR description body contains "## Claims vs code" with at least
  # three ❌ rows for chromeContext, RdpError::Navigation, dom-interactive;
  # the merge-gate AC-fidelity step fails with the RdpError::Navigation
  # ticked-but-no-evidence finding.
---

# Iteration 61z: Discipline skill integration

iter-61y landed the Rust side of the discipline tooling (`cargo xtask` checks,
pre-commit hook, CI job, plan template, CLAUDE.md section). Its themes D and
E — Phase 2 "Claims vs code" PR-description diff and the AC fidelity check at
the merge gate — were deferred because they edit `~/.claude/skills/ralph-loop/`,
which a cmux child workspace cannot touch. The cmux child runs against a
worktree of the project; it has no write access to the skill directory.

This iteration closes those two themes by editing the skill directly (outside
the cmux loop, in a regular Claude session or by hand). The skill update is
versioned in `~/.claude/skills/ralph-loop/SKILL.md` (or wherever the skill's
prompt source lives) and tested by replaying the iter-61v branch through it.

This is the kind of iteration the postmortem said we should be filing
proactively: small, scoped to closing a known deferral, named explicitly, and
written *before* the parent PR merges. The fact that we're filing it
*after* iter-61y merged is itself an instance of the failure mode the
discipline rules are meant to prevent — file the postmortem-amendment lesson
in [[iter-61m-61s-postmortem-loose-ends]] accordingly.

## Themes

- **A — Phase 2 "Claims vs code" PR-description section.** Parse iteration-branch commit messages for verb forms ("adds X", "implements Y", "wires Z", "fixes <symbol>", "closes #<n>"). For each, grep the diff for evidence. Emit a markdown section in the PR description listing ✅/❌ per claim. Exit non-zero if any ❌ remain unannotated.
- **B — AC fidelity check at merge gate.** Parse the iteration plan's `## Acceptance Criteria` block. Each ticked checkbox must be paired with (a) a test function whose name matches a slug in the AC text, (b) a grep pattern derived from the AC text matching the diff, or (c) an explicit `[deferred — new plan: <path>]` annotation. Fail the merge step otherwise.
- **C — Replay-test harness.** A `run-iteration.sh --replay <iter>` mode that runs themes A and B against an already-merged branch, comparing the output to a captured baseline. Used both as a unit test for the skill and as a CI smoke check against iter-61v.

## Tasks

### A. Phase 2 Claims-vs-code section

- [x] In `~/.claude/skills/ralph-loop/scripts/`, add `claims-vs-code.sh` that:
  1. Reads the iteration's branch name from `state.json` (or argv).
  2. `git log --format="%s%n%b" main..<branch>` → extract claim-bearing sentences. Regex: `(?:adds|implements|wires|fixes|closes)\s+([A-Z][A-Za-z0-9_:.]+|#\d+)`.
  3. For each claim, run `git diff main..<branch> -- crates/` and `rg` the captured symbol against the diff. Match: ✅. No match: ❌.
  4. Emit a markdown section to stdout:
     ```
     ## Claims vs code
     <generated at YYYY-MM-DDTHH:MM:SSZ by ralph-loop>
     - "adds DescriptorFront::get_process" → ✅ crates/ff-rdp-core/src/fronts/descriptor.rs:42
     - "implements RdpError::Navigation" → ❌ no match in diff
     ```
  5. Exit code: 0 if all ✅ (or every ❌ has a `// allow-claim-miss: <reason>` line nearby); 1 otherwise.
- [x] Wire `claims-vs-code.sh` into `run-iteration.sh` between Phase 1 completion and `gh pr create`. The script runs in `check_iteration_discipline` (before Phase 2 starts); its output is written to `$RALPH_CACHE_DIR/iter-N-claims.md`, and the Phase 2 PROMPT_REVIEW instructs the agent to append it to the PR body via `gh pr edit`.
- [x] If the script exits 1, surface the failure in the cmux pane log [deferred — new plan: kb/iterations/iteration-61aa-claim-miss-hard-gate.md]. Currently claims-vs-code is advisory (a WARN log + report file); ac-fidelity-check is the hard gate. Promoting claim misses to a hard fail risks blocking iterations whose commit-message style differs from the heuristic and is a separate decision.

### B. AC fidelity check

- [x] Add `~/.claude/skills/ralph-loop/scripts/ac-fidelity-check.sh`:
  1. Parse `## Acceptance Criteria` block from the iteration plan file.
  2. For each line matching `- \[(x| )\] (.+)`:
     - If `[ ]` (unticked): no check needed.
     - If `[x]`: look for evidence. Heuristics, in order:
       - The AC text contains a `live_*` or `test_*` slug → grep for `fn <slug>` in the diff.
       - The AC text contains a backtick-quoted symbol → grep for that symbol in the diff.
       - The AC line includes `[deferred — new plan: <path>]` → accept if the referenced plan exists.
     - Otherwise: ❌.
  3. Exit code 0 if all ticked ACs have evidence, 1 otherwise.
- [x] Run this script in `run-iteration.sh` immediately before `gh pr merge`. Implemented as part of `check_iteration_discipline` (runs after Phase 1, before Phase 2 review/merge). Any ❌ returns non-zero and `run_phase` aborts the iteration; the Phase 2 review prompt would have been responsible for the actual merge step, so a `check_iteration_discipline` failure prevents the merge from being kicked off in the first place.
- [x] The script is conservative: prefer false ✅ over false ❌, because a false-fail blocks all merges. Whitelisted build-process ACs (`cargo fmt && cargo clippy ...`). Documented in `kb/decision-log.md` §"2026-05-24: Discipline skill integration".

### C. Replay harness

- [x] Add `~/.claude/skills/ralph-loop/scripts/run-iteration.sh --replay <iter-id>`:
  - Skips Phase 1 (implement) entirely.
  - Reads the iteration plan and the merged branch from `git log` (find the `iter-<id>/` branch's last commit on `main`).
  - Runs themes A and B against the merged diff.
  - Writes the output to `<cache-dir>/replay-<iter-id>.txt`.
- [x] Capture a baseline: `replay iter-61v` produces (verified):
  - ❌ for `SCREENSHOT_JS_PROGRAM` claim (the constant was deleted but the commit message references it).
  - ❌ for `dom-interactive` claim (no occurrence in the code diff — only in the plan markdown, which we exclude).
  - ❌ for `chrome-context` claim (same — only in the plan).
  - Note: the iter-61v *plan* honestly left `RdpError::Navigation` unticked at merge, so ac-fidelity-check passes on the as-shipped plan. We verified the negative case with a synthetic broken-plan fixture: ticking the `live_navigate_neterror_dns_fail` AC produces a ❌ in ac-fidelity-check.
- [x] Add this as a `cargo xtask check-discipline-regression` target so the baseline doesn't regress. Pinned: `--replay 61v` exits 1, `--replay 61t` exits 0.

## Acceptance Criteria [6/6]

- [x] `claims-vs-code.sh` produces a "Claims vs code" markdown section against the iter-61v branch with at least 3 ❌ rows. Verified: rows are `SCREENSHOT_JS_PROGRAM`, `dom-interactive`, `chrome-context` (the original plan predicted `chromeContext`, `RdpError::Navigation`, `dom-interactive` — the actual set differs because iter-61v's commit messages didn't use the `adds X` verb form for `RdpError::Navigation`; that token came in iter-61x's cleanup pass).
- [x] `ac-fidelity-check.sh` correctly distinguishes a ticked-without-evidence AC from a deferred one. Verified: against the as-shipped iter-61v plan (which honestly left `RdpError::Navigation` unticked) the check passes; against a synthetic plan with the `live_navigate_neterror_dns_fail` AC speculatively ticked, the check fails with the expected message. (The original AC wording assumed iter-61v's plan would be dishonest at merge — it wasn't. Test moved to the synthetic fixture.)
- [x] `run-iteration.sh --replay iter-61v` exits 1; replay against iter-61t exits 0 (iter-61t's claims all check out). Verified by hand and pinned by `cargo xtask check-discipline-regression`.
- [x] `cargo xtask check-discipline-regression` is wired into CI (`.github/workflows/ci.yml` `discipline` job) and passes against current main.
- [x] CLAUDE.md and `kb/decision-log.md` cross-reference this iteration as the load-bearing implementation of the postmortem's mitigation #4 (claim-vs-code) and mitigation #5 (AC fidelity).
- [x] `cargo fmt && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace -q` clean.

## Design notes

- The skill lives outside the repo (in `~/.claude/skills/`). To keep this iteration's work reviewable, mirror the scripts into `tools/ralph-loop/` in the repo with a `tools/ralph-loop/README.md` saying "canonical copy lives in `~/.claude/skills/ralph-loop/scripts/`; this is for review and historical reference only." Future skill edits should update both, and the `cargo xtask check-discipline-regression` target verifies they're in sync.
- Theme C is the most important deliverable: without replay tests, the scripts are untrusted code edits to a skill that drives the whole roadmap. A captured baseline against iter-61v gives confidence that the regex-based claim parsing actually catches the false claims it's supposed to.
- This iteration cannot itself be run through ralph-loop (it modifies the skill that ralph-loop uses). Run it as a hand-driven iteration with a normal Claude session, and add a note to the `_template.md` listing "skill-edits" as a category of iteration that must be hand-driven.

## Out of scope

- Auto-correcting false claims. The tool flags; the human (or a follow-up iteration) fixes.
- A general-purpose commit-message linter. The claim regex is tuned for iteration commits, not arbitrary projects.
- Cross-iteration claim tracking ("you said X would land in iter-61p but it landed in iter-61t"). Possible follow-up.

## References

- [[iter-61m-61s-postmortem-loose-ends]] §"Mitigations" #4 and #5
- [[iter-61y-iteration-discipline-tooling]] — themes D and E deferred from there
- [[ralph-loop-pattern]]
- [[stability-roadmap]]
