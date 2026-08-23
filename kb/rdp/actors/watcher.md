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

### `unwatchResources` is destructive, not merely a subscription decrement (iter-164)

`unwatchResources` destroys the parent-process resource watcher for the named
types, along with any state that watcher owns. For `network-event` that state
includes the `NetworkObserver` holding the session's URL block-list and
throttling config — so a single `unwatchResources(["network-event"])` silently
undoes `setBlockedUrls` / `setNetworkThrottling` (see [[network-parent]]).
It is *not* ref-counted server-side: one client's unwatch wipes the
subscription for every other user of that connection.

Consequence for the daemon: the resource subscriptions belong to the daemon,
which installs them at startup and keeps them for the session, so the daemon
drops a proxied client's `unwatchResources` for daemon-owned types
(`network-event`, `console-message`, `error-message`) exactly as it already
drops `unwatchTargets`. Being `oneway`, dropping either frame leaves no client
waiting. See DEC-037 in [[decision-log]].

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

## Iter-129 update — frame-target extra-actor fields + `enumerate_frame_targets`

Settled by the [[frame-targets]] research spike (2026-07-20): `watchTargets`
already delivers a `target-available-form` per iframe (same-origin AND
cross-origin/out-of-process, uniformly) — but ONLY when the tab's watcher was
obtained via `TabActor::get_watcher_with_options(..., Some(true))` (see
[[tab#getWatcher-and-isServerTargetSwitchingEnabled-iter-129|tab.md]]). Without
that flag, `watchTargets("frame")` on a default watcher yields **zero** target
forms — not even the top-level target.

### `TargetEvent` extra-actor fields (iter-129)

`parse_target_event` (`crates/ff-rdp-core/src/actors/watcher.rs`) now also
extracts, from the same opaque `target-available-form` blob (`Arg(0,"json")`
per `devtools/shared/specs/watcher.js:96-105` — no spec drift, the shape is
undeclared):

- `console_actor: Option<ActorId>` — the target's own WebConsole actor.
  **The payoff**: eval against this actor runs inside that specific frame's
  global, CSP-bypassing (Debugger sandbox), which is how `click`'s frame-scan
  fallback and `ff-rdp consent accept` reach cross-origin CMP iframes
  (e.g. Sourcepoint's `sp_message_iframe_*` on theguardian.com).
- `inspector_actor: Option<ActorId>` — the target's own inspector (unused by
  the eval-based click path; kept for parity/future walker-based work).
- `browsing_context_id: Option<u64>` — stable per-frame id.
- `process_id: Option<u64>` — differs from the top target's `processID` for
  out-of-process (Fission) cross-origin frames; matches it for in-process
  frames (same-origin iframes are ALSO delivered as their own targets, just
  sharing the top's `processID`).

### `enumerate_frame_targets` — the one new primitive

`enumerate_frame_targets(transport, watcher_actor, settle: Duration) ->
RdpResult<Vec<TargetEvent>>` (`crates/ff-rdp-core/src/actors/watcher.rs`,
re-exported from `ff_rdp_core`): issues `watchTargets("frame")` **and**
`watchResources(["document-event"])` — the target-event stream stays dark
until both are sent (same quirk documented above for
`commands/navigate.rs`) — then drains the transport for `settle`
(`DEFAULT_FRAME_TARGETS_SETTLE` = 800ms), deduping `target-available-form` by
actor id and applying `target-destroyed-form` removals, before returning the
snapshot.

**Deliberately does NOT call `unwatchTargets`/`unwatchResources` before
returning.** The whole point is to hand back each target's `console_actor` so
the caller can eval inside that frame — but with
`isServerTargetSwitchingEnabled: true`, unwatching `"frame"` tears down
**every** target Firefox spawned under that switching regime (top level
included), destroying their console/inspector actors with them. Confirmed
live against Firefox 153: an `unwatchTargets` call immediately followed by
`evaluateJSAsync` on a just-returned frame's `console_actor` produced
`target-destroyed-form` for both targets and the eval never got a reply. The
"frame" subscription is left active for the lifetime of the connection —
harmless, since each direct/no-daemon CLI connection is short-lived and daemon
connections already tolerate a standing "frame" subscription the same way
`navigate`'s own prelude does.

Callers **must** have obtained `watcher_actor` via
`get_watcher_with_options(Some(true))` — this is therefore an opt-in helper,
not a change to the default (non-frame-aware) target-acquisition path used by
every other command. First consumers: `click`'s frame-scan fallback
(`crates/ff-rdp-cli/src/commands/click.rs`) and `ff-rdp consent accept` /
`navigate --auto-consent` (`crates/ff-rdp-cli/src/commands/consent.rs`).

### `watchTargets` is NOT repeatable on one connection (iter-137)

`ParentProcessWatcherRegistry.watchTargets(watcher, targetType)`
(`devtools/server/actors/watcher/ParentProcessWatcherRegistry.sys.mjs`) does
nothing but `addOrSetSessionDataEntry(watcher, TARGETS, [targetType], "add")`.
Once `"frame"` is in a watcher's session data, a **second** `watchTargets`
call on the same connection adds nothing and Firefox re-delivers **no**
`target-available-form`. The subscription is per connection, not per request.

Consequence for [[enumerate_frame_targets]]: it only works on a connection
that has not already subscribed. The ff-rdp **daemon** owns the single RDP
connection and subscribes once at startup, so every proxied command's
`watchTargets` was a no-op and the drain window came back empty —
`enumerate_frame_targets` returned **zero** targets, not even the top-level
one. That silently voided every iteration-129 feature (`click --frame`, the
cross-origin frame scan, `consent accept`) in the *default* connection mode
while they kept working under `--no-daemon`.

Fix (iter-137 Theme A), in three parts:

1. The daemon requests its watcher with
   `get_watcher_with_options(..., Some(true))` — without server-side target
   switching the watcher emits no `target-available-form` at all, so the
   daemon saw nothing to record (`daemon status` reported `target_count: 0`
   for whole sessions).
2. The daemon records every raw `target-available-form` /
   `target-destroyed-form` in `SharedState::frame_targets` — deduped by target
   actor id, removed on destroy, and cleared when a *different* top-level
   target actor appears (a cross-process target switch; a re-announcement of
   the same top-level actor is not a switch and keeps its frames).
3. A new daemon request `{"to":"daemon","type":"frame-targets"}` returns the
   recorded packets untouched:
   `{"from":"daemon","type":"frame-targets","targets":[…],"target_count":N}`.
   The CLI replays them through
   `ff_rdp_core::target_events_from_packets(packets)` — the *same*
   add/replace/remove rules `enumerate_frame_targets` applies to a live drain —
   so the daemon and `--no-daemon` snapshots cannot drift.

`crates/ff-rdp-cli/src/commands/frame_targets.rs` is the single entry point
that picks the mechanism per connection (`ConnectedTab::via_daemon`); `click`
and `consent` both go through it. The daemon path polls the snapshot up to
`DEFAULT_FRAME_TARGETS_SETTLE` while it still shows only the top-level target,
so a command issued immediately after `navigate` does not race frame creation.

### Where resource events are addressed from (iter-159)

**`isServerTargetSwitchingEnabled: true` does not move `network-event`
delivery off the watcher actor.** This was the load-bearing question of
iter-159 and it was answered on the wire, not by reading the spec twice.

Recorded frame (fixture
`crates/ff-rdp-cli/tests/fixtures/resources_available_network_server_target_switching.json`,
captured by `live_159_record_resources_available_with_server_target_switching`
against a watcher created exactly the way the daemon creates it):

```json
{"type": "resources-available-array", "from": "server1.conn0.watcher3",
 "array": [["network-event", [{"actor": "server1.conn0.netEvent6",
   "cause": {"type": "document"}, "method": "GET", "isNavigationRequest": true,
   "url": "https://example.com/", …}]]]}
```

`from` is the **watcher** actor. The reason is structural, not incidental:
`devtools/server/actors/resources/index.js` keeps two dictionaries that both
contain `TYPES.NETWORK_EVENT` —

- `ParentProcessResources` → `resources/network-events.js`, watched from the
  **WatcherActor** (which runs in the parent process) and emitted by
  `WatcherActor.notifyResources` → `emitResources`
  (`devtools/server/actors/watcher.js`), i.e. `from: <watcher>`;
- `FrameTargetResources` → `resources/network-events-content.js`, which by its
  own doc comment "only handles events for requests (js/css) blocked by CSP"
  plus cached/data-channel resources.

`WatcherActor.watchResources` splits the requested types with
`Resources.getParentProcessResourceTypes` and handles the parent-process ones
itself before delegating the rest to targets, so ordinary HTTP traffic is
always the watcher's. The target actor *does* declare
`resources-available-array` in its own spec
(`devtools/shared/specs/targets/window-global.js`) and events do arrive with
`from: …//windowGlobalTarget2` — but those belong to a **different** watcher
(a proxied CLI command's own), which is exactly why the daemon's
`is_watcher_event` compares `from` against its own watcher id and must **not**
be widened to accept target actors.

Verified against the 153/154 skew: `devtools/server/actors/resources/index.js`,
`watcher/session-context.js`, `shared/specs/watcher.js`,
`shared/specs/targets/window-global.js` and `shared/specs/descriptors/tab.js`
are byte-identical between `FIREFOX_BETA_153_BASE` and the checked-out
154.0a1 revision `0088392ab4cc`. `watcher.js` differs by 23/-10 lines, all in
an unrelated `browserElement` → `webProgress` refactor plus a new
`getExistingNetworkParentActor` accessor; `notifyResources`, `emitResources`
and `watchResources` are unchanged.

### `tabNavigated` arrives at load *stop* (iter-159)

Measured on Firefox 153: for a plain daemon `navigate` to an
`en.wikipedia.org` article the daemon's `tabNavigated` handler ran after **257**
network entries had already been buffered — i.e. after the whole page load, not
at commit. Anything that treats `tabNavigated` as "the navigation starts here"
will scope a navigation's own requests to the *previous* epoch. The daemon's
network buffer therefore starts each `network-event` epoch at the oldest
*surviving* network entry instead, which is sound because
`network-events.js`'s `#onTopBrowsingContextWillNavigate` destroys the previous
document's request actors and the daemon prunes them on the matching
`resources-destroyed-array`.

### `isServerTargetSwitchingEnabled` also gates the three `dom-*` events (iter-174)

iter-129 established that the flag gates `target-available-form` delivery.
iter-174 measured a second, larger consequence: **without the flag the
content-process `document-event` watcher never runs at all**, so
`dom-loading` / `dom-interactive` / `dom-complete` are never emitted — on any
connection, for any navigation verb.

What still works without it, and is exactly why this looked healthy for four
iterations:

| resource / event | emitted from | arrives without the flag? |
|---|---|---|
| `will-navigate` (`document-event`) | parent process (`WatcherActor`) | **yes** |
| `network-event` (+ `resources-updated-array`) | parent process (`WatcherActor`) | **yes** |
| `dom-loading` / `dom-interactive` / `dom-complete` | content process, per target | **no** |

`watchTargets("frame")` and `watchResources(["document-event", …])` are both
accepted and acked, and the parent-process half of the stream flows normally.
The wire trace of a `reload` on a direct connection (FF154, static localhost
page) is the whole story:

```text
→ watchTargets  {targetType:"frame"}            ← ack, and NO target-available-form
→ watchResources ["document-event","network-event"] ← ack
→ reload
← document-event  will-navigate                 (from watcher, parent process)
← network-event   cause=document                (from watcher, parent process)
… 21 s of nothing …
```

Consequence for ff-rdp: any wait that resolves on `dom-complete` must obtain
its watcher with `getWatcher {isServerTargetSwitchingEnabled: true}`. The
daemon's `establish_watcher` always did; the direct route did not, so
`reload --no-daemon` took 21 011 ms where the daemon took 111 ms, and
`navigate --no-daemon --wait-strategy events` (which has no readystate
fallback) timed out unconditionally. `navigate`'s default `Both` strategy hid
it: its interleaved `document.readyState` poll answered at ~300 ms and nobody
asked which half had produced the answer.

Diagnostic shortcut, should this recur: `RUST_LOG=debug … 2>&1 | grep
"document-event observed"`. `will-navigate` alone — with `network-event`s
still flowing — is this defect, not a dead connection.

The fix lives in `navigate.rs::get_navigation_watcher`, deliberately scoped to
the two navigation waits rather than to `connect_and_get_target`: the flag also
moves top-level target delivery onto the watcher, so callers that hold a target
actor across a navigation must re-resolve it (`refresh_console_actor`), which
those two already did. Suites that subscribe to other content-process
resources on a direct connection (`console --follow`'s `console-message`, and
whatever `click.rs` / `emulate.rs` subscribe to) were **not** audited by
iter-174 — that audit is
`kb/iterations/iteration-189-content-process-resources-on-the-direct-route.md`,
which claims no defect, only an open question. Plain `console` (no `--follow`)
is known-good on the direct route: it primes via `startListeners` on the legacy
target actor, measured working during iter-174.
