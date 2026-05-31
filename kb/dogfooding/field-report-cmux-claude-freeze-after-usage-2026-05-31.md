---
title: "Field report: cmux pane's claude session freezes after `/usage` (recurring)"
type: field-report
date: 2026-05-31
status: open
source: in-session observation, recurring
commands_used: [cmux send, cmux send-key, cmux read-screen, /usage]
tags:
  - field-report
  - cmux
  - claude-code
  - ralph-loop
  - dx
---

# Field report — cmux pane's claude session freezes after `/usage` (recurring)

Hit at least three times in the last few days, including during iter-91
Phase 2 manual hand-off (after PR #128 was already created). Filing
because the failure mode is consistent and is one of the main reasons
ralph-loop's manual Phase 2 path keeps stranding.

## Symptom

1. A long-running claude session inside a `cmux claude-teams` pane
   reaches the point where its `/usage` dialog gets surfaced — usually
   because the agent was approaching its context budget and ran
   `/usage` to look, or the dialog was auto-shown.
2. The dialog shows the usual usage tables (skills %, subagents %, day
   / week toggle, "Esc to cancel" at the bottom).
3. The claude prompt line below the dialog (`❯`) is empty and the
   session does not accept further input.
4. `cmux send-key --surface <id> escape` returns OK but the dialog is
   not dismissed. Multiple Esc, `q`, and Ctrl-C all return OK at the
   cmux layer but the screen does not change and no character lands
   in the `❯` input line on the next `cmux read-screen`.
5. The session is effectively wedged: input from cmux's stdin channel
   does not reach claude-code, but cmux's process-state view of the
   pane reports it as alive.

Concrete log snippet from iter-91 (surface:31), after the user
visited `/usage`:

```
  Skills                  % of usage
  /review-pr                      7%
  /loop                           5%
  /merge-pr                       2%
  ...
  Esc to cancel

──────────────── iter-91-implement ──
❯
────────────────────────────────────
  ⏵⏵ auto mode on · PR #128 · 4 monitors
```

`cmux send-key --surface surface:31 escape` x3 → no change. `q` and
`ctrl+c` likewise no-op'd. Closing the pane via the cmux GUI and
launching a fresh `cmux claude-teams` session was the only working
recovery.

## Impact on ralph-loop

The skill's Phase 2 manual hand-off pattern is:

1. Phase 1 in cmux pane completes (or times out).
2. Orchestrator sends a follow-up prompt to the SAME pane via
   `cmux send --surface <id>`.
3. The implementing claude session — still alive — picks up the new
   prompt and runs review + merge.

When the session is wedged on `/usage`, step 3 silently no-ops. The
orchestrator's sentinel watcher waits indefinitely. The only fix is
to ask the user to close the pane and respawn from scratch.

Documented occurrences in this repo:
- iter-87 (first attempt, June 2026) — needed manual recovery
- iter-89 — needed manual recovery
- iter-91 — needed manual recovery (filing this report)

## Hypotheses

1. **claude-code's stdin handling in modal-dialog state is buggy.**
   The `/usage` modal may register a key handler that swallows
   Esc/Ctrl-C without forwarding to the dismissal logic. Possibly an
   async deadlock between the dialog renderer and the input loop.
2. **cmux's `send-key` translates `escape` to a wire-format claude
   doesn't recognise when in dialog mode.** Less likely — same key
   name dismisses other dialogs in the same session.
3. **The pane's PTY is in a half-stuck state.** cmux thinks it sent
   the keystroke; the PTY buffer is full or the child isn't reading
   from stdin. `cmux send` returns OK because the local send
   succeeded, not because the child processed it.

## Suggested next steps

- File upstream against **claude-code**: `/usage` dialog should be
  dismissable by Esc unconditionally, and Ctrl-C should bring the
  session back to the input prompt without exiting.
- File upstream against **cmux**: `send-key` and `send` should
  return a delivery-confirmation, not just a local-send OK. If the
  child's PTY isn't draining, surface that to the caller.
- **In ralph-loop**: stop relying on the Phase 1 cmux pane surviving
  for Phase 2. Either spawn a fresh `cmux claude-teams` session for
  Phase 2 every time (lose the resume convenience, gain reliability),
  or move Phase 2 out of cmux entirely and into the orchestrator's
  own context window. Both are valid; the second is in scope for the
  redesign discussion.

## References

- [[iteration-91-check-pre-fix-repro-perf-and-recoverability]]
- [[iteration-89-screenshot-fifth-attempt-single-theme]]
- [[iteration-87-gate-hardening-required-checks-and-dogfood-linter]]
