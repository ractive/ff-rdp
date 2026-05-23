---
type: rdp-note
tags:
  - rdp
  - firefox-server
  - resource
  - debugger
date: 2026-05-23
firefox_files:
  - devtools/server/actors/resources/thread-states.js
  - devtools/server/actors/thread.js
  - devtools/server/actors/breakpoint.js
title: "Resource: thread-state"
---

# Resource: `thread-state`

Per-target. Emitted each time the target's JS thread **pauses or resumes**. Backs the debugger UI's "paused" state.

## Payload

```
{
  resourceType: "thread-state",
  state: "paused" | "resumed",
  why: { type: "breakpoint" | "exception" | "debuggerStatement" | "interrupted" | "watchpoint" | "eventBreakpoint" | "stepIn" | "step" | ...,
         message?, exception?, actors?, frameFinished?, ... },
  frame: { actor, displayName, source, where, scope, environment, this },
  poppedFrames, recordingEndpoint, executionPoint,
}
```

## Gotchas

- Pairs with the ThreadActor's `attach` / `resume(resumeLimit)` methods.
- `eventBreakpoint` types match `actors/utils/event-breakpoints.js` (timer.set, dom.mutation, etc).
- For watchpoints: `why.type === "watchpoint"` with `actors: [actorID]` of the affected variable.
