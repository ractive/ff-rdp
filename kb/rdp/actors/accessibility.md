---
type: rdp-note
tags:
  - rdp
  - firefox-server
  - actor
  - accessibility
date: 2026-05-23
firefox_files:
  - devtools/server/actors/accessibility/accessibility.js
  - devtools/server/actors/accessibility/walker.js
  - devtools/server/actors/accessibility/accessible.js
  - devtools/shared/specs/accessibility.js
title: AccessibilityActor
---

# AccessibilityActor (typeName `"accessibility"`)

Entry-point for the accessibility tree, audits and simulators.

- Source: `devtools/server/actors/accessibility/accessibility.js` (130 lines — orchestrator).
- Plus: `walker.js`, `accessible.js`, `audit.js`, `simulator.js`, `parent-accessibility.js` in same dir.
- Spec:   `devtools/shared/specs/accessibility.js`.

## Methods (delegated to sub-actors)

- `bootstrap()` — returns `{enabled, canBeDisabled, canBeEnabled}`.
- `getWalker()` → AccessibilityWalkerActor.
  - `children(accessible)`, `getAncestry`, `getAccessibleFor(domnode)`.
  - `audit(progress callback)` — runs a11y audit, returns issues.
  - `highlightAccessible(acc, options)`, `unhighlight`.
- `getSimulator()` → SimulatorActor — color-vision simulators (protanopia, achromatopsia, contrast-loss, …).

## Events

- `init` / `shutdown` — accessibility service started/stopped.
- `can-be-disabled-change`, `can-be-enabled-change`.

## Lifecycle

- One per target. Lives until target destroyed.
- Calling `enable()` instantiates the global Gecko accessibility service if not already running. Calling `disable()` may fail if other consumers (screen readers) are using it.

## Gotchas

- Accessibility service is a system-wide singleton. Once enabled, performance cost persists until shutdown.
- On Windows, an active screen reader can prevent `disable`.
