---
type: rdp-note
tags:
  - rdp
  - firefox-client
  - flow
  - screenshot
  - bug-lookup
date: 2026-05-23
firefox_files:
  - devtools/client/shared/screenshot.js
  - devtools/shared/specs/screenshot.js
  - devtools/shared/specs/screenshot-content.js
  - devtools/server/actors/screenshot.js
  - devtools/server/actors/screenshot-content.js
  - devtools/server/actors/utils/capture-screenshot.js
  - browser/components/screenshots/
title: "Flow: Take a screenshot"
---

# Flow: Take a (full-page) screenshot

This file documents the DevTools two-actor screenshot path and ff-rdp's
parent-process fallback. The release-qualified signature below supersedes
the older boolean-only description (iteration 257, 2026-09-14).

## Two screenshot subsystems in Firefox

There are *two* unrelated screenshot code paths in the tree. Don't confuse
them:

1. **DevTools `:screenshot` command** (the one we care about, what RDP
   exposes). Client entry: `devtools/client/shared/screenshot.js`. Server:
   two actors — `screenshot-content` (in the content process) and
   `screenshot` (in the parent process).
2. **Firefox Screenshots feature** in `browser/components/screenshots/`.
   This is the in-product "take a screenshot" UI exposed in the browser
   chrome. Different code path entirely; uses `drawSnapshot` but does its
   own rect computation. Not addressable via RDP. Ignore for our purposes.

## The DevTools path — three actor calls

`captureScreenshot()` at
`devtools/client/shared/screenshot.js:68-116` is the authoritative client
implementation. It runs **three RDP requests** when fullpage / selector /
nodeActorID is involved (the case our bug hits). For viewport-only it can
shortcut to two.

### Step 1 — `screenshot-content.prepareCapture(args)` (content process)

Spec: `specs/screenshot-content.js`. Sent to the
`screenshotContentActor` from the target form (see [[attach-target]]).

```json
{"to":"...screenshotContent4","type":"prepareCapture",
 "args":{"fullpage":true,"selector":null,"nodeActorID":null}}
```

Implementation: `devtools/server/actors/screenshot-content.js:57-143`.

For `fullpage: true`, lines 85-103 compute the actual rect using
`window.innerWidth + window.scrollMaxX - window.scrollMinX - scrollbarWidth`
(and the symmetric Y form). It also returns `windowDpr` and `windowZoom`.

Without this step or an equivalent document measurement, you don't
know the full-page dimensions, and the parent-process actor at step 3 will
default `rect = null`, which `drawSnapshot` interprets as "current viewport".

Reply shape:

```json
{"rect":{"left":0,"top":0,"width":1280,"height":4823},
 "windowDpr":2,"windowZoom":1,"messages":[]}
```

### Step 2 — Client computes scales

`screenshot.js:97-104`:

```js
args.dpr ||= windowDpr;
args.snapshotScale = args.dpr * windowZoom;
if (args.ignoreDprForFileScale) args.fileScale = windowZoom;
args.browsingContextID = targetFront.browsingContextID;
```

`browsingContextID` is critical for step 3 — the parent-process actor uses
it to find the right `BrowsingContext` to draw.

### Step 3 — `screenshot.capture(args)` on the **root-scoped** actor (parent process)

Spec: `specs/screenshot.js`. Note `screenshot.js:108-110`:

```js
const rootFront = targetFront.client.mainRoot;
const parentProcessScreenshotFront = await rootFront.getFront("screenshot");
const captureResponse = await parentProcessScreenshotFront.capture(args);
```

The `capture` actor lives on **root**, not on the target. This matters: you
get it via `RootFront.getFront("screenshot")`, *not* `target.getFront(...)`.
Wire packet:

```json
{"to":"...screenshot1","type":"capture",
 "args":{"fullpage":true,
         "rect":{"left":0,"top":0,"width":1280,"height":4823},
         "snapshotScale":2,"browsingContextID":12,
         "filename":"...","dpr":2}}
```

Implementation: `devtools/server/actors/utils/capture-screenshot.js:73-182`.

The actor calls
[`browsingContext.currentWindowGlobal.drawSnapshot(rect, ratio, "rgb(255,255,255)", {resetScrollPosition: args.fullpage})`](https://raw.githubusercontent.com/mozilla-firefox/firefox/FIREFOX_155_0_1_RELEASE/devtools/server/actors/utils/capture-screenshot.js)
at Firefox 155.0.1 lines 115–120, then draws the resulting `ImageBitmap` to a canvas, then
`canvas.toDataURL("image/png", "")` and returns:

```json
{"data":"data:image/png;base64,iVBOR...","width":1280,"height":4823,
 "filename":"...-fullpage.png","messages":[]}
```

Note `filename` is auto-suffixed with `-fullpage` at line 78-80 of
`capture-screenshot.js` when `args.fullpage` is true.

## Common failure modes

| Symptom | Cause |
|---|---|
| Got viewport, asked fullpage | Skipped `prepareCapture` → `rect=null` → `drawSnapshot` defaults to viewport. |
| Got blank / `data: null` | Image was too big, hit OOM; `drawToCanvas` returned null; check `messages` for `screenshotDPRDecreasedWarning` or `screenshotRenderingError`. |
| Wrong scale | Forgot to set `snapshotScale = dpr * zoom`. |
| Wrong browsing context (e.g. screenshot of about:blank) | Forgot `browsingContextID` in args. |
| Got truncated fullpage | `clampDimensionsIfNeeded` in `capture-screenshot.js:87` capped the dimensions. `messages` carries `screenshotTruncationWarning` in this case. |

## Standard actor sequence

The standard actor sequence is:

1. `getTarget` on the chosen tab descriptor → form contains
   `screenshotContentActor` and `browsingContextID`.
2. Send `prepareCapture` to `screenshotContentActor` with
   `{fullpage: true}`. Read back `rect`, `windowDpr`, `windowZoom`.
3. Compute `snapshotScale = dpr * windowZoom`.
4. Send `capture` to the **root** `screenshot` actor with
   `{fullpage:true, rect, snapshotScale, browsingContextID, filename, dpr}`.
5. Decode `data` (a `data:image/png;base64,...` URL) and write to disk.

## Firefox 155 signature and ff-rdp compatibility (iteration 257)

The actual path is `dom/chrome-webidl/WindowGlobalActors.webidl`, not
`dom/webidl/WindowGlobalActors.webidl`. Verified release-tagged source:

- [Firefox 120.0](https://raw.githubusercontent.com/mozilla-firefox/firefox/FIREFOX_120_0_RELEASE/dom/chrome-webidl/WindowGlobalActors.webidl), lines 155–158, and
  [154.0.1](https://raw.githubusercontent.com/mozilla-firefox/firefox/FIREFOX_154_0_1_RELEASE/dom/chrome-webidl/WindowGlobalActors.webidl), lines 215–218:
  argument 4 is `optional boolean resetScrollPosition = false`.
- [Firefox 155.0.1](https://raw.githubusercontent.com/mozilla-firefox/firefox/FIREFOX_155_0_1_RELEASE/dom/chrome-webidl/WindowGlobalActors.webidl), lines 90–102 and 227–230:
  argument 4 is `optional DrawSnapshotOptions options = {}`. The dictionary
  members are `boolean resetScrollPosition = false` and `boolean drawView = false`.
  Firefox 155.0 has the same declaration.

[Bug 2058388](https://bugzilla.mozilla.org/show_bug.cgi?id=2058388), final landed
[commit 78f289876b9c2022059c951097c71713142c67a0](https://github.com/mozilla-firefox/firefox/commit/78f289876b9c2022059c951097c71713142c67a0),
changed this signature. The first stable release is **155.0**. This is separate
from the historical Firefox 151 actor-module loading problem (fixed on 153 by
bug 2043900). The local July 8 Firefox checkout is not a 155 source tree.

With a non-null rectangle and `drawView: false`, `resetScrollPosition` temporarily
resets the root scroll frame for fixed-element positioning. `drawView: true`
instead uses viewport-relative coordinates and includes root scrollbars. Null
rectangles always render the visible viewport. ff-rdp keeps `drawView: false`
for document captures, passes a full-document `DOMRect`, and retains null-rect
viewport capture, scale 1, and its fixed/sticky freeze/restore cleanup.

The fallback tries the boolean first, preserving both values on Firefox 120–154.
Only the exact `TypeError: WindowGlobalParent.drawSnapshot: Argument 4 can't be
converted to a dictionary.` permits one retry with
`{resetScrollPosition: <same boolean>, drawView: false}`. Conversion failed
before rendering, so this retry does not duplicate a completed capture. Other
errors and the retry's own error propagate. A dictionary-first try/catch would
silently coerce an object to `true` on old WebIDL and cannot detect support.

The CLI deliberately bypasses `ScreenshotActor::capture` for every full-page
request, retaining iteration 92's Firefox 151 viewport-clamp workaround. Thus
the seven Firefox 155 failures do not prove a primary-actor failure: the actor
was never called. Firefox 155's upstream primary helper already uses the new
dictionary. The iteration257 direct primary-actor probe on Firefox155.0.1
returned a decoded1366×4000 PNG after `prepareCapture(fullpage=true)` returned
that same rectangle: no primary failure occurred in that measurement.

The checked-in dogfood script also captured viewport1366×683 and fullpage1366×4000
on both the absolute official120.0 runtime and installed155.0.1, preserving
scrollY500, viewport dimensions and fixed-header style. The new direct-core
live guard passes on155, and disabling the retry makes both its unit test and
live guard fail with the dictionary TypeError. All seven original full-page
regressions passed in iteration257's own dual-gate sweep (342pass/2unrelated
consent failures across344 exact names). See
[[iteration-257-firefox-155-drawsnapshot-dictionary-arg]] for retained artifacts,
source provenance and explicit unrelated-failure dispositions.

## Backward-compat note

`captureScreenshot` checks `targetFront.hasActor("screenshotContent")` at
line 71. If the server lacks the content-side actor (Fx <87 or some
non-tab targets), it falls back to a single `target.getFront("screenshot")`
call which does everything in the content process — but that path can't do
true fullpage. ff-rdp targets modern Firefox so this fallback isn't needed.
