---
type: rdp-note
tags:
- rdp
- firefox-server
- actor
- thread
- debugger
date: 2026-05-24
firefox_files:
- devtools/shared/specs/thread.js
- devtools/server/actors/thread.js
title: ThreadActor
---

# ThreadActor

The JavaScript debugger thread actor. Controls execution of JS in the attached
target — pause, resume, step, set breakpoints, evaluate expressions in paused
frames. One of the most complex actors in the protocol.

## Firefox references

| File | Lines | Purpose |
|------|-------|---------|
| `devtools/shared/specs/thread.js` | 1-190 | Protocol spec — resume, pause, frames, sources |
| `devtools/server/actors/thread.js` | 1-2414 | Server implementation |

## Key methods (from spec)

- `attach()` — attach to the thread (starts paused if `pause` option set).
- `resume(resumeLimit)` — resume execution; `resumeLimit` controls step mode.
- `frames(start, count)` — list call frames in the paused stack.
- `interrupt()` — forcibly pause a running thread (`oneway: true`).
- `sources()` — list all JS source files loaded in the thread.

## Status

Stub — backfilled in iter-73; expand on next touch.
