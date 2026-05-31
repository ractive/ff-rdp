---
title: "Field report: claude-code crashes during ralph-loop Phase 2 review (recurring)"
type: field-report
date: 2026-05-31
status: open
source: in-session observation, recurring
commands_used: [cmux claude-teams, /review-pr]
tags:
  - field-report
  - claude-code
  - ralph-loop
  - dx
  - crash
---

# Field report — claude-code crashes during ralph-loop Phase 2 review (recurring)

Second recurring claude-code/cmux instability hit on the same day as
the `/usage` freeze report
([[field-report-cmux-claude-freeze-after-usage-2026-05-31]]). The
freeze symptom is "input silently doesn't reach the session"; this
one is an outright session crash.

## Symptom

1. `cmux claude-teams --permission-mode auto --name iter-91-review`
   launched in a fresh pane (surface:37) with the iter-91 Phase 2
   review prompt.
2. The session ran for a while in `auto mode`, doing the
   `/review-pr` flow against PR #128 — reading the discipline log,
   issuing test edits, etc.
3. The session exited without writing the sentinel file at
   `/tmp/ralph-loop-done-91-review.manual`. The pane was left at a
   bare shell with the `claude --resume "iter-91-review"` hint visible.
4. User reported "Claude Code crashed in the iter-91-review phase"
   and manually `claude --resume`d the session.

No specific signal (e.g. a panic backtrace, an unhandled async
rejection) was captured because the session's stderr scrolled off
the pane before recovery.

## Impact on ralph-loop

This is the **fourth time in two days** that Phase 2 has been broken
by a claude-code / cmux issue:

- iter-87 first attempt — Phase 1 ac-fidelity timeout left pane open
- iter-89 — manual Phase 2 ended via `/usage` freeze
- iter-91 — Phase 2 wedged on `/usage`, user closed pane, fresh pane
  launched, and **that** session crashed mid-review.

The sentinel-based orchestrator pattern (orchestrator waits for
`/tmp/ralph-loop-done-N-review.manual`) tolerates any sub-failure
that ends with a write; it cannot recover from a session that exits
without writing. There is no liveness check — the sentinel watcher
waits forever.

## Hypotheses

1. **`/review-pr` or one of its sub-flows allocates so much context
   that the session OOMs / hits a hard token limit and the process
   dies without writing the sentinel.** This would explain the
   crashes clustering around `/review-pr`. The same prompt
   (without review) doesn't seem to crash.
2. **A long-lived `cmux claude-teams` session has a slow leak that
   eventually destabilises the process** — separate from any one
   slash command. Would explain why fresh-pane sessions are also
   affected once the workspace has been running for hours.
3. **Backgrounded subagents inside the session (rust-developer,
   general-purpose) accumulate orphaned state** that wedges or
   kills the parent. The pre-freeze `/usage` output showed
   "4 monitors" running in the iter-91 session.

## Suggested next steps

- Upstream against **claude-code**: when the session exits for any
  reason (crash, OOM, signal), write a JSON event to a known
  location naming the cause. Tools like ralph-loop can then detect
  the exit explicitly instead of waiting on a sentinel forever.
- Upstream against **claude-code**: capture at least the last 100
  lines of session stderr to a per-session log file so post-mortem
  diagnosis is possible.
- **In ralph-loop**: add a liveness check to the sentinel watcher
  — verify the claude-teams process named `iter-N-review` is still
  alive; if not, declare review failed and surface a clear error.
- **Restructure Phase 2**: rather than depending on the long-lived
  cmux pane and slash-command flow, the orchestrator could drive
  the merge directly using `gh` CLI + a single `claude --resume`
  for the review-pr step. Smaller per-session scope → smaller crash
  surface.

## References

- [[field-report-cmux-claude-freeze-after-usage-2026-05-31]] — sibling
  field report (the `/usage`-freeze symptom).
- [[iteration-91-check-pre-fix-repro-perf-and-recoverability]] — the
  iteration on which this was observed.
