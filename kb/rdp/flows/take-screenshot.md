---
type: rdp-note
tags: [rdp, firefox-client, flow, screenshot, bug-lookup]
date: 2026-05-23
firefox_files:
  - devtools/client/shared/screenshot.js
  - devtools/shared/specs/screenshot.js
  - devtools/shared/specs/screenshot-content.js
  - devtools/server/actors/screenshot.js
  - devtools/server/actors/screenshot-content.js
  - devtools/server/actors/utils/capture-screenshot.js
  - browser/components/screenshots/
---

# Flow: Take a (full-page) screenshot

**Critical lookup for ff-rdp `--full-page` bug.** This file documents the
DevTools two-actor screenshot path *exactly* as Firefox itself implements it.
If your screenshot is just-the-viewport when you asked for fullpage, you
likely skipped step 3.

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

**This is the step ff-rdp is missing.** If you don't call this, you don't
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
[`browsingContext.currentWindowGlobal.drawSnapshot(rect, ratio, "rgb(255,255,255)", fullpage)`](https://searchfox.org/mozilla-central/search?q=drawSnapshot)
at line 114-119, then draws the resulting `ImageBitmap` to a canvas, then
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

## ff-rdp fix sketch

Our `screenshot --full-page` command should:

1. `getTarget` on the chosen tab descriptor → form contains
   `screenshotContentActor` and `browsingContextID`.
2. Send `prepareCapture` to `screenshotContentActor` with
   `{fullpage: true}`. Read back `rect`, `windowDpr`, `windowZoom`.
3. Compute `snapshotScale = dpr * windowZoom`.
4. Send `capture` to the **root** `screenshot` actor with
   `{fullpage:true, rect, snapshotScale, browsingContextID, filename, dpr}`.
5. Decode `data` (a `data:image/png;base64,...` URL) and write to disk.

## Backward-compat note

`captureScreenshot` checks `targetFront.hasActor("screenshotContent")` at
line 71. If the server lacks the content-side actor (Fx <87 or some
non-tab targets), it falls back to a single `target.getFront("screenshot")`
call which does everything in the content process — but that path can't do
true fullpage. ff-rdp targets modern Firefox so this fallback isn't needed.
