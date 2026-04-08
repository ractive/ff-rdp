---
title: "Firefox 149 Cookie Protocol: sessionString Requirement"
type: research
date: 2026-04-08
status: completed
tags: [firefox, rdp-protocol, cookies, storage-actor]
related: "[[iteration-34-cookies-fix]]"
---

# Firefox 149 Cookie Protocol: sessionString Requirement

## Problem

Firefox 149's `getStoreObjects` on the cookie store actor crashes with:
```
TypeError — can't access property "toLowerCase", sessionString is undefined
```

This occurs because Firefox's StorageActor implementation calls `.toLowerCase()` on the `sessionString` parameter, which is `undefined` when not provided.

## Root Cause

The `getStoreObjects` method in Firefox's cookie store actor expects an `options` object with at least a `sessionString` field. The accepted/expected values are:

- `"Session"` — standard DevTools label for the session-cookie grouping
- `"Persistent"` — standard DevTools label for the persistent-cookie grouping

In practice, Firefox uses `sessionString` as a grouping/display hint in DevTools rather than as a response filter. When omitted, Firefox still tries to process it by calling `.toLowerCase()`, which crashes on `undefined`.

## Correct Protocol Sequence

### 1. Get Watcher
```json
→ {"to": "<tab_actor>", "type": "getWatcher"}
← {"from": "<tab_actor>", "actor": "<watcher_actor>"}
```

### 2. Watch Resources
```json
→ {"to": "<watcher>", "type": "watchResources", "resourceTypes": ["cookies"]}
← {
    "type": "resources-available-array",
    "from": "<watcher>",
    "array": [["cookies", [{
      "actor": "<cookie_actor>",
      "hosts": {"https://example.com": []},
      "resourceId": "cookies-12884901889"
    }]]]
  }
```

### 3. Get Store Objects (FIXED)
```json
→ {
    "to": "<cookie_actor>",
    "type": "getStoreObjects",
    "host": "https://example.com",
    "resourceId": "cookies-12884901889",
    "options": {"sessionString": "Session"}
  }
← {
    "from": "<cookie_actor>",
    "data": [...cookies...],
    "offset": 0,
    "total": N
  }
```

### 4. Unwatch
```json
→ {"to": "<watcher>", "type": "unwatchResources", "resourceTypes": ["cookies"]}
```

## Key Parameters

| Parameter | Required | Purpose |
|-----------|----------|---------|
| `host` | Yes | Host origin filter (from watchResources hosts map) |
| `resourceId` | Yes (FF149+) | Resource ID from watchResources response |
| `options.sessionString` | Yes (FF149+) | Session type string; crashes if missing |
| `options.sortOn` | No | Optional server-side sort field |

## Comparison with localStorage

localStorage does NOT use the StorageActor protocol. It uses JavaScript evaluation via WebConsoleActor (`evaluateJSAsync`), which avoids the sessionString issue entirely.

## Fix Applied

Added `"options": {"sessionString": "Session"}` to the `getStoreObjects` call in `StorageActor::list_cookies()`. The `"Session"` value is used because it's the standard grouping label in Firefox DevTools and doesn't actually filter — all cookies (session + persistent) are returned regardless.
