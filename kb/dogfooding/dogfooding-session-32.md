---
title: Dogfooding Session 32
date: 2026-04-08
type: dogfooding
status: completed
firefox_version: 149
binary: ./target/release/ff-rdp
port: 6000
mode: daemon
site: comparis.ch
---

# Dogfooding Session 32

Focused re-test of commands that failed in previous sessions.

## Previously Failed — Now Fixed

| # | Command | Previous Issue | Current Result |
|---|---------|---------------|----------------|
| 3 | `responsive "body" --widths 320,768,1024` | Viewport wasn't resizing | FIXED. Widths 320/768/1024 all differ. Body rect.width matches viewport width. Heights differ (5225 at 320, 5097 at 768/1024). |
| 7 | `network --follow` (daemon) | Unknown | FIXED. Captured 390 lines of request/response events after navigating to /krankenkassen. Both request and response events with full details. |
| 8 | `a11y --depth 3` | JS fallback with debug noise | FIXED (partially). Valid JSON on stdout (verified with json.load). Debug line on stderr: `accessibility walker root methods unrecognized ... falling back to JS eval`. Stderr separation is correct — no noise in stdout. |
| 9 | `sources` | JS fallback with debug noise | FIXED (partially). Valid JSON on stdout (294 lines, verified). Debug on stderr: `sources thread actor failed ... falling back to JS DOM/Performance API`. Clean separation — no debug in stdout. |

## Previously Failed — Still Broken

| # | Command | Previous Issue | Current Result | Notes |
|---|---------|---------------|----------------|-------|
| 1 | `cookies` | Returning 0 results | ERROR: `actor error from server1.conn3.cookies5: TypeError — can't access property "toLowerCase", sessionString is undefined` | JS eval shows 30 cookies exist via `document.cookie.split(";").length`. The cookies actor is crashing on a cookie with an undefined session string. |
| 2 | `screenshot --output /tmp/dogfood-screenshot.png` | Broken on Firefox 149 | ERROR: `screenshotActor.capture failed (operation timed out)` | Also fails with `--timeout 15000`. Error message suggests non-headless mode but Firefox IS headless. The screenshotActor itself is timing out. |
| 4 | `console --follow` | Producing no output | STILL BROKEN. Started follow in background, generated 3 console messages via eval, waited 3 seconds, killed. Output file was empty. |
| 5 | `network --limit 20` (daemon, no navigate) | Returning 0 results | STILL BROKEN. Hint: `performance-api fallback eval failed: operation timed out`. Returns 0 results. Note: `network --follow` works fine (test 7). |
| 6 | `navigate --with-network --network-timeout 5` | Capturing only 1 request | STILL BROKEN. Captured only 1 request (a beacon to troubadix.data.comparis.ch) when navigating to /hypotheken. Expected dozens of requests. |

## Smoke Test Results

| Command | Status | Notes |
|---------|--------|-------|
| `perf vitals` | PASS | tbt_ms=0.0 (not -0.0). lcp_ms=0.0 with note about headless approximation. fcp_ms=138, ttfb_ms=104. |
| `storage localStorage` | PASS | Returns cookies/consent data correctly. |
| `storage sessionStorage` | PASS | Returns 6 items. |
| `geometry "header" --visible-only` | PASS | Returns header element, in_viewport=true, rect correct. |
| `type "input" "test"` | PASS | typed=true, value="test". |
| `click "a[href]"` | PASS | clicked=true. |
| `perf vitals --fields fcp_ms,ttfb_ms` | PASS | Correctly filters to only fcp_ms and ttfb_ms. |

## New Issues Found

1. **`cookies` actor crash**: New error type — `TypeError: can't access property "toLowerCase", sessionString is undefined`. This was previously "0 results" but now it's an actor-level crash. The cookies actor in Firefox 149 may have a protocol change around session string handling.

2. **`a11y` and `sources` JS fallback**: Both commands work but fall back to JS evaluation because Firefox 149's accessibility walker and sources thread actors have changed their API. The fallback is clean (debug on stderr only), but native actor support is degraded.

3. **`lcp_ms` always 0.0**: `perf vitals` reports lcp_ms=0.0 with a note that LCP isn't available from PerformanceObserver in headless Firefox. This is a known limitation but worth noting.

## Summary

- **4 commands fixed** (or working with acceptable fallback): responsive, network --follow, a11y, sources
- **5 commands still broken**: cookies (actor crash), screenshot (timeout), console --follow (empty), network --limit (timeout), navigate --with-network (only 1 request)
- **7 smoke tests pass**: No regressions in previously working commands
- **Key pattern**: Commands that use Firefox actor protocol directly (cookies actor, screenshot actor, console listener, Performance API eval) are failing on Firefox 149. Commands that use JS eval fallback (a11y, sources) or the network watcher actor (network --follow) work correctly.
