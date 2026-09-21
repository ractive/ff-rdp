---
type: rdp-note
tags:
- rdp
- firefox-server
- actor
- tab
date: 2026-05-24
firefox_files:
- devtools/shared/specs/descriptors/tab.js
- devtools/server/actors/targets/window-global.js
title: TabDescriptorActor
---

# TabDescriptorActor

Describes a browser tab as a debuggable target. The tab descriptor is obtained
from the root actor's `listTabs()` response and is used to attach to a specific
tab's target for inspection, console access, and network monitoring.

## Firefox references

| File | Lines | Purpose |
|------|-------|---------|
| `devtools/shared/specs/descriptors/tab.js` | 1-69 | Protocol spec — descriptor form, getTarget |
| `devtools/server/actors/targets/window-global.js` | 1-2055 | WindowGlobalTarget implementation (backing target) |

## Key methods (from spec)

- `getTarget()` — returns the `WindowGlobalTargetActor` for this tab.
- `getFavicon()` — returns the tab's favicon data URL.
- `getWatcher({ isServerTargetSwitchingEnabled?, isPopupDebuggingEnabled? })` —
  returns the tab's `WatcherActor`. Both options are `Option(0, "boolean")`
  per `devtools/shared/specs/descriptors/tab.js:24-28`.

## `getWatcher` and `isServerTargetSwitchingEnabled` (iter-129)

`TabActor::get_watcher` (`crates/ff-rdp-core/src/actors/tab.rs`) sends
`getWatcher` with no arguments — matches every pre-iter-129 call site and
Firefox's default (server-side target switching **disabled**). In that mode
`watchTargets("frame")` on the returned watcher yields **zero**
`target-available-form` events — not even for the top-level target, which is
instead delivered by the descriptor's own `getTarget`.

`TabActor::get_watcher_with_options(transport, tab_actor,
is_server_target_switching_enabled: Option<bool>)` is the opt-in variant:
`Some(true)` sends `{"isServerTargetSwitchingEnabled": true}`, which flips the
watcher into spawning a `target-available-form` for **every** window-global
target — top level and every iframe, same-origin or cross-origin/out-of-process
(Fission), uniformly. This is the mechanism [[enumerate_frame_targets]] (see
[[watcher#TargetEvent-extra-actor-fields-iter-129|watcher.md]]) is built on,
and in turn what `click`'s frame-scan fallback and `ff-rdp consent accept`
use to reach cross-origin CMP iframes. Empirically verified against live
Firefox 152/153 in [[frame-targets]] (2026-07-20 research spike).

**CAUTION** — enabling the flag changes where the top-level target is
delivered. The daemon startup watcher enables it; protocol 2 clients obtain
that descriptor's target from its watcher snapshot (see below). Direct and
unmanaged descriptors retain their existing `getTarget` path. Frame-aware
call sites also enable the flag for `enumerate_frame_targets`.

## `getTarget` frame → `TargetInfo`

The `getTarget` reply wraps the target in a `frame` object carrying the target
actor plus a set of per-target sub-actor ids (created lazily by the server on
first access). ff-rdp parses the ones it consumes into `TargetInfo`
(`crates/ff-rdp-core/src/actors/tab.rs`): `actor` (WindowGlobalTarget),
`consoleActor`, `threadActor`, `inspectorActor`, `screenshotContentActor`,
`accessibilityActor`, `responsiveActor`, **`manifestActor`** (iter-104 — read
into `TargetInfo::manifest_actor`, drives `ManifestFront::fetch_canonical_manifest`
for the `ff-rdp manifest` command; see [[manifest]]), and `browsingContextID`.
Absent optional fields deserialize to `None`, so older Firefox builds that omit
a sub-actor are tolerated.

### `innerWindowId` and `url` (iter-220)

Two further frame fields are read into `TargetInfo::inner_window_id` and
`TargetInfo::url`. They exist because **`getTarget` keeps returning the
outgoing document while a navigation is in flight** — verified on live Firefox
153 on 2026-08-30, `en.wikipedia.org/wiki/Ada_Lovelace` → `Charles_Babbage`:

```
getTarget → frame.actor  = server1.conn2.child81/windowGlobalTarget2
            innerWindowId = 15032385539    url = …/Ada_Lovelace
<click; tabNavigated {state:"start", url: …/Charles_Babbage}>
getTarget → frame.actor  = server1.conn2.child83/windowGlobalTarget2   ← NEW prefix
            innerWindowId = 15032385539    url = …/Ada_Lovelace        ← SAME document
```

So the actor id is **not** a usable "did the document change" key: the
descriptor re-forwards the same docshell under a fresh `childN/` prefix on every
call. `innerWindowId` is, and it is also the join key against the watcher's
`target-destroyed-form` (whose own `target.actor` lives in an unrelated
`watcherN.processN//` namespace and therefore cannot be matched by id).

`url` covers the case `innerWindowId` cannot: a same-document `#fragment`
navigation flips the URL and never changes the id.

Both are consumed by `page_view::collect_settled` and
`RdpTransport::set_target_guard` — see [[iteration-220-with-page-after-navigating-click]].

## Status

Stub — backfilled in iter-73; `getTarget` frame fields documented in iter-104;
`innerWindowId`/`url` and the stale-frame finding added in iter-220.

## Daemon-owned watcher targets (iter-262 correction)

`TargetInfo::from_watcher_form` parses the raw watcher form using the same field
validation as `getTarget`, preserving BC, inner-window and optional actor metadata.
For the exact descriptor bound to the primary startup watcher, daemon protocol 2
clients resolve a snapshot over an authenticated disposable local connection.
They never issue legacy `getTarget` for that descriptor, including during the
pending destruction gap. The initial BC is metadata, not permanent tab identity;
matching watcher/descriptor identity owns replacement forms. A lazy watcher on a
separate Firefox connection is excluded. Direct/unmanaged descriptors retain
`getTarget`. Raw external proxy consumers are outside this prevention guarantee.

A live snapshot is not a lifetime promise. Navigation re-resolves on subsequent
probe ticks until completion, retaining epoch and URL freshness checks. Socket
queries have one absolute per-read/write deadline and cannot consume main-stream
events or own the main RPC slot. Protocol/auth/query failures never trigger legacy
fallback. Older daemon versions require restart rather than silent compatibility.
The correction still awaits real-Firefox pre/post proof and original sweep gates.

### Submission callers and target handover (iteration262)

`type --submit` treats a navigation-start or matching destruction as a reason
to stop evaluating the outgoing console. It does not treat that event as
replacement readiness. The submission action, navigation polls and target
handover share one absolute deadline beginning before Enter. `ConnectedTab`
polls outgoing/Pending snapshots without installing them and installs a
replacement only when its inner-window identity or current URL differs from
the submission origin. Exhaustion returns a submission-handover timeout before
`--settle`, `--wait-for` or `--with-page` can evaluate the displaced document.
The later settle/predicate/page operations retain their existing own budgets.

A lifecycle interruption before an evaluation acknowledgement retires its
late reply using the existing console reply-abandonment mechanism; an
interruption after acknowledgement retains result-ID matching. This is
single-connection evaluation handling, not a change to daemon RPC ownership.
Offline coverage drives the real type caller across outgoing, Pending and
replacement snapshots, both acknowledgement/result interruptions, and bounded
exhaustion. Firefox validation of this caller delta remains required.

A Live snapshot whose target disappears during the current-URL query is also
retryable Pending, but only for a typed lifecycle failure. Submission callers
keep the original deadline across that race. Offline caller coverage includes
`noSuchActor`, destruction without a metadata reply, late old metadata replies,
same-document URL changes, and a non-lifecycle protocol error that must fail
without retrying.
