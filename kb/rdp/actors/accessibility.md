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

- `bootstrap()` — returns `{state: {enabled, canBeDisabled, canBeEnabled}}`. On the
  content accessibility actor the state is just `{enabled}`; on the root's
  `parentAccessibilityActor` it carries `canBeEnabled` / `canBeDisabled`.
- `getWalker()` → AccessibilityWalkerActor.
  - `children()` — **takes no arguments**. It always resolves to a
    single-element array holding the root document accessible
    (`walker.js` → `children()` delegates to the internal `getDocument()`
    helper). Passing an `accessible` argument does nothing: protocol.js drops
    unknown request fields, so the walker still answers with the root.
  - `getAncestry(accessible)`, `getAccessibleFor(domnode)`.
  - `startAudit(options)` — runs the a11y audit, streams `audit-event`.
  - `highlightAccessible(acc, options)`, `unhighlight`, `pick`, `showTabbingOrder`.
- `getSimulator()` → SimulatorActor — color-vision simulators (protanopia, achromatopsia, contrast-loss, …).

## AccessibleActor (typeName `"accessible"`)

Each node in the tree is its own actor.

- `children()` — no arguments; returns that node's children. **This** is how you
  descend the tree; the walker's same-named method is only the root accessor.
- `audit(options)`, `getRelations()`, `hydrate()`, `snapshot()`.

## Events

- `init` / `shutdown` — accessibility service started/stopped.
- `can-be-disabled-change`, `can-be-enabled-change`.

## Lifecycle

- One per target. Lives until target destroyed.
- Calling `enable()` instantiates the global Gecko accessibility service if not already running. Calling `disable()` may fail if other consumers (screen readers) are using it.

## Gotchas

- Accessibility service is a system-wide singleton. Once enabled, performance cost persists until shutdown.
- On Windows, an active screen reader can prevent `disable`.
- **There is no `getRootNode` and no `getDocument` packet type** (iter-136). Both
  are absent from `accessibleWalkerSpec`; `getDocument` exists only as an
  internal walker helper. Firefox 153 answers either with
  `unrecognizedPacketType`. Use the walker's argument-less `children()`.
- **The walker stalls, it does not error, while the accessibility service is
  off** (iter-136). `getDocument()` returns `this.once("document-ready")` when
  there is no root accessible yet, and that promise never settles — the request
  simply never gets a reply, so a client blocks until its socket read timeout.
  Check `bootstrap().state.enabled` on the content accessibility actor first
  (`AccessibilityActor::is_service_enabled`); enabling requires `enable()` on
  the root form's `parentAccessibilityActor`, which is a browser-global change
  ff-rdp does not make on the user's behalf — `ff-rdp a11y` falls back to its
  JS-derived tree instead.
