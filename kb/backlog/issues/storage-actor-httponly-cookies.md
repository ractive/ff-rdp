---
title: "Native StorageActor for httpOnly cookies"
type: feature
status: open
priority: high
discovered: 2026-04-07
tags: [storage, cookies, protocol, correctness]
---

# Native StorageActor for httpOnly cookies

The `cookies` command uses `document.cookie` via JS eval, which cannot read httpOnly
cookies. Firefox's native StorageActor can access all cookies including httpOnly,
LocalStorage, SessionStorage, IndexedDB, and Cache API.

This is a correctness gap — users running `ff-rdp cookies` may think a site has
no cookies when it actually has httpOnly session cookies.

## Protocol

```json
{"to": "<storageActor>", "type": "listStores"}
{"to": "<cookieStoreActor>", "type": "getStoreObjects", "host": "example.com"}
```

## Scope

- Phase 1: StorageActor for cookies (httpOnly visibility)
- Phase 2: LocalStorage/SessionStorage via StorageActor
- Phase 3: IndexedDB, Cache API
