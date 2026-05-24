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

## Status

Stub — backfilled in iter-73; expand on next touch.
