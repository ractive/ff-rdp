---
title: "Iteration 31: Dogfooding Fixes"
type: iteration
status: completed
date: 2026-04-08
tags:
  - iteration
  - bugfix
  - dogfooding
  - ux
  - compatibility
branch: iter-31/dogfooding-fixes
---

# Iteration 31: Dogfooding Fixes

Fixes and improvements from the comparis.ch dogfooding session (session 29).
See [[dogfooding/dogfooding-session-29]] for full findings.

## Part A: Firefox 149 Compatibility (HIGH)

Three RDP actors are broken in Firefox 149.

- [x] Investigate `accessiblewalker` — `getRootNode` unrecognized in Firefox 149.
  May need to use a different method name or discover the new protocol.
  Launch headless Firefox and explore the actor's available methods.
- [x] Investigate `screenshotContentActor` — `captureScreenshot` unrecognized.
  Firefox 149 may have moved screenshot to a different actor. Research.
- [x] Investigate `sources` — returns `undefined passed where a value is required`.
  Protocol change in thread/source listing.
- [x] Add Firefox version detection: read version from root actor greeting and
  warn when running against untested versions.

## Part B: UX Friction Fixes (MEDIUM)

- [x] Accept `localStorage`/`sessionStorage` as aliases for `local`/`session`
  in the `storage` command
- [x] `network` without `--follow`: show Performance API resource timing entries
  as fallback when watcher has no buffered events (like `perf audit` already does).
  Print a hint if no data at all.
- [x] `navigate --with-network`: add `--network-timeout` flag (default 5s instead
  of current ~17s idle detection). The current timeout is too conservative for
  real-world sites with continuous beacon traffic.
- [x] Normalize `tbt_ms: -0.0` to `0.0` in perf vitals output
- [x] `--format text` for `perf audit` and `snapshot` — currently outputs JSON
  even when `--format text` is requested. Implement text rendering.

## Part C: LCP Null Fix (HIGH)

`lcp_ms` is always null. This is a significant gap for web performance analysis.

- [x] Investigate why LCP is never populated in headless Firefox — likely
  PerformanceObserver doesn't fire for LCP in headless mode.
- [x] Try fallback: `performance.getEntriesByType('largest-contentful-paint')`
- [x] If Firefox headless truly doesn't support LCP, document the limitation
  and consider a JS-based approximation

## Part D: Minor Improvements

- [x] `geometry` command: add `--visible-only` flag to filter out zero-size
  and `display:none` elements (consent overlays flood results)
- [x] `click` response: flatten to `{clicked: true, tag, text}` instead of
  raw RDP grip objects
- [x] Daemon: handle abrupt client disconnect from `--follow` commands more
  gracefully (don't leave port unavailable)
- [x] `console --follow`: investigate why no output was captured even after
  generating console messages via eval. Likely timing or subscription issue.
- [x] `ff-rdp launch`: auto-set `app.update.enabled=false` in the profile's
  `user.js` (note: `devtools.debugger.remote-enabled` is already handled by
  `ensure_devtools_prefs` and `USER_JS` since iter 17)

## Test Fixtures

All e2e test fixtures must be recorded from a real Firefox instance — never hand-craft them.
Run with `FF_RDP_LIVE_TESTS_RECORD=1 cargo test -p ff-rdp-core --test live_record_fixtures -- --ignored` to record fixtures.
