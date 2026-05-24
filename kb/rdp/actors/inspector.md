---
type: rdp-note
tags:
- rdp
- firefox-server
- actor
- inspector
date: 2026-05-24
firefox_files:
- devtools/shared/specs/inspector.js
- devtools/server/actors/inspector/inspector.js
title: InspectorActor
---

# InspectorActor

The DOM inspector actor. Provides access to the page's DOM tree via a WalkerActor,
and style information via a PageStyleActor. Entry point for all devtools DOM inspection.

## Firefox references

| File | Lines | Purpose |
|------|-------|---------|
| `devtools/shared/specs/inspector.js` | 1-79 | Protocol spec — methods, forms, child actors |
| `devtools/server/actors/inspector/inspector.js` | 1-362 | Server implementation |

## Key methods (from spec)

- `getWalker()` — returns a `WalkerActor` for DOM traversal.
- `getPageStyle()` — returns a `PageStyleActor`.
- `getHighlighter()` — returns a highlighter actor for visual overlay.

## Status

Stub — backfilled in iter-73; expand on next touch.
