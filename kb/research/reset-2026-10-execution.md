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
| 1 | 286 `sources` native-path fix from archived patch; `dom --frames` hint fix | agent (Rust) | done | #285 (ff22b7cb) |
| 2 | Process cut: CLAUDE.md, skills, plan template, `live.yml` nightly, quarantine rule, timing assertions removed, AGENTS.md reset, close 259/266/268/293/294 + PR #281, roadmap done | agent (process) | done | #286 (4def27f4) |
| 3 | Daemon deletion; refs in-page; throttle/emulate per-call or removed; capture-mode docs | agent (Rust) | done | #287 (fcb4a439) |
| 4a | Env-var and iteration-hook purge; help drift; deprecated aliases; `allow(dead_code)` | agent (Rust) | in-progress | |
| 4b | `actors`/`fronts` collapse; launch/profile simplification | agent (Rust) | in-progress | |
| 5 | Watch nightly live run for a week; then product features | session | planned | |

## Log

- 2026-10-02: phase 3 merged (#287): 243 files, +3,020/−40,069. Refs stamped in-page (`data-ffrdp-ref`), `throttle`/`emulate` commands gone (`navigate --throttle/--block` instead), `inspect <expr>` single-connection, `network --headers` bug fixed. Phases 4a/4b launched in parallel.

- 2026-10-02: phase 3 PR #287 opened (DEC-056, DEC-057). `shortstat` against origin/main: 238 files changed, 2884 insertions(+), 40028 deletions(-). Deleted: `src/daemon/` (about 17.8k lines: server 8.1k, client 3.0k, registry 1.2k), `daemon_record.rs` (842), `daemon_status.rs` (168), the `emulate`/`throttle` commands, 21 daemon live-test files plus the `daemon`/`daemon_parity`/`emulate`/`throttle` e2e suites, `eval_object_leak_soak`, the `--no-daemon` live guard, the xtask `daemon-locks` invariant and the core proxy hooks. Refs moved in-page (`data-ffrdp-ref`); `--throttle`/`--block` on navigate/reload; emulate deleted (settings reset on disconnect, verified live); `inspect <js-expression>`. Targeted live run: 159 passed.

- 2026-10-02: phase 1 merged (#285): `ThreadActor::attach` now sends `options: {}`; `sources` reaches the native path (6 real source actors on iana.org). `dom --frames` hint was already fixed on main (8fbb1ba2). Phase 3 (daemon removal) launched.

- 2026-10-02: phase 2 merged (#286). Leftovers for phase 4a: live_220 2.5 s bound + `timing_load_note`; `HANDOFF-iter-233-256.md` still mentions iteration-close; historical `*.dogfood.sh` + `dogfood-lib.sh` (used by tools/axi-bench). Rust 1.99 `assert_is_empty` is allowed in the lint tables rather than fixed at ~30 sites.

- 2026-10-02: phase 0 started.
- 2026-10-02: phase 2 PR #286 opened; PR #281 closed unmerged.

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
