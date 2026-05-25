---
type: rdp-note
tags:
- rdp
- firefox-server
- actor
- target
date: 2026-05-24
firefox_files:
- devtools/shared/specs/targets/window-global.js
- devtools/server/actors/targets/window-global.js
- devtools/server/actors/targets/base-target-actor.js
title: WindowGlobalTargetActor
---

# WindowGlobalTargetActor

The core "target" actor that represents a browsing context (tab, frame, or
process). All per-tab tooling (inspector, console, network monitor, debugger)
is reached through a target. Obtained by calling `getTarget()` on a descriptor.

## Firefox references

| File | Lines | Purpose |
|------|-------|---------|
| `devtools/shared/specs/targets/window-global.js` | 1-163 | Protocol spec — target form, child actors |
| `devtools/server/actors/targets/window-global.js` | 1-2055 | Main implementation |
| `devtools/server/actors/targets/base-target-actor.js` | 1-179 | Shared base class |

## Key methods (from spec)

- `attach()` — attach to the target (returns state and actors).
- `detach()` — detach and release the target.
- `navigate(url)` — navigate the target to a URL.
- `reload({options: {force?: bool}})` — reload the current document.
  iter-80 Theme B wired the optional `options.force` into
  `WindowGlobalTarget::reload(transport, target, force)` and `ff-rdp reload
  --hard` so callers can bypass the HTTP cache. See
  [[rdp/actors/targets/window-global-target]] for the wire-level spec.

## Status

Stub — backfilled in iter-73; iter-80 expanded `reload` to surface the
optional `options.force` (Firefox spec key — *not* `forceReload`) argument.
