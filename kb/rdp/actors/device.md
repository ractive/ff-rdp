---
type: rdp-note
tags:
- rdp
- firefox-server
- actor
- device
date: 2026-05-24
firefox_files:
- devtools/shared/specs/device.js
- devtools/server/actors/device.js
title: DeviceActor
---

# DeviceActor

Provides device and platform metadata about the connected Firefox instance (OS, screen
resolution, hardware concurrency, etc.). Used for diagnostics and responsive-mode setup.

## Firefox references

| File | Lines | Purpose |
|------|-------|---------|
| `devtools/shared/specs/device.js` | 1-18 | Protocol spec — method signatures and return types |
| `devtools/server/actors/device.js` | 1-75 | Server implementation |

## Key methods (from spec)

- `getDescription()` — returns a `DeviceDescription` dict with OS, CPU, memory, screen info.

## Status

Stub — backfilled in iter-73; expand on next touch.
