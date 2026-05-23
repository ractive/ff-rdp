---
type: rdp-note
tags:
  - rdp
  - firefox-client
  - flow
date: 2026-05-23
firefox_files:
  - devtools/shared/specs/descriptors/tab.js
  - devtools/shared/specs/targets/browsing-context.js
  - devtools/shared/specs/watcher.js
  - devtools/client/fronts/descriptors/tab.js
  - devtools/client/fronts/targets/browsing-context.js
title: "Flow: Attach to a target"
---

# Flow: Attach a descriptor → reach child actors

A `TabDescriptorFront` returned by `listTabs` is just metadata. To call
`evaluateJSAsync`, `getDocument`, `capture`, or `watchResources` you must
*attach* — promote the descriptor to a live `BrowsingContextTargetFront`
with child actors.

## Step-by-step (modern path, Fx 90+)

1. **Pick a descriptor** from `listTabs` output:

   ```js
   const descriptor = (await client.mainRoot.listTabs()).find(t => t.selected);
   ```

2. **Get the target.** `descriptor.getTarget()` does the heavy lifting:

   - Sends `{to: <descriptor>, type: "getTarget"}`.
   - Server replies with a `targetForm` — an object describing every child
     actor (`{actor, consoleActor, threadActor, inspectorActor,
     screenshotContentActor, ...}`).
   - Client builds a `BrowsingContextTargetFront`, sets its `actorID`, and
     stashes the form so `target.getFront("console")` etc. resolve
     synchronously to the right actorIDs.

3. **(Optional) Get the watcher.** For cross-target / cross-frame
   subscriptions, also call `descriptor.getWatcher()` which gives a
   `WatcherFront` (spec at `specs/watcher.js`).

   ```js
   const target = await descriptor.getTarget();
   const watcher = await descriptor.getWatcher();
   ```

4. **Resolve child fronts on demand:**

   ```js
   const console      = await target.getFront("console");
   const inspector    = await target.getFront("inspector");
   const walker       = await inspector.getWalker();
   const screenshotC  = await target.getFront("screenshot-content");
   ```

   `getFront(typeName)` uses the actorID for that typeName already present in
   the target form — no extra round-trip unless we need to traverse
   (`inspector.getWalker()` does send a request).

## What "attach" means now vs. then

Historically there was an explicit `{to: target, type: "attach"}` packet
required before child actors became reachable. As of recent Firefox the
`getTarget` call effectively *is* the attach — the server creates the target
actor and its children on-demand and returns them in one shot. The old
`attach` method still exists on some specs for backward-compat but is mostly
a no-op.

The descriptor stays alive: closing the tab triggers a `targetDestroyed`
event on the descriptor, and the target front's `actorID` is cleared. Always
re-fetch via `descriptor.getTarget()` rather than caching across navigations.

## The target form (key fields)

Returned by `getTarget`. Names you'll see in real packets:

- `actor` — the target actor itself.
- `consoleActor` — the WebConsole, for evaluation & cached messages.
- `inspectorActor` — entry to walker / page-style / layout.
- `styleSheetsActor`, `cssPropertiesActor`, `memoryActor`, `threadActor`,
  `performanceActor`, `screenshotContentActor`, `responsiveActor`,
  `accessibilityActor`, `manifestActor`, ...
- `browsingContextID` — needed for parent-process `ScreenshotFront.capture`
  (see [[take-screenshot]]).
- `traits` — feature flags, often duplicating root traits.

## ff-rdp implementation pointers

ff-rdp skips much of this layering: most commands send `getTarget` once and
keep the form as a plain `serde_json::Value`, then look up actorIDs by key
(`form["consoleActor"]`, `form["browsingContextID"]`, ...) when needed.

Next: [[evaluate-js]], [[watch-resources]], [[take-screenshot]].
