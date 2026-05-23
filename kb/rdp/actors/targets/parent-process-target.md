---
type: rdp-note
tags: [rdp, firefox-server, actor, target]
date: 2026-05-23
firefox_files:
  - devtools/server/actors/targets/parent-process.js
  - devtools/shared/specs/targets/parent-process.js
---

# ParentProcessTargetActor (typeName `"parentProcessTarget"`)

The Browser Toolbox's main target: the parent-process `browser.xhtml` and chrome window.

- Source: `devtools/server/actors/targets/parent-process.js` (179 lines).
- Extends [[window-global-target]] (it **is** a WindowGlobalTarget that happens to be the top chrome doc).

## Notable

- Loaded into the `shared` global so it can debug devtools own code and Firefox internals.
- Spec is a thin wrapper around `windowGlobalTargetSpec` plus an extra trait.
- Reached via `ProcessDescriptor(parent).getTarget()` — only when **not** xpcshell / background-task.

## Gotchas

- Inspecting it shows the **browser chrome DOM** (`browser.xhtml`), not the loaded page content.
- Toggling `getTargetConfigurationActor` on this target affects chrome behaviour; usually you want a watcher with session type `ALL` instead.
