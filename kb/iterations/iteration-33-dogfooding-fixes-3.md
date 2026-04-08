---
title: "Iteration 33: Dogfooding Fixes Round 3"
type: iteration
status: in-progress
date: 2026-04-08
tags:
  - iteration
  - bugfix
  - dogfooding
  - ux
  - compatibility
  - daemon
  - research
branch: iter-33/dogfooding-fixes-3
---

# Iteration 33: Dogfooding Fixes Round 3

Fixes from dogfooding session 31 (daemon mode, post iteration 32).
See [[dogfooding/dogfooding-session-31]].

**IMPORTANT: Do thorough research before implementing each fix.** Launch headless
Firefox, explore the RDP protocol, inspect available actors/methods, and read
Firefox source code (searchfox.org) to understand the correct APIs before writing
code. Don't guess — discover the actual protocol.

## Part A: Cookies Regression (HIGH — was working, now broken)

- [x] **Research:** Launch headless Firefox on port 6000, navigate to a site with
  cookies, and manually explore the StorageActor protocol. Send raw RDP messages
  to understand what methods/params the cookie actor expects in Firefox 149.
  Check if the actor name changed, if `getCookies` was renamed, or if the
  request format changed. Compare with the working `storage local` command
  to understand what's different.
- [x] Fix the `cookies` command to return cookies again.
- [x] Verify it returns the same cookies as `document.cookie` via eval.

**Root causes found and fixed:**
1. `storage.rs`: FF149 changed `watchResources("cookies")` response — `hosts` is now an
   object map (not array), and a `resourceId` field is required by `getStoreObjects`.
   Fixed `parse_cookie_store_resource` to extract `resourceId` and updated `list_cookies`
   to pass `{"host": host, "resourceId": resource_id}`.
2. `server.rs`: `is_watcher_event` was intercepting ALL `resources-available-array` events
   by type. Fixed to also check `from == daemon_watcher_actor` so only daemon-owned events
   are buffered.
3. `cookies.rs`: In daemon mode, `getWatcher` on the same connection always returns the
   daemon's watcher actor, so daemon still intercepts the cookies response. Fixed by opening
   a direct TCP connection to Firefox (bypassing the proxy) only for the cookies lookup.

## Part B: Debug Messages Polluting stdout (HIGH — breaks JSON piping)

- [ ] `a11y --depth 3` prints `debug: accessibility walker root methods
  unrecognized...` to stdout before JSON. Route to stderr.
- [ ] `sources` prints `debug: sources thread actor failed...falling back to JS`
  to stdout before JSON. Route to stderr.
- [ ] Audit all fallback code paths for any other debug/info messages that go
  to stdout. All non-JSON output must go to stderr.

## Part C: Network in Daemon Mode (MEDIUM)

- [ ] **Research:** Understand why the Performance API fallback for `network
  --limit N` doesn't trigger in daemon mode. Read the code path for both
  daemon and no-daemon modes. The fallback should fire whenever the
  WatcherActor buffer is empty.
- [ ] Enable Performance API fallback in daemon mode for `network --limit N`.
- [ ] **Research:** Investigate why `navigate --with-network --network-timeout 5`
  captures only 1 request in daemon mode. The WatcherActor subscription may
  need to be established earlier or events may be arriving after the timeout.
  Add logging to understand the timing.
- [ ] Fix `navigate --with-network --network-timeout 5` to capture a reasonable
  number of requests in daemon mode.
- [ ] Fix `total_transfer_bytes: -0.0` — normalize to `0.0`.

## Part D: Firefox 149 Protocol Research & Fixes (MEDIUM)

For each broken feature, do thorough protocol research first.

### Screenshot
- [ ] **Research:** Connect to Firefox 149 on the RDP port and enumerate all
  available actors on the tab target. Look for any actor that mentions
  "screenshot", "capture", or "image". Check searchfox.org for
  `captureScreenshot` to find what replaced it. Try the WebDriver BiDi
  `browsingContext.captureScreenshot` command. Try `canvas.toDataURL()` via
  eval as a JS-only fallback.
- [ ] Implement working screenshot for Firefox 149 using the discovered method.

### Responsive / Viewport Resize
- [ ] **Research:** Connect to Firefox 149 and enumerate the ResponsiveActor's
  available methods. Check searchfox.org for `setViewportSize` to find what
  replaced it. Try `Emulation.setDeviceMetricsOverride` (CDP-style) or
  `browsingContext.setViewport` (BiDi). As a JS fallback, try
  `window.resizeTo()` combined with reading `window.innerWidth` to verify.
- [ ] Implement working viewport resize for Firefox 149.

### Console --follow
- [ ] **Research:** Connect to Firefox 149 and explore the console actor
  protocol. Check if `startListeners` is still the correct method. Try
  sending `startListeners` with different listener types. Check if the event
  name changed (e.g., `consoleAPICall` vs `pageError` vs something new).
  Use the raw RDP connection to send `evaluateJSAsync('console.log("test")')`
  and observe what events come back on the console actor.
- [ ] Fix `console --follow` to capture console output.

## Test Fixtures

All e2e test fixtures must be recorded from a real Firefox instance — never hand-craft them.
Run with `FF_RDP_LIVE_TESTS_RECORD=1 cargo test -p ff-rdp-core --test live_record_fixtures -- --ignored` to record fixtures.
