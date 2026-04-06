---
title: "Ralph Loop: Multi-Iteration Autonomous Orchestration"
status: research
tags: [automation, claude-code, workflow]
date: 2026-04-06
---

# Ralph Loop Pattern

Use one Claude Code session as a thin orchestrator that spawns independent `claude -p`
processes for each iteration. Each child process gets a fresh context window, its own
git worktree, and can spawn its own sub-agents internally.

## Why

A single Claude session runs out of context mid-implementation. The Agent tool helps
but agents **cannot spawn their own agents** — only one level of delegation.

`claude -p` spawns a full Claude Code instance that:
- Gets its own 200k context window
- Can use the Agent tool internally (multi-level delegation)
- Reads CLAUDE.md and follows all project conventions
- Can use skills (/create-pr, /merge-pr, etc.)

## Architecture

```
Orchestrator session (this session — thin loop)
  │
  ├─ claude -p "Implement iteration 23..."  ← full Claude Code instance
  │    ├─ Agent(Explore): research codebase
  │    ├─ Agent(rust-developer): implement
  │    ├─ Agent(rust-developer): write tests
  │    └─ /create-pr, /review-pr, /merge-pr
  │
  ├─ claude -p "Implement iteration 24..."  ← starts after 23 is merged
  │    └─ ...
  │
  └─ ... (sequential — each builds on the previous)
```

The orchestrator's context stays minimal — just loop state and exit codes/summaries.

## Child Process Prompt Template

Substitute `{N}` with the iteration number. Use this verbatim as the `-p` prompt:

```
You are implementing iteration {N} of the ff-rdp project.

1. Read the iteration plan from kb/iterations/iteration-{N}-*.md
2. Implement everything in the plan (code, tests, error handling) using agents whenever possible
3. Ensure all docs, help texts, and ./kb are updated to reflect changes
4. Run quality gates: cargo fmt, cargo clippy, cargo test
5. /create-pr
6. /review-pr — fix all review issues
7. If a plan exists for the next iteration, check if its scope needs to be
   adapted based on what you learned during implementation. Update it if so.
8. /merge-pr
```

## Orchestrator Command

```bash
claude -p \
  --permission-mode auto \
  --worktree \
  --output-format json \
  "<child process prompt>"
```

| Flag | Purpose |
|---|---|
| `-p` | Non-interactive, prints result and exits |
| `--permission-mode auto` | Claude decides risk; low-risk auto-approved, high-risk may pause. Use `bypassPermissions` if `auto` is too cautious |
| `--worktree` | Isolated git worktree per iteration (own branch) |
| `--output-format json` | Structured result the orchestrator can parse |
| `--model sonnet` | Optional: use cheaper model for simpler iterations |

### Permission Modes (escalation ladder)

1. **`auto`** — smart default; Claude decides based on risk
2. **`bypassPermissions`** — skips prompts but keeps hooks and safety checks
3. **`--dangerously-skip-permissions`** — nuclear; bypasses everything including hooks. Only for fully sandboxed environments

## Orchestrator Loop

Iterations run **sequentially** — each one builds on the merged result of the previous.

```
for N in requested range:
  1. Read iteration plan from kb/iterations/ to confirm it exists
  2. Run: claude -p --worktree --permission-mode auto --output-format json \
       "<child process prompt with {N} substituted>"
  3. Check exit code and JSON output
  4. On failure: STOP the loop. Report which iteration failed and why.
     Leave the worktree intact for inspection.
  5. On success: verify the merge landed on origin/main
     a. git fetch origin main
     b. git log origin/main --oneline -5  # confirm the merge commit is there
     c. git worktree prune                # clean up stale worktree references
  6. Report iteration N complete, proceed to N+1
```

## Failure Policy

- **Stop on first failure.** Do not skip failed iterations — later iterations depend on earlier ones.
- On failure, report: which iteration, what step failed, and the JSON output from the child.
- The worktree is preserved so the user can inspect or resume manually.
- The user decides whether to retry, fix manually, or abort.

## Considerations

- **Full lifecycle per child**: each child handles implement → PR → review → fix → adapt next → merge
- **Forward planning**: the child adapts the next iteration's plan if it exists — skipped for the last iteration if no plan follows
- **Error handling**: clippy/test failures are handled inside the child. The orchestrator only sees success/failure
- **Context isolation**: each child starts fresh — no cross-contamination between iterations
