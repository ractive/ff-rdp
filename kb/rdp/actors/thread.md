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

## Iteration259: attach request and paused event remain distinct

`ThreadActor::attach` sends with `send_ordinary` while retaining its existing
`recv_event_from` wait for that thread's `paused` event. The event does not
itself discharge an ordinary reply obligation in daemon ownership accounting.
The caller's legacy paused-only receive compatibility is unchanged; it is not a
claim that every Firefox version emits an ACK or that an event is an ACK.
Direct Firefox wire bytes are unchanged; the descriptor is daemon-v3 metadata.

### Installed156 lifecycle correction — pending286

Installed Firefox156.0.1 attach returns an ordinary response and either preserves
an already-attached thread or enters RUNNING; it does not itself emit paused.
The current Rust helper still waits for paused and attempts unsupported detach
cleanup. The sources command calls that helper directly; timeout is not one of
its fallback conditions. These pre-existing defects are assigned to
[[iteration-286-native-sources-thread-lifecycle]], not repaired by259. The older
paused-on-attach and one-way interrupt descriptions above are inaccurate for
this package: specs/thread.js:72–107 and server/actors/thread.js:390–431 apply.
No new native failure was executed in this source review.
