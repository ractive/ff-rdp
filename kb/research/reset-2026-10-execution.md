---
title: "Reset 2026-10: execution tracker"
type: research
status: in-progress
date: 2026-10-02
tags: [research, step-back, execution]
---

# Reset 2026-10: execution tracker

Executes [[step-back-2026-10-02]]. Driven directly from a Claude Code session
with delegated agents — not through ralph-loop, not through iteration plans.
Each phase is one PR (or one docs merge). Agents work in their own worktree off
`origin/main`, run fmt → clippy → `cargo test --workspace -q` plus targeted
live tests only, never the full sweep, never file a new iteration plan, never
investigate a historical flake.

| Phase | Scope | Owner | Status | PR |
|---|---|---|---|---|
| 0 | Land step-back note and Codex handoff docs on `main` | session | done | docs merge |
| 1 | 286 `sources` native-path fix from archived patch; `dom --frames` hint fix | agent (Rust) | planned | |
| 2 | Process cut: CLAUDE.md, skills, plan template, `live.yml` nightly, quarantine rule, timing assertions removed, AGENTS.md reset, close 259/266/268/293/294 + PR #281, roadmap done | agent (process) | planned | |
| 3 | Daemon deletion; refs in-page; throttle/emulate per-call or removed; capture-mode docs | agent (Rust) | planned | |
| 4a | Env-var and iteration-hook purge; help drift; deprecated aliases; `allow(dead_code)` | agent (Rust) | planned | |
| 4b | `actors`/`fronts` collapse; launch/profile simplification | agent (Rust) | planned | |
| 5 | Watch nightly live run for a week; then product features | session | planned | |

## Log

- 2026-10-02: phase 0 started.

## Global skill follow-ups

The skills under `~/.claude/skills/` are shared with the owner's other repositories, so the
phase-2 PR did not edit them. Recommended edits, to be made by hand in a regular session:

- **`create-pr` §2 "Pre-PR discipline gates"**: drop `check-dogfood-script` from the list of
  plan-taking subcommands (it no longer exists). Run the xtask `check-*` enumeration **once per
  iteration**, not on every `/create-pr` update: skip it when `git rev-parse HEAD^{tree}` equals
  the tree the last green run recorded. The `--help` discovery stays (an absent gate is normal).
- **`create-pr` §2 "Quality gates"**: skip `cargo fmt` / clippy / `cargo test` when the tree hash
  is unchanged since the last green run in this session (CLAUDE.md now says "run once after the
  last code change"). Only run them if `/create-pr` itself committed new code changes.
- **`review-pr`**: one local review pass per PR; do not start a second independent review for
  style-only or doc-only fixes, and do not re-run the quality gates after fixes that touch only
  `kb/` or comments.
- **`merge-pr` §4 "Update iteration status ON THE BRANCH"**: drop the separate
  `chore(iter-N): mark status=done` commit. Plan ticks and `status: done` now land in the same
  commit as the code (CLAUDE.md "PR Discipline"), so a status-flip commit only triggers a second
  full CI run. Keep the step as a check: if the plan is not yet `done`, stop and say so rather
  than committing.
- **`ralph-loop` / `new-ralph-loop`**: `ralph-loop/SKILL.md` (line ~273) still refers to the
  `live-sweep` as part of closing an iteration; the per-PR live sweep is gone and the
  `iteration-close` skill was deleted from this repo. Remove that step; one fmt/clippy/test run
  after the last code change, one xtask pass, `gh pr merge --merge --delete-branch`.
  The in-repo mirrors under `tools/ralph-loop/` and `tools/new-ralph-loop/` are no longer
  required to stay in sync (the mirror rule was dropped).
