---
type: rdp-note
tags:
  - rdp
  - firefox-server
  - actor
  - console
  - eval
  - critical
date: 2026-05-23
firefox_files:
  - devtools/server/actors/webconsole.js
  - devtools/server/actors/webconsole/eval-with-debugger.js
  - devtools/shared/specs/webconsole.js
title: WebConsoleActor
---

# WebConsoleActor (typeName `"console"`)

Lives **inside the target actor** (one per WindowGlobalTarget / WorkerTarget / ContentProcessTarget). Reach it via the target's form: `target.consoleActor`.

- Source: `devtools/server/actors/webconsole.js` (1683 lines).
- Spec: `devtools/shared/specs/webconsole.js`.
- Eval impl: `devtools/server/actors/webconsole/eval-with-debugger.js`.

## Methods

### `evaluateJSAsync({ text, frameActor?, url?, selectedNodeActor?, selectedObjectActor?, innerWindowID?, mapped?, eager?, disableBreaks?, preferConsoleCommandsOverLocalSymbols?, evalInTracer? })`

Returns immediately with `{ resultID }` (a `"<timestamp>-<counter>"` string).

The **actual** result arrives later as an unsolicited `evaluationResult` event carrying the same `resultID`. Client must correlate.

Critical pattern from `webconsole.js:786`:

```js
DevToolsUtils.executeSoonWithMicroTask(async () => {
  let response = await this.evaluateJS(request);
  response = await this._maybeWaitForResponseResult(response);   // <-- awaits Promise if mapped.await
  response.timestamp = ChromeUtils.dateNow();
  this.emit("evaluationResult", { type: "evaluationResult", resultID, startTime, ...response });
});
return { resultID };
```

#### Promise handling — IMPORTANT for ff-rdp

A Promise return value is **only** awaited if the caller passed `mapped: { await: true }`. Code path (`webconsole.js:944`):

```js
if (mapped?.await && result?.class === "Promise" && typeof result.unsafeDereference === "function") {
  awaitResult = result.unsafeDereference();
}
```

Without `mapped.await`, a Promise comes back as a generic object grip with `class: "Promise"` and the client sees nothing useful. This is the well-known "evaluateJSAsync doesn't resolve Promises" gotcha — it can resolve them, but the request must opt in. The Firefox DevTools console UI sets `mapped.await = true` whenever the input parses as a top-level `await`.

If the promise rejects, response gets `topLevelAwaitRejected: true` and `awaitResult` is removed.

### `autocomplete(text, cursor, frameActor?, selectedNodeActor?, authorizedEvaluations?, expressionVars?)`

Returns `{ matches: string[], matchProp }` — uses `jsPropertyProvider`.

### `getCachedMessages(messageTypes: string[])`

Returns cached page errors, console-api calls etc. recorded **since the listeners were started**. Types: `"PageError"`, `"ConsoleAPI"`. Returns `{ messages | error, …}`.

### `startListeners(listeners: string[]) → { startedListeners }`

Listener kinds: `"PageError"`, `"ConsoleAPI"`, `"FileActivity"`, `"ReflowActivity"`, `"ContentProcessMessages"`, `"DocumentEvents"`. Listeners populate the cache and fire live events.

### `stopListeners(listeners?: string[])`, `clearMessagesCacheAsync()`

## Events

- `evaluationResult` — the async return for `evaluateJSAsync`. Fields: `resultID, awaitResult, exception, exceptionMessage, exceptionStack, hasException, frame, helperResult, input, notes, result, startTime, timestamp, topLevelAwaitRejected`.
- `consoleAPICall` — `{message, clonedFromContentProcess?}` for `console.log/warn/error/…`.
- `pageError` — `{pageError}` for JS exceptions / CSP violations / parse errors.
- `logMessage` — generic `{message, timeStamp}`.
- `serverNetworkEvent` (renamed to `networkEvent` client-side) — `{eventActor}`. Used in legacy non-Watcher mode.
- `reflowActivity`, `fileActivity`, `documentEvent`, `inspectObject`.

## Lifecycle

- Created lazily by the WindowGlobalTargetActor / WorkerTargetActor when the target is created.
- One per target. Destroyed with the target.
- `frameActor` arg lets you eval in the scope of a specific paused stack frame (debugger).

## Gotchas for ff-rdp

- **The eval is async, the response isn't the result.** Always wait for the matching `evaluationResult` event.
- **`mapped: { await: true }` is required to resolve Promises.** See [feedback_recorded_fixtures] and [project_rdp_async_constraints] memory notes.
- `eager: true` runs in "eager evaluation" mode (no side effects, e.g. for hover-previews in the console). It silently returns `undefined` for non-pure expressions.
- `disableBreaks: !!request.disableBreaks` (line 878) — defaults to **true** in internal calls, so DevTools own evals never trip breakpoints. Set `disableBreaks: false` only if you want breakpoints to fire.
- `innerWindowID` lets you target a specific iframe inside a top target.
- Workers have a stripped-down listener set — only `worker-listeners.js` is loaded under `isWorker`.
- Result objects come back as **grips**; complex objects need a follow-up to ObjectActor to inspect properties.
