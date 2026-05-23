---
type: rdp-note
tags:
  - rdp
  - firefox-server
  - resource
  - storage
date: 2026-05-23
firefox_files:
  - devtools/server/actors/resources/storage-cookie.js
  - devtools/server/actors/resources/storage-local-storage.js
  - devtools/server/actors/resources/storage-session-storage.js
  - devtools/server/actors/resources/storage-indexed-db.js
  - devtools/server/actors/resources/storage-cache.js
  - devtools/server/actors/resources/storage-extension.js
  - devtools/server/actors/resources/storage/
title: "Resource: storage (cookies/local/session/idb/cache)"
---

# Storage Resources

A family of frame-target resources, one per storage backend. All follow the same shape and surface a per-backend StorageActor (cookies/localStorage/etc) that the client can call methods on.

| Resource type | Storage actor module |
|---|---|
| `cookies` | resources/storage-cookie.js |
| `local-storage` | resources/storage-local-storage.js |
| `session-storage` | resources/storage-session-storage.js |
| `indexed-db` | resources/storage-indexed-db.js |
| `Cache` (note capitalized) | resources/storage-cache.js |
| `extension-storage` | resources/storage-extension.js |

## Generic payload

```
{
  resourceType: <one of above>,
  actor: <storage actor id>,
  hosts: { <origin>: [keys] },
  ...
}
```

The actor exposes:

- `getStoreObjects(host, names, options)` — paginated retrieval.
- `addItem(host, item)`, `editItem(...)`, `removeItem(host, name)`, `removeAll(host)`, `removeAllSessionCookies(host)`.

## Gotchas

- `Cache` is the only resource type that starts with a capital — it's the Cache API (used by service workers), not HTTP cache.
- Storage **lives in the parent process** for cookies and IndexedDB, in the content process for local/session storage — the watchers route accordingly.
- Cross-origin isolated documents have separate buckets — each origin appears as its own `host` key.
- Adding/editing IndexedDB entries from devtools requires opening the DB transaction; not all schemas are editable.
