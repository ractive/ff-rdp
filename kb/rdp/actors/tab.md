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

## Status

Stub — backfilled in iter-73; `getTarget` frame fields documented in iter-104.
