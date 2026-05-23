---
type: rdp-note
tags: [rdp, firefox-server, actor, descriptor]
date: 2026-05-23
firefox_files:
  - devtools/server/actors/descriptors/process.js
  - devtools/shared/specs/descriptors/process.js
---

# ProcessDescriptorActor (typeName `"processDescriptor"`)

Represents a parent or content process. Returned by `RootActor.listProcesses()` and `getProcess(id)`.

- Source: `devtools/server/actors/descriptors/process.js` (251 lines).
- Spec:   `devtools/shared/specs/descriptors/process.js`.

## Methods

- `getTarget()` — returns:
  - **Parent process** (`isParent`): `ParentProcessTargetActor` (a WindowGlobalTarget subclass for the browser chrome). Exception: in xpcshell or background-task mode, returns a `ContentProcessTargetActor` (no chrome doc).
  - **Content process**: connects via `connectToContentProcess` and returns the `ContentProcessTargetActor` over IPC.
- `getWatcher({enableWindowGlobalThreadActors?})` — creates a [[../watcher]] with `BROWSER_TOOLBOX` (a.k.a `ALL` session type) when called on the parent process; spawns targets for every BrowsingContext in the browser.

## Lifecycle

- Created lazily by RootActor. Process id 0 is the parent.
- The parent-process target is loaded into the **shared** module loader, not contextual, so it can debug system code.

## Gotchas

- Used by the Browser Toolbox: `getProcess(0).getWatcher({…})` is the entry into "debug all of Firefox".
- A normal tab debugging session uses [[tab-descriptor]] not this.
- `connectToContentProcess` requires the content process to be alive — calling on a just-crashed process throws.
