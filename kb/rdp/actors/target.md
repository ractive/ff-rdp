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
  iter-102 Theme B routed the `force=true` path through the matched
  `actor_request`/`recv_reply_from` reply path (`fronts/target.rs`) — it was
  the last production caller of the blind `transport.request` (send + one
  *unmatched* recv), which has been removed. The matched path routes an
  interleaved `tabNavigated` push (the reload's most likely moment) to the
  event sink instead of consuming it as the reply, so the actor's reply stream
  no longer desyncs. Unit test:
  `reload_force_tolerates_tab_navigated_push_before_reply`. Live AC:
  `live_reload_force_with_watched_resources`.

## Status

Stub — backfilled in iter-73; iter-80 expanded `reload` to surface the
optional `options.force` (Firefox spec key — *not* `forceReload`) argument.
