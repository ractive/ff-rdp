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
