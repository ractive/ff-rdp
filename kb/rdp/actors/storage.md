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
- devtools/server/actors/resources/storage/cookies.js
- devtools/server/actors/resources/utils/parent-process-storage.js
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

## FF152: cookie enumeration went silent (iter-121)

Discovered in [[dogfooding-session-61]] and CONFIRMED on a clean single Firefox
152.0.6 instance: `StorageActor::list_cookies` returned an **empty** vector, so
the `cookies` command silently fell back to `document.cookie` — missing every
httpOnly cookie and nulling all `secure`/`sameSite`/`domain` flags.
`cookies --storage-only` returned 0.

**Root cause — message-ordering, not a wire-contract change.** Raw RDP tracing
(`FF_RDP_TRACE_RAW=1 … --log-level trace cookies --storage-only`) showed
`getStoreObjects` was **never sent**. The sequence:

1. `watchResources(["cookies"])` is sent.
2. On FF152 the `resources-available-array` cookie event arrives **before** the
   `watchResources` ACK. `actor_request`/`recv_reply_from` (iter-74+) classifies
   any `from`+`type` packet as an event and routes it to the transport's event
   sink, returning only the plain ACK `{"from":"watcher…"}`.
3. On the direct command path **no event sink is installed**, so
   `forward_event` **dropped the event entirely**.
4. `list_cookies` then tried `transport.recv()` to recover the event — but it
   was already consumed and dropped, so `recv()` blocked to the 10 s socket
   timeout, `cookie_resource` stayed `None`, `getStoreObjects` was never sent,
   and the result was empty (exit 0).

In FF149 the ACK came first, then the event, so the old `recv()` fallback caught
it — which is why the bug was version-dependent.

The FF152 cookie resource event shape is unchanged and already parsed:
`array:[["cookies",[{actor, hosts:{"https://host":[]}, resourceId:"cookies-…",
resourceKey:"cookies", browsingContextID, traits}]]]`. The `getStoreObjects`
spec is also unchanged (`host: Arg(0)`, `names: Arg(1,"nullable:array:string")`,
`options: Arg(2,"nullable:json")`). The FF152 architectural note: cookies now run
in the **parent process** (`resources/storage-cookie.js` → `ParentProcessStorage`
→ `resources/storage/cookies.js`), and hosts derive from
`watcherActor.getAllBrowsingContexts()` (`getHostName` returns `uri.prePath`,
e.g. `https://example.com`).

**Fix (Theme A)**: `list_cookies` now installs a temporary event-sink collector
around the `watchResources` call via `RdpTransport::swap_event_sink` (restoring
any prior sink afterwards), then finds the cookie resource from the captured
events first, falling back to the inline ACK and one extra `recv()` for older
Firefox orderings. `getStoreObjects` fires correctly and cookies return with real
`isHttpOnly`/`isSecure`/`sameSite`.

**Fix (Theme B)**: `commands::cookies::run` attaches a
`warnings[{type:"storage_actor_empty", …}]` marker when the StorageActor
enumeration is empty but `document.cookie` still contributed entries, so a
degraded result is never presented as complete
(`attach_storage_degraded_warning`).

Recorded fixture: `get_store_objects_cookies_httponly_response.json` (a real
FF152 reply with an httpOnly `secret` cookie). Live ACs:
`live_cookies_httponly_enumerated` + `live_cookies_storage_only_nonempty`
(`live/live_cookies.rs`); recorder `live_cookies_httponly`
(`live_record_fixtures.rs`).
