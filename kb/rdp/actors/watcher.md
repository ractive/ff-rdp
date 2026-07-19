---
type: rdp-note
tags:
  - rdp
  - firefox-server
  - actor
  - watcher
  - critical
date: 2026-05-23
firefox_files:
  - devtools/server/actors/watcher.js
  - devtools/shared/specs/watcher.js
  - devtools/server/actors/watcher/ParentProcessWatcherRegistry.sys.mjs
  - devtools/server/actors/watcher/session-context.js
  - devtools/server/actors/resources/index.js
title: WatcherActor
---

# WatcherActor (typeName `"watcher"`)

The backbone of modern devtools. **All resource streaming** (network events, console messages, document-events, css changes, …) flows through this actor.

- Source: `devtools/server/actors/watcher.js` (952 lines).
- Spec: `devtools/shared/specs/watcher.js`.

Obtained via `TabDescriptorActor.getWatcher({ isServerTargetSwitchingEnabled, isPopupDebuggingEnabled })` (or `ProcessDescriptor.getWatcher`). The descriptor calls `new WatcherActor(conn, sessionContext)` and `manage()`s it.

## Session Context

A WatcherActor is bound to a `sessionContext` (`actors/watcher/session-context.js`):

```
SESSION_TYPES = { ALL, BROWSER_ELEMENT, WEBEXTENSION, WORKER, CONTENT_PROCESS }
```

For `ff-rdp` the relevant type is `BROWSER_ELEMENT` — bound to one `<browser>` element identified by `browserId`. Stored in `this._browserElement`.

## Methods

| Method | Args | Returns | Behavior |
|---|---|---|---|
| `watchTargets` | `targetType: string` | `{}` | Start observing one target type. Spawns existing matching targets and emits `target-available-form` for each, then for any new ones. Target types: `"frame"` (WindowGlobal), `"process"` (ContentProcessTarget), `"worker"`, `"service_worker"`, `"shared_worker"`. |
| `unwatchTargets` | `targetType, options?` | oneway | Stop observing. |
| `watchResources` | `resourceTypes: array:string` | `{}` | Subscribe to one or more resource types. Causes IPC to relevant content processes via `DevToolsProcess` JSProcessActor. Existing resources are emitted, then live. |
| `unwatchResources` | `resourceTypes` | oneway | |
| `clearResources` | `resourceTypes` | oneway | Drops accumulated network-event / console-message caches. |
| `getParentBrowsingContextID` | `browsingContextID` | `nullable:number` | |
| `getNetworkParentActor` | — | `networkParent` | Throttling/blocking/persistence config (parent-process). |
| `getBlackboxingActor` | — | `blackboxing` | |
| `getBreakpointListActor` | — | `breakpoint-list` | |
| `getTargetConfigurationActor` | — | `target-configuration` | Cache disable, viewport CSS, color-scheme sim. |
| `getThreadConfigurationActor` | — | `thread-configuration` | Pause-on-exception, etc. |

## Events

The 5 events ff-rdp must handle:

- `target-available-form` — `(targetForm)` — new target. `targetForm.actor` is the target actorID, `.targetType` is `"frame" | "process" | …`. Use this to wire your console/inspector against the target.
- `target-destroyed-form` — `(targetForm, options?)`.
- `resources-available-array` — `(array)` where each entry is `[resourceType, resourcesArray]`. **Throttled by 100 ms** (see `RESOURCES_THROTTLING_DELAY`, line 65).
- `resources-updated-array` — same shape, partial deltas (e.g. network-event-update fields).
- `resources-destroyed-array` — same shape, for resources that go away (rare).

The throttle batches `available/updated/destroyed` queues into `#throttledResources`, flushed via `throttle(this.emitResources, 100)`.

## Lifecycle / IPC

- On `destroy()`: iterates `ChromeUtils.getAllDOMProcesses()` and IPCs `destroyWatcher({watcherActorID})` on every `DevToolsProcess` JSProcessActor — fan-out cleanup across all content processes.
- The Browser Toolbox session uses `BrowserToolboxDevToolsProcess` instead, to live in a distinct compartment so it can debug system code.

## Resource types (from `actors/resources/index.js` `TYPES`)

```
console-message, css-change, css-message, css-registered-properties, document-event, error-message,
last-private-context-exit, network-event, network-event-decoded-body-size, network-event-stacktrace,
platform-message, reflow, server-sent-event, session-history, source, stylesheet, thread-state,
jstracer-trace, jstracer-state, websocket, webtransport,
Cache, cookies, extension-storage, indexed-db, local-storage, session-storage,
extensions-backgroundscript-status
```

See [[rdp/resources/README|resources/]] for each.

## Method support matrix

State of the `WatcherFront` (`crates/ff-rdp-core/src/fronts/watcher.rs`) after iter-61u.  "Spec" = present in `crates/ff-rdp-core/src/specs/watcher.rs`; "Front" = a typed Rust method exists on `WatcherFront`; "Wired" = called from production code paths (daemon or CLI commands), not only tests.

| Method | Spec | Front | Wired | Notes |
|---|---|---|---|---|
| `watchTargets` | yes | `watch_targets` | yes | Daemon engagement + `commands/navigate.rs`. |
| `unwatchTargets` | yes (oneway) | `unwatch_targets` | yes | Used on daemon shutdown to avoid hang (iter-61n). |
| `watchResources` | yes | `watch_resources` | yes | Via `ResourceCommand::subscribe` (iter-61q/t). |
| `unwatchResources` | yes (oneway) | `unwatch_resources` | yes | |
| `clearResources` | yes (oneway) | `clear_resources` | primitive | Front exists; no production call site yet. |
| `getParentBrowsingContextID` | yes | `get_parent_browsing_context_id` | primitive | iter-61u — Front only. |
| `getNetworkParentActor` | yes | `get_network_parent_actor` | wired | iter-109 — `NetworkParentFront` + `throttle` CLI command (network throttling / URL blocking).  Reply shape corrected to the nested `{networkParent: {actor}}` form (was flat `ActorRef`) — see below. |
| `getBlackboxingActor` | yes | `get_blackboxing_actor` | primitive | iter-61u — Front only. |
| `getBreakpointListActor` | yes | `get_breakpoint_list_actor` | primitive | iter-61u — Front only. |
| `getTargetConfigurationActor` | yes | `get_target_configuration_actor` | primitive | iter-61u; `TargetConfigurationFront` exists but not yet called from a CLI command. |
| `getThreadConfigurationActor` | yes | `get_thread_configuration_actor` | primitive | iter-61u — Front only. |

See [[from-our-codebase/wired-vs-primitive]] for the broader wired-vs-primitive snapshot across iter-61p..61u landings.

## Oneway methods — important protocol constraint (iter-74)

`unwatchTargets`, `unwatchResources`, and `clearResources` are all declared `oneway: true` in `devtools/shared/specs/watcher.js`. Firefox **never** sends a reply packet for these. Calling `actor_request` on them would hang until the socket read timeout.

In ff-rdp these are now routed through `actor_send` (which writes the packet and returns immediately). The `WatcherActor::unwatch_resources`, `unwatch_targets`, and `clear_resources` methods all return `Result<(), ProtocolError>` — no `Value` reply.

Contrast with `walker.releaseNode` (`devtools/shared/specs/walker.js:127-133`): it is response-less in practice but is **not** declared `oneway: true` in the spec, so it correctly remains an `actor_request`. Do not conflate "no useful reply value" with "oneway" — only the spec annotation determines oneway status.

## target-destroyed-form — registry invalidation (iter-74)

When the watcher emits `target-destroyed-form`, ff-rdp calls `Registry::invalidate_target` on the target actor, which cascades to all dependent fronts (inspector, walker, console) registered with that `target_root`. This prevents stale-actor errors on subsequent operations.

The Rust entry points are:
- `WatcherEvent::TargetDestroyed { target, options }` — parsed from the packet
- `dispatch_watcher_event(packet, registry)` — combines parsing + registry invalidation
- Called in `daemon/server.rs::handle_target_event`

## Gotchas for ff-rdp

- **Iframe-before-top-level race**: bfcache navigations can deliver iframe targets before the top target. `_earlyIframeTargets` caches them until the top arrives (see comment block ~L123).
- **Throttle delay** means a tiny burst of network events can be batched into one `resources-available-array` packet — your handler must iterate.
- A WatcherActor will not see anything until you `watchTargets("frame")` AND `watchResources([...])`. Resources alone get nothing.
- `getNetworkParentActor()` must be the path to set throttling — the per-event NetworkEventActor only reads, never writes.
- **`getNetworkParentActor` reply is nested (iter-109):** like `getTargetConfigurationActor` (iter-103), the actor ID is returned under a named typed-actor key — `{"networkParent": {"actor": "<id>", …}, "from": …}` — not at the top level. `spec::response::NetworkParentActorRef` reads `networkParent.actor`; `WatcherFront::get_network_parent_actor` unwraps it. The flat `ActorRef` shape (still used by the blackboxing/breakpoint-list/thread-configuration accessors) was wrong for this method.
- The registry lives in `ParentProcessWatcherRegistry.sys.mjs` (singleton, `global: "shared"`) — devtools can only have one logical view of the watcher set per process tree.

## Iter-76 update — ResourceGripGuard

- Watched resources (consoleAPICall, evaluationResult) may embed grip actor IDs. The watcher now wraps these in `ResourceGripGuard`, which drops the underlying `ScopedGrip` handles when the subscription is dropped, enqueueing release on the transport-shared release queue.
- Closes the `actor-leak-in-daemon` open gap (kb/rdp/from-our-codebase/open-gaps.md:36).

## Iter-76b update — extract_grips + type-safe dispatch

- `extract_grips(event: &Value) -> Vec<Grip>` (re-exported from `ff_rdp_core`) walks resource payloads for embedded grip actor IDs.
  Paths walked: `array[*][1][*].message.arguments`, `array[*][1][*].message.styles`, `result`, `exception`.
  Returns `Grip::Object` for `{type:"object"}` and `Grip::LongString` for `{type:"longString"}` sub-values.
- `ResourceGripGuard::add_grip(grip: Grip)` now dispatches to `AnyGripHandle::Object(GripHandle::<ObjectGrip>)` or `AnyGripHandle::LongString(GripHandle::<LongStringGrip>)` — a `LongString` actor is no longer wrongly wrapped as an `ObjectGrip`.
- `dispatch_firefox_message` in `daemon/server.rs` calls `extract_grips` and `add_grip` so grips are actually released when the guard drops (was inert in iter-76).
- The `grip_release_drainer_loop` thread now genuinely owns `ReleaseQueueRx` and sends release packets over the shared `FramedWriter`.

## Iter-77 update — unwatchTargets options + printf substitution

- `WatcherActor::unwatch_targets` now takes `Option<&str>` for `targetType`
  and `Option<&Value>` for `options`.  Passing `target_type = None` is
  rejected with `RdpError::Spec { reason: "targetType required" }` and NO
  packet is sent (closes the silent default-to-`"frame"` from W4 in the
  iter-73 review).  `WatcherFront::unwatch_targets` mirrors this with an
  `options: Option<Value>` parameter; `request::UnwatchTargets` skips
  serialising `options` when `None`.
- `parse_console_resources` now applies Firefox's `%s`/`%d`/`%i`/`%f`/
  `%o`/`%O`/`%c`/`%%` substitution to the first argument when it is a
  format string — ported from `devtools/server/actors/webconsole.js:1100-1175`.
  `%c` consumes its arg silently (no CSS in our text output).
- `parse_target_event` now rejects empty `actor` strings via the new
  `ActorId::try_new` constructor — closing L2 from the iter-73 review.

## Iter-101 update — top-level target-switch re-watch + buffer purge

**What the watcher re-delivers on a target switch (and what it does not).**
Because ff-rdp's daemon subscribes to resources at the **tab-scoped
WatcherActor** level (`watchResources` on the watcher, not on a per-target
front), a server-side target switch — including a *cross-process* top-level
switch, which emits `target-destroyed-form` for the old top target and
`target-available-form` for the new one — is **transparent to resource
delivery**: Firefox automatically re-emits `resources-available-array` for the
new target under the same watcher actor. The daemon therefore does **not** need
to re-issue `watchResources` per new target the way Firefox's own
`resource-command.js:486-517` client does for its per-target fronts.

What the daemon *did* lack (fixed in iter-101 Theme A) was **buffer hygiene**
across the switch:

- `handle_target_event` now branches on `is_top_level`
  (`TargetEvent.is_top_level`, parsed but previously never consumed).
- `SharedState.top_level_target` tracks the current top-level target actor.
- On a top-level `target-available-form` whose actor **differs** from the
  tracked one (a genuine cross-process switch), `handle_top_level_target_switch`
  calls `ResourceBuffer::purge_destroyed_target`, dropping the outgoing
  document's stale buffered resources so a post-switch drain window
  (`network --since`, `console` drain) never mixes in dead-target state.
  Nav-boundary bookkeeping is left intact (the switch does not rewind
  `total_inserted`, so existing `store_start` values stay valid).
- The **first** top-level target (session start) and a same-actor
  re-announcement do **not** purge — there is no prior document to discard.

Registry invalidation for the destroyed target still runs via
`dispatch_watcher_event` → `Registry::invalidate_target` (iter-74); Theme A only
adds the buffer purge on top.

## Iter-111 update — live coverage for the target-switch path

[[iteration-111-daemon-live-coverage]] adds the live end-to-end proof that the
transparent-re-delivery + buffer-purge behaviour above actually holds through a
real cross-process switch:
`live_daemon_follow_survives_cross_process_nav`
(`crates/ff-rdp-cli/tests/live/live_111_daemon_follow_cross_process.rs`) opens a
daemon-proxied `network --follow` stream, drives a top-level (and, under
`FF_RDP_LIVE_NETWORK_TESTS=1`, a genuine Fission example.com → wikipedia.org)
navigation, and asserts a **post-nav-sourced** `navigation` event still reaches
the still-open stream — i.e. the watcher subscription is not stranded on the
destroyed target.

Two practical constraints this test surfaced (relevant to anyone building on the
watcher follow path):

- `console --follow` is **not** a viable live signal for the switch: ordinary
  `console.log` is delivered as a direct console-actor push and is *not* routed
  through the watcher `console-message` resource stream on the tested Firefox,
  so a daemon follow never observes it. `network --follow` (navigation /
  network-event resources) is the reliable stream.
- A follow stream holds the daemon's single RPC-writer slot (iter-101 Theme B),
  so the page must be driven with `--no-daemon` while a daemon-proxied follow is
  open — a second daemon-routed command is refused with `daemon_busy`.

## Iter-122 update — `document-event` / `dom-complete` may never fire on FF152

[[iteration-122-navigate-dom-complete-ff152]] found that on Firefox 152 the
`document-event` resource stream can go quiet for a page that has, in fact,
finished loading: `dom-complete` (and sometimes `dom-loading` with a real URL)
simply never arrives for some static pages and SPAs, even though
`document.readyState` is already `"complete"` and `location.href` holds the real
URL. Confirmed on a clean single instance: default `navigate` to
`example.com`-class pages burned ~7 s (the full events budget) before the
readystate fallback rescued it — while `--no-wait` returned in 0.06 s with the
page already loaded.

Mitigation in ff-rdp (does **not** change the watcher protocol usage, which
already subscribes correctly per the `watchTargets("frame")` +
`watchResources` contract above):

- The default `--wait-strategy both` now **interleaves a lightweight
  `document.readyState` probe** into the `document-event` drain loop
  (`wait_for_doc_complete` in `crates/ff-rdp-cli/src/commands/navigate.rs`). It
  returns as soon as the page reports `complete` (guarded by the iter-92
  `navigationStart > pre_epoch` freshness check), instead of blocking the whole
  events budget waiting for a `dom-complete` that may never come. Pages that
  *do* fire `dom-complete` promptly (comparis: ~0.69 s) still take the richer
  event path — the probe is given a 300 ms head start and only runs every
  ~250 ms so events keep priority.
- When a committing `document-event` carries **no URL** (the SPA case), the
  committed URL is resolved via `window.location.href` rather than surfaced as
  `about:blank`.
- `elapsed_ms` is now measured from the single navigate-start `Instant` across
  both the events and readystate phases, so it reflects true wall-clock instead
  of only the ~1 ms readystate-poll duration.
