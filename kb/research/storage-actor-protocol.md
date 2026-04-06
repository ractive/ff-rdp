---
title: Firefox StorageActor Protocol Discovery
date: 2026-04-06
tags: [protocol, storage, cookies, firefox-rdp]
---

# Firefox StorageActor Protocol Discovery

## Key Finding: StorageActor is NOT a Direct Actor

The StorageActor is **not** available as a direct actor on the `getTarget` frame response. Instead, access is mediated through the WatcherActor.

## Access Pattern

1. Call `watchResources("cookies")` on the WatcherActor
2. Firefox responds with a `resources-available-array` message containing:
   - Actor ID for the cookie storage actor
   - `browsingContextID`
   - `hosts` map (mapping hostnames to empty arrays initially)
   - `traits` object

## Cookie Actor Methods

The cookie storage actor supports the following methods:

| Method | Description |
|--------|-------------|
| `getStoreObjects` | Retrieve cookies for a given host |
| `getFields` | Get schema metadata for all cookie fields (including editability) |
| `addItem` | Add a new cookie |
| `removeItem` | Remove an existing cookie |

## `getStoreObjects` Details

- **Requires** `host` parameter (e.g., `"https://example.com"`)
- **Requires** `options.sessionString` — any string like `"Session"` to avoid a Firefox sort bug in `natural-sort.js`
- The `names` parameter expects **uniqueKey values**, not cookie names

## Cookie Wire Format

Fields returned for each cookie:

| Field | Description |
|-------|-------------|
| `name` | Cookie name |
| `value` | Cookie value |
| `host` | Cookie domain/host |
| `path` | Cookie path |
| `expires` | Epoch milliseconds, `0` = session cookie |
| `size` | Cookie size in bytes |
| `isHttpOnly` | HTTP-only flag |
| `isSecure` | Secure flag |
| `sameSite` | SameSite attribute |
| `hostOnly` | Host-only flag |
| `lastAccessed` | Last access timestamp |
| `creationTime` | Creation timestamp |
| `updateTime` | Update timestamp |
| `uniqueKey` | Unique identifier (see format below) |
| `partitionKey` | Partition key for cookie partitioning |

## UniqueKey Format

```
name{9d414cc5-8319-0a04-0586-c0a6ae01670a}host{GUID}path{GUID}partitionKey
```

Each component is separated by a GUID-tagged segment.

## `getFields` Response

Returns schema metadata for all cookie fields, including whether each field is editable.

## Working Resource Types

| Resource Type | Status |
|---------------|--------|
| `"cookies"` | Works — returns cookie storage actor |
| `"indexed-db"` | Works — returns IndexedDB storage actor |
| `"cookie"` (singular) | Does NOT work — returns empty/ignored |
| `"storage"` | Does NOT work |
| `"local-storage"` | Does NOT work |
| `"session-storage"` | Does NOT work |
| `"cache-storage"` | Does NOT work |

## See Also

- [[iterations/iteration-11-native-cookie-access]]
- [[research/rdp-protocol-deep-dive]]
