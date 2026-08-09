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

## iter-84: getRoot diagnostic helper (Theme B)

When the screenshot actor is not advertised in the root form (observed on
Firefox 151+), the previous error gave no hint of what actors *were*
present.  `ScreenshotActor::get_root_raw()` returns the unparsed root
reply so callers can list the available actor IDs in their error
message, which improves diagnosability before deciding whether the
actor has moved to a per-target form or been renamed.

## iter-85: Firefox 151+ fallback path via WindowGlobalTarget (Theme B)

On Firefox 151+, `screenshotActor` was observed absent from `getRoot`
(dogfood-57).  The iter-85 fix adds `ScreenshotActor::screenshot_via_target()`
which implements a fallback path:

1. `root.listTabs` → find the selected tab actor.
2. `tabActor.getTarget` → obtain the `WindowGlobalTarget` actor ID.
3. Send a `screenshot` request (or `takeScreenshot` as a secondary fallback)
   directly to the `WindowGlobalTarget` actor.

The CLI's `try_two_step_screenshot` fallback ladder:
- **Path A** (standard): `getRoot` → `screenshotActor.capture`.
- **Path B** (FF151+): if `screenshotActor` absent or module-load failure,
  call `screenshot_via_target()` before giving up.

The target-actor `screenshot` method is not declared in
`devtools/shared/specs/targets/window-global.js` (spec drift); annotated
with `// allow-spec-drift: bug TBD`.

Fixture: `crates/ff-rdp-core/tests/fixtures/getroot_ff151.json` — synthetic
FF 151 `getRoot` shape with no `screenshotActor` field (replace with a
recorded fixture when a live FF 151 instance is available).

Unit tests added:
- `screenshot_via_target_uses_target_screenshot_method` — mock server validates
  the full `listTabs` → `getTarget` → `screenshot` sequence.
- `get_actor_id_returns_error_when_screenshotactor_absent_ff151` — confirms the
  fallback trigger condition (error names the missing field).

Live test: `live_screenshot_ff151_cli` (`live_screenshot_ff151.rs`) —
`#[ignore]` gated; asserts `-o /tmp/x.png` produces a valid PNG file.

## iter-135: `snapshotScale` has NO server default — the Firefox 153 break

**Symptom (Firefox 153.0.3):** every live RDP capture failed with

```
screenshot: screenshotActor.capture failed (invalid packet: screenshotActor
capture response missing 'data' field) — screenshots require headless mode;
relaunch with: ff-rdp launch --headless
```

**The reply shape never drifted.** Recorded from the wire
(`FF_RDP_TRACE_RAW=1 RUST_LOG=trace ff-rdp screenshot`):

```json
{"value":{"data":null,
          "filename":"Bildschirmfoto am 2026-08-09 um 12.17.19.png",
          "messages":[{"level":"error",
                       "text":"Fehler beim Erstellen der Grafik. Sie war wahrscheinlich zu groß."}]},
 "from":"server1.conn6.screenshotActor7"}
```

`data` is present and `null`; the reason sits in `messages` (localised — the
text above is `screenshotRenderingError` in de-DE). ff-rdp discarded `messages`
and reported the field as missing, which read as protocol drift.

### Root cause

`capture-screenshot.js` does **not** default `snapshotScale`:

```js
const ratio = args.snapshotScale;               // no `?? 1`
let data = await drawToCanvas(ratio);           // → drawSnapshot(rect, undefined, …)
if (!data && ratio > 1.0) { … }                 // undefined > 1.0 === false → no retry
if (!data) { messages.push({level:"error", text: L10N.getStr("screenshotRenderingError")}); }
```

Inside `drawToCanvas`, `snapshot.width / actualRatio` is `NaN`, so
`canvas.width = NaN` and `canvas.toDataURL` throws; the `catch` returns `null`.

Since iter-77 ff-rdp omitted `snapshotScale` whenever `windowDpr * windowZoom
=== 1.0`, on the (wrong) belief that the server defaulted it. **It does not.**

### Why it only surfaced on 153

On Firefox 149–152, `screenshotActor.capture` failed earlier, at actor-module
load (`capture-screenshot.js` imported `ScreenshotsUtils.sys.mjs` without
`{ global: "shared" }`), and ff-rdp fell back to the parent-process
`drawSnapshot` path. **Firefox 153 fixed the module load** —
[Bug 2043900](https://bugzilla.mozilla.org/show_bug.cgi?id=2043900),
`414cbad5bf8b`, "[devtools] Fix screenshot features in the Browser Toolbox" —
so the request finally reached the renderer and the latent omission became
fatal. This also means the iter-89/iter-117 SD-2 module-load workaround is no
longer *needed* on 153, though it stays for older builds.

### Fix

- `ScreenshotArgsExt::snapshot_scale` is now a plain `f64`, **always
  serialised**; `ScreenshotFront::capture` likewise always sends `Some(scale)`.
- `parse_capture_response()` (new, `actors/screenshot.rs`) folds the reply's
  `messages` into the error via `capture_no_image_data_error()` instead of
  discarding them. Both the root-actor and `screenshot_via_target` paths use it.
- `specs::screenshot::response::CaptureValue.data` is `Option<String>` and the
  struct gained `messages: Vec<CaptureMessage>` — the old `data: String` failed
  to deserialise a `data: null` reply outright.
- The CLI retries through the `drawSnapshot` fallback when a capture comes back
  with no image data (`CAPTURE_NO_IMAGE_DATA` marker).

### Reply fields observed on 153 (success)

```json
{"value":{"data":"data:image/png;base64,…","height":683,"width":1366,
          "filename":"…png","messages":[]}}
```

`height`/`width` are **not** in the spec's `RetVal("json")` description but are
always present; ff-rdp reads dimensions from the PNG IHDR instead, so it does
not depend on them.

### Fixtures (recorded from Firefox 153.0.3, never hand-written)

- `tests/fixtures/capture_screenshot_response.json` — success shape (base64
  payload truncated to 160 chars for reviewability).
- `tests/fixtures/capture_screenshot_no_image_data_response.json` — the
  `data: null` shape, recorded by deliberately re-sending the pre-iter-135
  request without `snapshotScale`.

Recorders: `live_record_capture_screenshot` and
`live_record_capture_screenshot_no_image_data` in
`crates/ff-rdp-core/tests/live_record_fixtures.rs`.

Tests: `crates/ff-rdp-core/tests/screenshot_capture_shapes.rs`
(`unit_screenshot_capture_parses_ff153_shape`,
`unit_screenshot_capture_parses_legacy_shape`) and
`crates/ff-rdp-cli/tests/live/live_135_screenshot_ff153.rs`.

### Error-message correction (Theme C)

The old failure text told users to relaunch headless *even when they already
were*, and appended "screenshot actor not found in Firefox N root form" on a
path only reached **after** an actor was found and called. Both claims are gone;
`capture_failure_message()` now states that Firefox rendered no image and
suggests dropping `--full-page` or running `ff-rdp doctor`.
`screenshot_errors_carry_no_headless_relaunch_hint` greps the module source so
the hint cannot be reintroduced.
