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

- `attach(options)` — required JSON options object; ordinary empty reply. On the
  qualified current package, a fresh thread becomes running and an already
  attached thread retains its state.
- `resume(resumeLimit)` — resume execution; `resumeLimit` controls step mode.
- `frames(start, count)` — list call frames in the paused stack.
- `interrupt(when)` — pause request with a JSON argument; not declared oneway.
- `sources()` — list all JS source files loaded in the thread.

## Status

Expanded in iteration 286 from the installed Firefox package; earlier stub
wording about paused attach and oneway interrupt was incorrect for this package.

## Qualified contract — 2026-09-28 (iteration 286)

Firefox 156.0.1, BuildID 20260921121718, SourceStamp
`6f2c158dfc7e9693f880fad2510ceb51a158c069`; installed browser omni.ja SHA256
`85f891cec3e54150027582ac74eb96fc3774cc0ab4bfd96f7192905149d05fff`.
The archive member `shared/specs/thread.js:72–107` declares attach
`options: Arg(0, "json")`, `response: {}`, sources returning an array, and
interrupt without oneway. Server `thread.js:390–431` returns immediately when
already attached, otherwise transitions DETACHED→RUNNING. Its sources (lines 1583–1595)
enumerates source forms without a paused-state requirement. Thread detach is
absent from the current spec/server.

`ThreadActor::attach` therefore sends `options: {}` and consumes an ordinary
reply. A paused event is forwarded through the caller's installed event sink,
not consumed as method completion. `list_sources` only attaches/enumerates;
it does not resume an independently owned pause, on success or error, and never
sends unsupported detach. The caller retains ownership of its connection and
closes an owned direct transport to release connection-scoped actors. Explicit
resume remains for callers that own a pause. The old public detach helper is
deprecated for API compatibility; no repository caller uses it.

The single H1 native occurrence did not reach the predicted paused-event wait:
attach omitted options and Firefox returned `undefined passed where a value is
required`. The CLI then reported JS fallback with an empty source actor. No
sources request occurred. With `options: {}` the native path succeeds: on
2026-10-02 `ff-rdp sources` against https://www.iana.org/help/example-domains
listed real `…/sourceN` actors with no `fallback_method`, and
`live_286_native_sources` (which the JS fallback cannot satisfy) passes.

## Native source text presentation — 2026-09-28

The repaired native path exposes real source actor IDs. Iteration 286's first
closing sweep found a 127-character `sources --format text` table header: the
URL, actor and `isBlackBoxed` columns exceeded the existing 120-character text
contract when their widths were added. The candidate correction opts only the
sources caller into a 120-character table budget, counting characters when
padding and shortening cells. Full source URLs/actor IDs remain intact in JSON
and jq input; field projection and sorting precede presentation as before.
The budget counts characters, not terminal columns, so wide (e.g. CJK)
characters in un-encoded URLs can still exceed 120 display columns.
