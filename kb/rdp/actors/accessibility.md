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
  ff-rdp does not make on the user's behalf by default — `ff-rdp a11y` falls
  back to its JS-derived tree instead unless the caller opts in with
  `--native` (iter-143, see below).

## Opt-in native tree (`ff-rdp a11y --native`, iter-143 Theme B)

`AccessibilityActor::enable_service`/`disable_service` (ff-rdp-core) wrap
`enable()`/`disable()` on `parentAccessibilityActor` (obtained from the root
form's `getRoot` response — `RootActor::get_root`). The CLI's
`run_native_opt_in` (`ff-rdp-cli/src/commands/a11y.rs`):

1. Reads `parentAccessibilityActor` off `getRoot`.
2. Checks `bootstrap().state.enabled` on the content accessibility actor; if
   already `true`, does nothing further (never touches state it did not
   create — [[decision-log#DEC-027]]).
3. Otherwise calls `enable()`, re-checks `bootstrap()`, and errors explicitly
   (never silently falls back) if it still reports disabled.
4. Walks the tree, then calls `disable()` — but only when step 2/3 is what
   turned the service on.

This is opt-in, never the default: `enable()` is browser-global and
process-wide, and its performance cost persists until the browser shuts down.
`ff-rdp a11y` (no flag) never calls it.

## Bounded walker deadline (iter-143 Theme C)

Both the auto-detect path (`run_native_or_js_fallback`) and `--native`
(`run_native_opt_in`/`walk_native_tree_bounded`) narrow the transport's read
timeout to `A11Y_WALKER_TIMEOUT` (3s) around `getWalker`/the root
accessor/`children` calls, restoring the previous timeout afterward. This
bounds the iter-136 stall (walker never replies while the service is off) to
a few seconds instead of the full `--timeout` (default 10s, but
user-configurable much higher) — relevant if a race disables the service
between the `bootstrap()` check and the walk, or a future call site skips the
check.
