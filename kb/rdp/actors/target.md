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

## Iteration262 correction: watcher target URL freshness

`WindowGlobalTarget::current_url` sends the declared `listFrames` request to the
already selected actor, using the ordinary actor-matched reply path. Installed
Firefox156.0 (BuildID20260909172920, SourceStamp
`a80bd15ddee3b4bf3679aeba340e9d2db933c467`) implements `listFrames` by calling
`_docShellsToWindows`; `_docShellToWindow` reads `window.location.href` and marks
the original top document with `isTopLevel`. This is read from the installed
`browser/omni.ja`, not inferred from a legacy target's event behavior.

The daemon's availability form is a document/actor identity snapshot; its URL can
be stale after fragment/history changes. Eligible resolution therefore refreshes
the URL from this actor before returning `TargetInfo`, under the same absolute
deadline. Only a top-level frame with no parent contributes the URL. Replies from
other actors and child frames cannot supply it. A missing top frame clears the URL
rather than claiming the cached value is current. No legacy descriptor lookup,
new RPC owner, or daemon-local query subscription is introduced. The side snapshot
connection itself remains local and disposable; this metadata request uses the
caller's existing main transport and event handling.

Non-live coverage exercises wrong-actor and child-frame filtering, and the actual
snapshot-resolver/settlement path for the same actor/inner-window with a changed
URL. Real Firefox regression and gates remain pending at the source-only freeze.

### Metadata acquisition during target handover (iteration262)

A watched target form is a sample: Firefox may destroy its WindowGlobal before
`listFrames` answers. The CLI scopes the transport lifecycle guard to the
sampled `innerWindowId`, restoring any outer guard on every exit. A typed
`UnknownActor` reply or matching lifecycle interruption returns Pending to
the existing acquisition loop, which retains its absolute deadline. Endpoint,
authentication and other protocol errors remain errors.

`WindowGlobalTarget::current_url` sends the same `listFrames` request and
retains its top-frame-only URL parsing. If the reply wait is interrupted or
times out after sending, it retires that untyped reply through the transport's
existing abandoned-reply mechanism. A delayed reply cannot supply a later
request's URL. An actual actor error has already consumed its reply and is not
retired again. Same-document fragment/history changes still use the current
URL, not the stale availability form.
