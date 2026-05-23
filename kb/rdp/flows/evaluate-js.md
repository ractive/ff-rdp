---
type: rdp-note
tags:
  - rdp
  - firefox-client
  - flow
  - webconsole
date: 2026-05-23
firefox_files:
  - devtools/shared/specs/webconsole.js
  - devtools/server/actors/webconsole.js
  - devtools/client/webconsole/middleware/event-telemetry.js
title: "Flow: Evaluate JS in a target"
---

# Flow: Evaluate JavaScript in a target

The DevTools console, autocomplete, the `:screenshot` command, Eager
Evaluation, and panel commands all ultimately go through one method:
`WebConsoleFront.evaluateJSAsync`. It is the most-used and most-subtle
method in the protocol.

## The two-phase shape

`evaluateJSAsync` is **async-by-design** at the protocol level — not just at
the JS API level. The reason: an actor can only have one in-flight request at
a time (RDP serializes per-actor). If `evaluate` blocked the response,
autocomplete and other console requests would stall for the duration of a
long-running script. So it returns immediately with a `resultID`, and the
actual result arrives later as an `evaluationResult` *event*.

Spec at `specs/webconsole.js:149-164`:

```js
evaluateJSAsync: {
  request: {
    text:                Option(0, "string"),
    frameActor:          Option(0, "string"),
    url:                 Option(0, "string"),
    selectedNodeActor:   Option(0, "string"),
    selectedObjectActor: Option(0, "string"),
    innerWindowID:       Option(0, "number"),
    mapped:              Option(0, "nullable:json"),
    eager:               Option(0, "nullable:boolean"),
    disableBreaks:       Option(0, "nullable:boolean"),
    ...
  },
  response: RetVal("console.evaluatejsasync"),  // { resultID: "string" }
}
```

The event (`specs/webconsole.js:45-62`):

```js
evaluationResult: {
  resultID:        Option(0, "string"),
  result:          Option(0, "nullable:json"),  // a grip
  exception:       Option(0, "nullable:json"),
  exceptionMessage:Option(0, "nullable:string"),
  hasException:    Option(0, "nullable:boolean"),
  awaitResult:     Option(0, "nullable:boolean"),
  startTime, timestamp, frame, helperResult, ...
}
```

## Wire trace

Request:

```json
{"to":"server1.conn0.console5","type":"evaluateJSAsync",
 "text":"document.title","eager":false}
```

Immediate reply:

```json
{"from":"server1.conn0.console5","resultID":"server1.conn0-15"}
```

Then, **on the same connection but as a separate packet** (could be
milliseconds or minutes later):

```json
{"from":"server1.conn0.console5","type":"evaluationResult",
 "resultID":"server1.conn0-15",
 "result":"Example Domain",
 "input":"document.title", "startTime":1700000000000, ...}
```

The client correlates by `resultID`. DevTools uses a `Map<resultID, deferred>`
inside the webconsole front; ff-rdp does the same in
`crates/ff-rdp-core/src/eval.rs`.

## The `result` field: grips

A primitive result (`"Example Domain"`, `42`, `true`, `null`) comes back as
itself. An object result comes back as a **grip**:

```json
{"type":"object","class":"HTMLDivElement","actor":"server1.conn0.obj42",
 "preview":{...}}
```

To inspect further you'd request properties via the new `ObjectFront`. If you
just want the result printed, use the `preview` (a shallow snapshot of
properties) or call `getProperty` for live values.

## Long-running JS — the gotcha

ff-rdp's recorded gotcha (`feedback_network_perf_api.md`, the daemon work
in iter 37-38, and `project_rdp_async_constraints.md`):

> **`evaluateJSAsync` does not resolve `Promise` return values.** If the
> evaluated expression is itself a Promise, the `result` grip you get back
> describes the Promise object — not what it resolves to.

To get the eventual value: either `await` inside the expression (the server
supports top-level await — `awaitResult: true` is reported back), or use the
helper `helperResult` channel for command-style operations.

This is why ff-rdp's daemon-mode work needed a streaming event API: any time
you want to observe an async side-effect (a network event, a console.log
from inside a Promise) you must subscribe via the resource watcher, not via
the eval result.

## ff-rdp implementation pointers

`crates/ff-rdp-core/src/eval.rs` (or similar) wraps the protocol pattern:
sends `evaluateJSAsync`, parses `resultID` from the immediate reply, then
buffers packets until it sees a matching `evaluationResult` event. The
`--stringify` flag (mentioned in iter-61i kb notes) instructs callers to
pre-stringify objects in JS rather than relying on grip introspection.

See also: [[watch-resources]] for the related event-stream pattern.
