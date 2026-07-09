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

Active — see iter-73 (initial), iter-84 (retry delay), iter-85 (network cookie merge).

## iter-85: Set-Cookie header merge (Theme L)

**Problem**: cookies set via `Set-Cookie` response header (e.g.
`httpbin.org/cookies/set`) may not appear via `StorageActor` on FF 151 because
Firefox has not yet flushed them to the cookie store by the time `cookies list`
runs.

**Architecture note**: ff-rdp has no cross-command persistent state (each
command calls `connect_direct`), so buffering network response headers across
`navigate` + `cookies` invocations is not directly possible.

**What was implemented (iter-85)**:

1. `parse_set_cookie_header(header: &str) -> Option<NetworkSetCookie>` — parses
   a single `Set-Cookie` header value into a typed `NetworkSetCookie`.  Handles
   `Domain=`, `Path=`, `Max-Age=`, `Secure`, and `HttpOnly` attributes.
   `Expires=` date strings are not parsed (treated as session cookies).

2. `merge_storage_and_network_cookies(storage, network) -> Vec<CookieInfo>` —
   merges `StorageActor` cookies with `NetworkSetCookie` entries.  StorageActor
   wins on name conflict.  Network-only cookies are appended.

3. Unit test `unit_cookies_setcookie_merge` verifies the merge semantics:
   - `foo=storage_value` (storage) beats `foo=network_value` (network).
   - `bar=network_only` (network only) appears in the merged output.

**Limitation**: The `cookies` CLI command does not yet call
`merge_storage_and_network_cookies` — the architecture would require the
command to subscribe to network events AND parse response headers during the
same session.  The retry delay (iter-84) is still the active mitigation.
The merge function is wired in `lib.rs` (`parse_set_cookie_header`,
`merge_storage_and_network_cookies`, `NetworkSetCookie`) for future wiring when
a suitable network-events subscription path is added.

Live test: `live_cookies_set_cookie_header.rs` — `#[ignore]` gated;
navigates to `httpbin.org/cookies/set?probe=1` and asserts `probe=1` appears
in `cookies list` output.

## longString cookie values (iter-102)

The cookie `value` slot is declared `longstring` in
`devtools/shared/specs/storage.js`: a cookie value above Firefox's
`DebuggerServer.LONG_STRING_LENGTH` threshold (~10 KB) arrives as a
`{type:"longString", actor, length, initial}` grip, not an inline string.
`parse_cookie` (`actors/storage.rs`) resolves the slot through
`specs::types::resolve_long_string_slot`, so large cookie values are returned in
full rather than dropped to empty. Unit test:
`parse_cookie_resolves_longstring_value`. Live AC:
`live_cookie_longstring_value` (`live_102_longstring_and_reload.rs`).
localStorage/sessionStorage value slots are also `longstring` but ff-rdp does
not yet consume them — wire them through the same helper when it does. See
[[lessons-learned#longstring-grips-everywhere]].
