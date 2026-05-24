---
type: rdp-note
tags:
- rdp
- firefox-server
- actor
- storage
date: 2026-05-24
firefox_files:
- devtools/shared/specs/storage.js
- devtools/server/actors/resources/storage/index.js
title: StorageActor
---

# StorageActor

Provides access to all browser storage types (cookies, localStorage,
sessionStorage, indexedDB, cache storage) for a given target. Supports
listing, reading, updating, and deleting storage entries.

## Firefox references

| File | Lines | Purpose |
|------|-------|---------|
| `devtools/shared/specs/storage.js` | 1-320 | Protocol spec — store types, CRUD methods |
| `devtools/server/actors/resources/storage/index.js` | 1-404 | Base storage resource implementation |

## Key methods (from spec)

- `getStores()` — returns available storage types for the target.
- `getStoreData(host, names)` — retrieve entries from a store.
- `removeItem(host, name)` — delete a storage entry.
- `editItem(data)` — update an existing entry.

## Status

Stub — backfilled in iter-73; expand on next touch.
