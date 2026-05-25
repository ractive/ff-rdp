---
type: rdp-note
tags:
  - rdp
  - firefox-server
  - actor
  - screenshot
  - critical
  - full-page-bug
date: 2026-05-23
firefox_files:
  - devtools/server/actors/screenshot.js
  - devtools/server/actors/utils/capture-screenshot.js
  - devtools/shared/specs/screenshot.js
title: ScreenshotActor
---

# ScreenshotActor (typeName `"screenshot"`)

The **parent-process** screenshot actor, attached to RootActor (singleton, see `RootActor.getRoot`).

- Source: `devtools/server/actors/screenshot.js` — **only 25 lines**.
- Util:    `devtools/server/actors/utils/capture-screenshot.js`.
- Spec:    `devtools/shared/specs/screenshot.js`.

## Method

```
capture({
  fullpage?:  boolean,
  file?:      boolean,
  clipboard?: boolean,
  selector?:  string,
  dpr?:       string,    // note: STRING, not number
  delay?:     string,    // seconds
}) → json
```

Implementation (literal source):

```js
async capture(args) {
  const browsingContext = BrowsingContext.get(args.browsingContextID);
  return captureScreenshot(args, browsingContext);
}
```

Returns `{ data: dataURL, height, width, filename, messages: [{level, text}, …] }`.

## The two-actor dance — IMPORTANT

The `screenshot` actor itself is paper-thin. Real work is split:

1. **`screenshot-content` actor** (per target, content-process) — see [[screenshot-content]] — its `prepareCapture({fullpage, selector, nodeActorID})` runs inside the page and returns a `rect` plus `windowDpr/windowZoom`. For the default current-viewport case it returns `{rect: null}`.
2. **`screenshot` actor** (root, parent-process) — its `capture()` then calls `browsingContext.currentWindowGlobal.drawSnapshot(rect, ratio, "rgb(255,255,255)", fullpage)`.

The Firefox DevTools client orchestrates this two-step flow in `devtools/client/shared/screenshot.js`.

## `drawSnapshot` signature (the source of full-page truth)

```js
const snapshot = await browsingContext.currentWindowGlobal.drawSnapshot(
  rect,                  // DOMRect or null (null = current viewport)
  actualRatio,           // device pixel ratio
  "rgb(255,255,255)",    // background color
  args.fullpage          // boolean — THIS is what makes full-page actually render
);
```

Inside `capture-screenshot.js:114`. Note that **`fullpage` is the 4th positional argument** to `drawSnapshot`; passing only a large rect is not enough — without `fullpage: true` Gecko clips at the visual viewport boundaries.

## Full-page rect computation (in screenshot-content.js)

```js
if (fullpage) {
  const winUtils = window.windowUtils;
  const scrollbarHeight = {}, scrollbarWidth = {};
  winUtils.getScrollbarSize(false, scrollbarWidth, scrollbarHeight);
  left = 0; top = 0;
  width  = window.innerWidth  + window.scrollMaxX - window.scrollMinX - scrollbarWidth.value;
  height = window.innerHeight + window.scrollMaxY - window.scrollMinY - scrollbarHeight.value;
}
```

So the "page width/height" is `innerWidth + scrollMaxX - scrollMinX − scrollbar`. Critical for ff-rdp's --full-page bug: if we only set a custom `width/height` rect without also passing `fullpage: true` to `drawSnapshot`, Gecko will clip.

## Other behaviors

- Auto-clamps to safe max dimensions via `clampDimensionsIfNeeded` from `browser/components/screenshots/ScreenshotsUtils.sys.mjs`. If clamping happens, a `screenshotTruncationWarning` is pushed into `messages`.
- If `drawSnapshot` returns null at ratio > 1, retries at ratio 1.0 and adds `screenshotDPRDecreasedWarning`.
- Triggers `simulateCameraFlash` on `browsingContext.topFrameElement` unless `disableFlash` or `prefers-reduced-motion`.
- `filename` defaults to a generated `Screen Shot <date>.png` (or `…-fullpage.png` if fullpage).
- `args.rect` (if present) is a plain object that gets converted to `new globalThis.DOMRect(...)`.

## Gotchas for ff-rdp

- **The full-page bug**: if your CLI computes a giant rect from `document.documentElement.scrollWidth/Height` but does not set the actor's `fullpage: true`, Firefox will still clip to the viewport. The fix is to either pass `fullpage: true` in the `capture` args or compute a rect from `scrollMax{X,Y}` and pass it to the screenshot-content actor.
- `dpr` is typed as **string** in the spec. ff-rdp serialises it as a JSON string (e.g. `"2"`) since iter-70 — see `crates/ff-rdp-core/src/actors/screenshot.rs::ScreenshotActor::capture`. Closed.
- `browsingContextID` must be the **content browsing context** id (from TabDescriptor.form's `browsingContextID`), not the chrome window id.
- `data:` URL can be huge — for the parent process actor there's no streaming, the whole base64 PNG comes back in one JSON packet. ff-rdp must be ready to receive multi-MB responses.
- The screenshot util is in `browser/components/screenshots/`, **not** in devtools — Firefox UI screenshots share the same backend. Updates to clamping/DPR logic may land in the non-devtools path.

## Iter-76 update — bulk transport path

- The default screenshot path still returns base64 JSON (matches spec). The new `--bulk` CLI flag opts into the transport-level BULK_RESPONSE carrier via `Transport::recv_bulk_with_handler`, copying the PNG bytes in 8 KiB chunks straight to the output file. No full-body buffer alloc; peak RSS scales with chunk size, not image size.
- `--bulk` is a daemon-side optimisation; bytewise output must match the base64 path (`cmp` exit 0).

## Iter-77 update — ScreenshotArgsExt typed shim

- `crates/ff-rdp-core/src/actors/screenshot.rs::ScreenshotArgsExt` is now the
  single construction site for the outbound `capture` args.  It documents the
  spec drift explicitly: `browsingContextID`, `snapshotScale`, and `rect` are
  read by `devtools/server/actors/screenshot.js` but NOT declared in
  `devtools/shared/specs/screenshot.js:13-35`.  The struct carries an
  `allow-spec-drift: bug` annotation pointing at the upstream tracker.
- `snapshotScale` is still omitted when `windowDPR * windowZoom == 1.0` so
  outbound bytes are unchanged from the pre-iter-77 baseline.
