---
type: rdp-note
tags: [rdp, firefox-server, actor, target]
date: 2026-05-23
firefox_files:
  - devtools/server/actors/targets/content-process.js
  - devtools/shared/specs/targets/content-process.js
---

# ContentProcessTargetActor (typeName `"contentProcessTarget"`)

Represents a whole content **process** as a debug target. Returned by `ProcessDescriptor.getTarget()` for non-parent processes (and for xpcshell/background-task parent processes).

- Source: `devtools/server/actors/targets/content-process.js` (269 lines).
- Spec:   `devtools/shared/specs/targets/content-process.js`.

## Child actors exposed in form

- `consoleActor` — process-level [[../console]] (no window — runs in chrome compartment).
- `threadActor`, `memoryActor`, `tracerActor`.
- `webconsoleActor` (alias).

No DOM walker, no inspector — this target has no document.

## Methods

- `listWorkers()`, `pauseMatchingServiceWorkers()`.

## Lifecycle

- Created when `connectToContentProcess` IPC succeeds.
- Destroyed when the process exits.
- Used by the Browser Console / Browser Toolbox to surface process-level diagnostics.

## Gotchas for ff-rdp

- Not relevant for normal "debug a tab" flows.
- Targeting **all** content processes is how Browser Toolbox observes things like cross-process worker logs.
