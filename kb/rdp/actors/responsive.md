---
type: rdp-note
tags:
- rdp
- firefox-server
- actor
- responsive
date: 2026-05-24
firefox_files:
- devtools/shared/specs/responsive.js
- devtools/server/actors/emulation/responsive.js
title: ResponsiveActor
---

# ResponsiveActor

Controls responsive design mode (RDM) for a target. Allows setting a virtual
viewport width and height, device pixel ratio, and touch emulation. Used by
ff-rdp to simulate different screen sizes without resizing the browser window.

## Firefox references

| File | Lines | Purpose |
|------|-------|---------|
| `devtools/shared/specs/responsive.js` | 1-35 | Protocol spec — setViewportSize, touch emulation |
| `devtools/server/actors/emulation/responsive.js` | 1-74 | Server implementation |

## Key methods (from spec)

- `setViewportSize({width, height})` — override the viewport dimensions.
- `setDPPX(dppx)` — set the device pixel ratio.

## Status

Stub — backfilled in iter-73; expand on next touch.
