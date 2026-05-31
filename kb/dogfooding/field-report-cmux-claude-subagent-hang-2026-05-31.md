---
title: "Field report: claude-code subagent appears to hang on `Generating…` for tens of minutes (recurring)"
type: field-report
date: 2026-05-31
status: open
source: in-session observation, recurring
commands_used: [cmux claude-teams, rust-developer subagent, /review-pr]
tags:
  - field-report
  - claude-code
  - subagent
  - ralph-loop
  - dx
  - hang
---

# Field report — claude-code subagent appears to hang on `Generating…` for tens of minutes

Third recurring claude-code instability hit today, distinct from the
two earlier reports
([[field-report-cmux-claude-freeze-after-usage-2026-05-31]],
[[field-report-cmux-claude-crash-during-review-2026-05-31]]). The
session does not crash and does not freeze on a modal — it appears to
be actively "Generating…" but with minimal token throughput and no
visible progress.

## Symptom

Observed in the iter-91-review pane (surface:37, fresh
`cmux claude-teams --permission-mode auto` session). Mid-Phase-2,
after `/review-pr` started:

```
✽ Generating… (35m 40s · ↓ 14.7k tokens)
  ⎿  Tip: Use /clear to start fresh when switching topics and free up context

──────────────── iter-91-review ──
❯
────────────────────────────────────
  ⏵⏵ auto mode on · PR #128 · 2 shells

  ⏺ main                                    ↑/↓ to select · Enter to view
  ◯ rust-developer  Address CodeRabbit review on PR #128       34m 43s
```

Specifically:
1. A `rust-developer` subagent was dispatched to "Address CodeRabbit
   review on PR #128".
2. The subagent has been running for 34m 43s with the agent-list
   indicator (`◯`) suggesting still-pending status.
3. The parent session displays `Generating… (35m 40s · ↓ 14.7k
   tokens)`. ~15k tokens over 35 minutes is ~7 tokens/sec, far
   below normal streaming throughput.
4. Keyboard input (Esc, Ctrl-C, Enter, character keys) sent via
   `cmux send-key` is acknowledged at the cmux layer but does not
   interrupt the generating state nor produce any visible response
   in the pane.
5. The session is not crashed — the spinner is animated and the
   token count slowly advances — but the work has effectively stalled.

Notably, **no review-fix commits have been pushed to origin** during
the 35 minutes — the subagent is not making file system progress
either.

## Impact on ralph-loop

This is failure mode #5 on iter-91 alone (Phase 2 has been broken
five times for this single iteration):
1. Phase 1 hit 2h `RALPH_PHASE_TIMEOUT` before /create-pr could write the DONE sentinel.
2. Phase 2 manual relaunch wedged on `/usage` freeze.
3. Replaced pane, fresh session crashed mid-`/review-pr`.
4. User resumed; immediately wedged again.
5. Fresh subagent hangs in `Generating…` with ~7 tokens/sec throughput.

The orchestrator sentinel-watcher pattern (Bash `until [ -f sentinel ]`)
has no upper time bound, so the wait persists indefinitely while the
subagent "works".

## Hypotheses

1. **Backpressure / rate-limit silently engaged.** The Anthropic API
   may have throttled the subagent's streaming response without
   surfacing a user-facing message. The session's spinner kept
   spinning to mask the silence.
2. **Subagent invoked a tool with a very long body** (e.g. a Read on
   a giant file, or a Bash command with massive stdout). The
   "Generating…" timer counts wall clock, not just generation. If
   the subagent is *executing tools* rather than generating, the
   display is misleading.
3. **A long-running subagent has a per-call wall-time bound that
   exceeds normal use** and the parent doesn't surface the per-call
   identity, so we can't tell whether it's generating, executing a
   tool, or waiting on an API response.
4. **Subagent → parent communication deadlock**. The parent is
   awaiting the subagent's structured result and the subagent's
   call to a tool has stalled at the OS layer (network, FS).

## Suggested next steps

- **Upstream against claude-code**: When a subagent runs longer than
  a configurable threshold (e.g. 5 min), surface progress signals to
  the parent — current tool, current generation step, last-event
  timestamp. The user (or the orchestrator) needs a "is this making
  progress" signal that's better than a single spinner.
- **Upstream against claude-code**: Token-rate / activity heuristic.
  If a subagent generates fewer than N tokens per minute for more
  than M minutes, surface a "stalled?" UI affordance with a one-key
  cancel.
- **In ralph-loop**: cap Phase 2 with a wall-time budget *in the
  orchestrator's sentinel-watcher* (e.g. 30 min hard limit), and
  on expiry, declare review-failed and force the user to choose:
  merge-as-is, or hand off entirely.
- **In ralph-loop**: stop dispatching `/review-pr` from inside the
  iteration's pane. It is consistently the load-bearing failure
  point. Move the review step to a fresh, short-scope CC session
  invoked by the orchestrator directly.

## References

- [[field-report-cmux-claude-freeze-after-usage-2026-05-31]] — sibling
  report (Esc-doesn't-dismiss-`/usage`).
- [[field-report-cmux-claude-crash-during-review-2026-05-31]] — sibling
  report (`/review-pr` session crash).
- [[iteration-91-check-pre-fix-repro-perf-and-recoverability]] — the
  iteration on which all three were observed today.
