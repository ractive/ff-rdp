---
title: "Iteration 34: Fix Cookies Command (StorageActor Crash)"
type: iteration
status: completed
date: 2026-04-08
tags:
  - iteration
  - bugfix
  - cookies
  - firefox-149
  - protocol
  - research
branch: iter-34/cookies-fix
---

# Iteration 34: Fix Cookies Command (StorageActor Crash)

The `cookies` command crashes with a Firefox 149 actor error. This was a
regression introduced in iteration 33's cookies fix (which tried to work around
a daemon event interception issue).

## Error

```
actor error from server1.conn3.cookies5: TypeError — can't access property
"toLowerCase", sessionString is undefined
```

The StorageActor inside Firefox is crashing because `sessionString` is
`undefined` when it tries to call `.toLowerCase()` on it. This happens during
`getStoreObjects` on the cookie store actor.

## Context

### Current code flow (from `storage.rs`)
1. `TabActor::get_watcher()` → get watcher actor
2. `WatcherActor::watch_resources(["cookies"])` → subscribe to cookie store
3. Parse `resources-available-array` → extract cookie actor, `resourceId`, hosts
4. For each host: `getStoreObjects({host, resourceId})` → **CRASHES HERE**
5. Parse cookie data

### Iteration 33 changes
The previous fix addressed three issues:
1. `parse_cookie_store_resource` now extracts `resourceId` from the response
2. `list_cookies` passes `{host, resourceId}` to `getStoreObjects`
3. In daemon mode, opens a **direct TCP connection** to Firefox (bypassing
   daemon proxy) to avoid the daemon's watcher intercepting cookie events

### Likely root cause
The `sessionString` error suggests Firefox 149's `getStoreObjects` expects a
different parameter format. The `sessionString` is likely a parameter that
Firefox tries to extract from the request, and when it's missing, the JS
inside Firefox crashes. This could be:
- A new required parameter in Firefox 149 (e.g., `sessionString` or `sessionContext`)
- A change in how the cookie store expects to be queried
- The `resourceId` value being wrong or malformed

## Research Tasks

**IMPORTANT: Do thorough protocol research before attempting any code fix.**

- [x] **Raw protocol exploration with netcat/telnet.** Connect to Firefox RDP
  on port 6000 with `nc localhost 6000` or a small Rust script. Manually send
  the RDP handshake, then walk through the cookie protocol step by step:
  1. Send `listTabs` → get tab actor
  2. Send `getWatcher` on tab → get watcher actor
  3. Send `watchResources(["cookies"])` → capture the full response JSON
  4. Examine every field in the cookie store resource (actor, resourceId, hosts,
     and any other fields like `sessionContext`, `sessionType`, etc.)
  5. Send `getStoreObjects` with different parameter combinations to find what
     Firefox 149 actually expects
  6. Document the exact request/response for each step

- [x] **Compare with working `storage local` command.** The `storage local`
  (localStorage) command works fine. Compare its protocol flow with cookies:
  - Does localStorage also use `watchResources`?
  - Does it pass the same parameters to `getStoreObjects`?
  - What parameters does localStorage pass that cookies doesn't?

- [x] **Search Firefox source code.** Look at searchfox.org for:
  - The `getStoreObjects` implementation in the cookie actor
  - What `sessionString` is and where it comes from
  - Recent changes to the cookie storage actor in Firefox 149
  - The `StorageActorMixin` base class that both cookie and localStorage inherit

- [x] **Document findings** in `kb/research/cookies-protocol-ff149.md`

## Implementation

- [x] Fix the `getStoreObjects` call with the correct parameters discovered
  during research
- [x] Verify cookies work in both daemon and no-daemon mode
- [x] Verify the direct TCP connection workaround (from iter 33) is still
  needed, or if the fix makes it unnecessary
- [x] Update test fixtures if the protocol format changed

## Test Fixtures

All e2e test fixtures must be recorded from a real Firefox instance — never hand-craft them.
Run with `FF_RDP_LIVE_TESTS_RECORD=1 cargo test -p ff-rdp-core --test live_record_fixtures -- --ignored` to record fixtures.
