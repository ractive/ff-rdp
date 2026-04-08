---
title: "Screenshot Protocol Changes in Firefox 149"
type: research
tags: [firefox, rdp, screenshot, protocol]
date: 2026-04-08
---

# Screenshot Protocol Changes in Firefox 149

## Key Findings

1. **Legacy method removed.** Firefox 149 removed the legacy single-step `screenshotContentActor.captureScreenshot` method. Code relying on that method will fail.

2. **Two-step protocol now required.** The screenshot flow is now:
   - Step 1: `screenshotContentActor.prepareCapture` -- returns capture metadata
   - Step 2: `screenshotActor.capture` (root actor) -- performs the actual capture

3. **The `rect` field from `prepareCapture` MUST be forwarded to `capture`.** The `prepareCapture` response includes a `rect` field (with `left`, `top`, `width`, `height`) that must be passed through to `capture`. Without it, Firefox times out because `drawSnapshot` does not know the capture region.

4. **`rect` semantics by capture type:**
   - Viewport-only captures: `rect` is `null`
   - Fullpage/element captures: `rect` contains the region dimensions (`left`, `top`, `width`, `height`)

5. **Internal call chain.** The root `screenshotActor.capture` internally calls `browsingContext.currentWindowGlobal.drawSnapshot(rect, ratio, bgColor, fullpage)`.

6. **WebDriver BiDi is not applicable.** `browsingContext.captureScreenshot` is a separate protocol (Marionette port, not DevTools RDP) and cannot be used here.

## Firefox Source Files

- `devtools/server/actors/screenshot.js` -- root screenshot actor
- `devtools/server/actors/screenshot-content.js` -- content process actor
- `devtools/server/actors/utils/capture-screenshot.js` -- actual capture logic
- `devtools/shared/specs/screenshot.js` -- protocol spec (root actor)
- `devtools/shared/specs/screenshot-content.js` -- protocol spec (content actor)

## Fix Applied

Added `rect` (as `Option<CaptureRect>`) to the `PrepareCapture` struct and forwarded it in `ScreenshotActor::capture` args. This ensures the capture region from `prepareCapture` is always passed through to the root actor, preventing the timeout caused by a missing region in `drawSnapshot`.
