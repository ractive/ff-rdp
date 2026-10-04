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
| 4a | Env-var and iteration-hook purge; help drift; deprecated aliases; `allow(dead_code)` | agent (Rust) | done | #290 (b9ccc091) |
| 4b | `actors`/`fronts` collapse; launch/profile simplification | agent (Rust) | done | #291 (edcc701b) |
| 5 | Watch nightly live run for a week; then product features | session | in-progress | |

## Log

- 2026-10-04: dogfooding session 64 ([[dogfooding-session-64]], 3 agents, 49 issues, PR #303) → pre-release fix round: #304 snapshot refs past depth cut / per-document ref blocks + `stale_ref` / eval rejected promises; #305 `network --timeout` hard wall, `--viewport-height` removed, six nonexistent skill flags fixed, version 0.4.0; #306 `nav_cert_error` for about:certerror, `--throttle` readiness no longer taken from iframe targets, `click` waits for the navigation it starts and reports `navigated{}`. Independent re-check of the seven items pending; release v0.4.0 after it.

- 2026-10-03: deleted 13 stale remote branches (SHAs recorded for recovery via GitHub's reflog-less dangling-commit window; 4 were merged, 9 belonged to done/obsolete daemon-era plans):
- `iter-259/reply-ownership-after271` cee57bf4
- `iter-259/rpc-slot-handover-strands-an-in-flight-reply` cc44e873
- `iter-262/daemon-live-target-never-promoted` f2a95706
- `iter-268/auth-loss-attribution-20260921` 551abe6a
- `iter-268/daemon-pre-auth-connection-loss` 27604336
- `iter-271/bbc-consent-no-cmp-recurrence` 10312d2f
- `iter-275/guardian-consent-ready-target-failures` 803fc888
- `iter-276/cascade-fixture-attribution` cf91a06b
- `iter-277/direct-navigate-committed-about-blank` 62fda89d
- `iter-49/scroll-reload-fixes` 5fa7b132
- `reset/structure-core` e2629e8d
- `reset/structure-launch` af0020c9
- `reset/structure-navigate` 86a3a9cc


- 2026-10-03: backlog round 1, four PRs between 10:20 and 11:45 CEST (≈1 h 25 min, two agents at a time): #295 auto-wait probe shares `--timeout` (cause: 500 ms post-readiness window from iter-59; passes at load avg 165), #296 `navigate --with-network --headers/--security` + `click --wait-for-network --headers`, #297 `screenshot --color-scheme/--media` + `navigate --user-agent` (new `TargetConfigurationActor`), #298 ten wall-clock bounds removed + playbooks A2/D2/E1/E3 fixed. Also #293/#294: `ci` is now the single required status check on `main` (docs-only PRs pass in ~20 s; direct pushes refused). 9 backlog lines remain.

- 2026-10-03: follow-ups consolidated into `kb/backlog.md`; issues #288/#289 closed.
- 2026-10-03: phase 4b merged (#291): fronts collapsed into actors (15,429 → 12,347), navigate split into `navigate/`, launch waits for a listable tab, `--wait-timeout` → `--timeout-ms`, 284/279 diagnostic scaffolding deleted (owner-approved), eval/core grip release removed. Language pack kept (German console text without it on de_CH). Rust lines Oct 1 → Oct 3: 178k → 156k (CLI 158k → 119k, core 33k → 29k, xtask 10.6k → 8.4k). Wall time: analysis 19:30–21:00 CEST on 2026-10-02, execution 20:05 (first PRs) → 00:13 (#291 merged) CEST, ≈4 h for five PRs. Phase 5: first nightly-style live run triggered manually on main. Open follow-ups live in PR bodies #287/#290/#291 and issues #288/#289.

- 2026-10-02: phase 4b PR #291 opened: fronts collapsed into actors (15,429 -> 12,347 lines), navigate split (5,539 -> 5,427), launch+profile files 9,045 -> 8,042; launch waits for a listable tab; `navigate --timeout-ms`; 4a hooks, eval grip release, EvaluateScope frame/node fields and the owner-approved 166/279 diagnostics deleted. Language pack kept (de_CH console text stays German without it).

- 2026-10-02: 4a follow-ups for 4b: delete hooks `FF_RDP_147_CONTROL_HOME` (`launch.rs`), `FF_RDP_147_PIPE_PEER` (`launch/language_initialization/tests.rs`), `FF_RDP_284_*` (`launch/startup_controls.rs`), `FF_RDP_277_CAPTURE` (`live_166_navigate_document_status.rs`), `FF_RDP_ENGLISH_PACK_TEST_XPI` (`english_language_pack.rs`); resolve `allow(dead_code)` at `navigate.rs:1772` and `actors/watcher.rs:1110`; rename `navigate --wait-timeout` to `--timeout-ms` (with the "increase with --wait-timeout" error text); `EvaluateScope::{frame_actor, selected_node_actor}` has no CLI consumer after #290; `launch` returns before the first tab is listable (`tabs` right after saw 0 tabs).

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
