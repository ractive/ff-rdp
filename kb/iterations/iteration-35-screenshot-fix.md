---
title: "Iteration 35: Fix Screenshot Command (Firefox 149 Timeout)"
type: iteration
status: completed
date: 2026-04-08
tags:
  - iteration
  - bugfix
  - screenshot
  - firefox-149
  - protocol
  - research
branch: iter-35/screenshot-fix
---

# Iteration 35: Fix Screenshot Command (Firefox 149 Timeout)

The `screenshot` command times out on Firefox 149. All three screenshot
strategies fail.

## Errors

### Strategy 1: JavaScript `canvas.drawWindow()`
Returns `null` — `drawWindow()` is a privileged Firefox API that may be
restricted in headless mode or removed in Firefox 149.

### Strategy 2: Legacy `screenshotContentActor.capture()`
```
unrecognizedPacketType: captureScreenshot
```
Also tried `screenshot` and `capture` method names — all unrecognized.

### Strategy 3: Two-step protocol (added in iteration 33)
```
screenshotActor.capture failed (operation timed out)
```
Step 1 (`prepareCapture`) succeeds and returns `windowDpr`/`windowZoom`, but
step 2 (`screenshotActor.capture` on the root screenshot actor) times out.

## Current code flow

**File:** `crates/ff-rdp-cli/src/commands/screenshot.rs`

1. Try JS `canvas.drawWindow()` via `evaluateJSAsync` → returns null
2. Try `screenshotContentActor.capture()` (tries 3 method names) → unrecognized
3. Try two-step:
   a. `screenshotContentActor.prepareCapture(full_page)` → **succeeds** (returns DPR/zoom)
   b. `ScreenshotActor::get_actor_id()` from root → gets `screenshotActor` ID
   c. `ScreenshotActor::capture(screenshotActor, browsingContextID, full_page, prep)` → **TIMEOUT**

**File:** `crates/ff-rdp-core/src/actors/screenshot.rs`

The `capture` method sends:
```json
{
  "to": "server1.conn0.screenshotActor7",
  "type": "capture",
  "browsingContextID": 12,
  "fullpage": true,
  "dpr": 1.0,
  "snapshotScale": 1.0
}
```
Firefox never responds (timeout).

## Research Tasks

**IMPORTANT: Do thorough protocol research before attempting any code fix.**

- [x] **Raw protocol exploration.** Connect with `nc localhost 6000` or a Rust
  script. After handshake:
  1. `getRoot` → find `screenshotActor` ID
  2. Send `capture` to `screenshotActor` with varying parameters — try different
     field names, different values for `browsingContextID`
  3. Try sending `capture` directly to the `screenshotContentActor` (the
     per-tab actor) instead of the root actor
  4. Try the `prepareCapture` response more carefully — does it return any
     hints about what step 2 should look like?
  5. List ALL methods available on the screenshot actors — send an invalid
     method name and see if the error reveals available methods

- [x] **Explore alternative screenshot actors.** After `getRoot`, examine
  the full response for any new actors. After `getTarget`, examine the target
  info for new screenshot-related actors. Look for:
  - `browsingContext` related actors
  - Any actor with "image", "snapshot", "capture" in the name
  - WebDriver BiDi actors that might support `browsingContext.captureScreenshot`

- [x] **Test `browsingContext.captureScreenshot` (WebDriver BiDi).** Firefox 149
  may have moved screenshots to the BiDi protocol:
  - Check if there's a `browsingContext` actor available
  - Try sending BiDi-style commands

- [x] **Test JS alternatives.** Since `drawWindow` is restricted:
  - Try `html2canvas` approach (create a script that screenshots via DOM)
  - Try `OffscreenCanvas` with `drawWindow`
  - Try `window.navigator.mediaDevices.getDisplayMedia()` + canvas capture
  - Try `dom.canvas.drawWindow` pref — can we enable it in headless?

- [x] **Search Firefox source code** on searchfox.org for:
  - `screenshotActor` capture implementation
  - `drawSnapshot` (used by the root screenshot actor internally)
  - Recent changes to screenshot code in Firefox 149
  - What `browsingContext.captureScreenshot` looks like in BiDi

- [x] **Document findings** in `kb/research/screenshot-protocol-ff149.md`

## Implementation

- [x] Implement the working screenshot method discovered during research
- [x] Keep the fallback chain (try multiple approaches)
- [x] Update test fixtures (no fixture changes needed — existing fixtures already had `rect: null`)
- [x] Verify screenshot works in both daemon and no-daemon mode

## Root Cause

`prepareCapture` may return a non-null `rect` field that must be forwarded to
`screenshotActor.capture` for fullpage/element captures. The code was
ignoring that returned `rect`, causing Firefox to time out because
`drawSnapshot` didn't know the capture region. Viewport captures can
legitimately return `rect: null`. Fix: added `CaptureRect` to the
`PrepareCapture` struct and forwarded the non-null `rect` in the capture args.

## Test Fixtures

All e2e test fixtures must be recorded from a real Firefox instance — never hand-craft them.
Run with `FF_RDP_LIVE_TESTS_RECORD=1 cargo test -p ff-rdp-core --test live_record_fixtures -- --ignored` to record fixtures.
