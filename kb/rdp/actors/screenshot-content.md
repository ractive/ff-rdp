---
type: rdp-note
tags:
  - rdp
  - firefox-server
  - actor
  - screenshot
  - content-process
date: 2026-05-23
firefox_files:
  - devtools/server/actors/screenshot-content.js
  - devtools/shared/specs/screenshot-content.js
title: ScreenshotContentActor
---

# ScreenshotContentActor (typeName `"screenshot-content"`)

Per-target content-process actor. Sibling of [[console]] and [[walker]] on each WindowGlobalTargetActor.

- Source: `devtools/server/actors/screenshot-content.js` (144 lines).
- Spec:   `devtools/shared/specs/screenshot-content.js`.

## Method

```
prepareCapture({
  fullpage?:    boolean,
  selector?:    string,
  nodeActorID?: number,
}) → {
  rect:      {left, top, width, height} | null,
  windowDpr: number,
  windowZoom: number,
  messages:  [{level, text}, …],
  error?:    boolean,
}
```

**Does NOT actually take a screenshot.** It just computes the rect to render from inside the page (which is the only context that can run `document.querySelector`, read `window.scrollMaxX`, …) and hands the result to the parent-process [[screenshot]] actor that calls `drawSnapshot`.

## Behaviors

- **Default (no fullpage / selector / nodeActorID)**: returns `{ rect: null, … }`. Caller then passes `null` to `drawSnapshot`, which renders the current viewport.
- **`fullpage: true`**: returns the full-page rect (`width = innerWidth + scrollMaxX - scrollMinX − scrollbar`, height analogous). See [[screenshot]] for details.
- **`selector`**: `document.querySelector(selector)` then `getRect(originWindow, node, node.documentGlobal)` from `devtools/shared/layout/utils.js`. If no match, returns `{ error: true, messages: [warn] }`.
- **`nodeActorID`**: looks up via `this.conn.getActor(nodeActorID)` — the NodeActor from [[walker]]. Returns `{error: true}` if not found.

## DPR computation gotcha

```js
const windowDpr =
  window.browsingContext.top.overrideDPPX || window.devicePixelRatio;
```

Comment: *"Whether zoom is included in devicePixelRatio depends on whether there's an override, this is a bit suspect"* (FIXME bug 1760711). Translation: if the user passes `dpr` to the screenshot, the override is in effect and zoom may not stack the way the caller expects.

## ignoreSubFrames

`_getRectForNode` chooses between `node.documentGlobal` and `node.documentGlobal.top` based on `this.targetActor.ignoreSubFrames`. That's a target-actor flag that controls whether iframe content is treated as one document tree.

## Lifecycle

- Created by the target actor's constructor (WindowGlobalTargetActor).
- Destroyed with the target.
- Has no events.
