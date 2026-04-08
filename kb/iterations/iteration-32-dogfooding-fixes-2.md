---
title: "Iteration 32: Dogfooding Fixes Round 2"
type: iteration
status: completed
date: 2026-04-08
tags:
  - iteration
  - bugfix
  - dogfooding
  - ux
  - compatibility
  - daemon
branch: iter-32/dogfooding-fixes-2
---

# Iteration 32: Dogfooding Fixes Round 2

Fixes from dogfooding session 30 (daemon + no-daemon). See [[dogfooding/dogfooding-session-30]].

## Part A: Daemon Mode Regressions (HIGH)

- [x] `network --limit N` returns 0 results in daemon mode — Performance API
  fallback does not trigger. The fallback should fire whenever the WatcherActor
  buffer is empty, regardless of daemon/no-daemon mode.
- [x] `navigate --with-network --network-timeout 5` captures far fewer requests
  in daemon mode (1 vs many). Investigate whether WatcherActor events are
  buffered lazily in daemon mode and ensure the short-timeout path collects
  them properly.

## Part B: Broken Commands (HIGH)

- [x] `responsive` command does not resize the viewport — all requested widths
  report viewport.width=1366 with identical element rects. Investigate the
  viewport resize mechanism (likely needs `emulationActor.setDPPX` or
  `responsiveActor` API). Test with widths 320, 768, 1024.
- [x] `console --follow` produces no output even when console.log is triggered
  via eval. Investigate the subscription mechanism — may be a timing issue
  (subscribe before eval), a Firefox 149 protocol change, or a missing
  `startListeners` call on the console actor.

## Part C: Firefox 149 Compatibility (MEDIUM)

These have been broken since session 29. Investigate protocol changes and
implement fallbacks where possible.

- [x] `screenshot` — `captureScreenshot` unrecognized on `screenshotContentActor`.
  Research alternatives: `browsingContext.captureScreenshot` (WebDriver BiDi),
  canvas-based screenshot via eval, or a different actor in Firefox 149.
- [x] `a11y --depth 3` — `getRootNode` unrecognized on `accessiblewalker`.
  Research the new accessibility protocol in Firefox 149 (method may have been
  renamed or moved to a different actor).
- [x] `sources` — returns `undefined passed where a value is required`.
  Research `threadActor.sources()` or alternative approaches for listing loaded
  scripts in Firefox 149.

## Part D: UX Polish (LOW)

- [x] Flatten `type` command response — like `click` returns
  `{"typed": true, "tag": "INPUT", "value": "..."}` instead of raw actor/grip
  objects.
- [x] `lcp_ms` inconsistency — `perf vitals` returns `0.0` with
  `lcp_approximate: true` but `perf compare` returns `null`. Make both
  consistent (use the same approximation logic in compare).
- [x] `--fields` flag — verify it works for `perf vitals`. If not, fix or
  remove the flag.
- [x] Transient `listTabs` error — observed `invalid packet: listTabs response
  missing 'tabs' field` when running eval immediately after navigate in daemon
  mode. Add retry or guard against race condition.

## Test Fixtures

All e2e test fixtures must be recorded from a real Firefox instance — never hand-craft them.
Run with `FF_RDP_LIVE_TESTS_RECORD=1 cargo test -p ff-rdp-core --test live_record_fixtures -- --ignored` to record fixtures.
