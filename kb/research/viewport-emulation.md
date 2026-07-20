---
title: Viewport emulation over Firefox RDP (mobile screenshots)
date: 2026-07-20
type: research
tags: [rdp, viewport, emulation, research]
firefox_version: "152.0.6"
iteration: iteration-133
verdict: no-subfloor-emulation-over-rdp
---

# Viewport emulation over Firefox RDP

Research spike for [[iteration-133-viewport-emulation]]. Settles two contradictory
positions about whether ff-rdp can do true mobile viewport emulation
(e.g. 390×844 with real media-query evaluation) over Firefox's **RDP TCP wire**
(not WebDriver BiDi).

## TL;DR verdict

**The standing position is correct; the "new claim" is false.**

- ❌ **No RDP actor sets viewport size.** The `target-configuration` actor's
  `SUPPORTED_OPTIONS` has no width/height/customViewport field. Empirically, a
  `customViewport` patch is silently stripped by the server (echoes back `{}`),
  and `innerWidth` / media queries do not change.
- ❌ **RDM does NOT do viewport sizing over RDP.** Firefox Responsive Design
  Mode sets the viewport **client-side in the parent chrome process** by
  resizing the `<iframe class="browser">` element (CSS custom properties
  `--rdm-width` / `--rdm-height` on `.browserStack`, plus a Redux dispatch).
  There is no protocol message involved — it is local DOM manipulation only
  possible from inside the DevTools chrome, which a remote TCP client is not.
- ✅ **Headless `--window-size=W,H` gives true small viewports — but ONLY in
  `--screenshot` (batch) mode**, where Firefox exits after capture. It does
  **not** apply to a normally-launched headless instance running a
  `--start-debugger-server` (the mode ff-rdp uses), which defaults to 1366 wide.
- ⚠️ **The interactive/live content window has a ~500px floor.** `-width 320`
  and `-width 390` both clamp live `innerWidth` up to **500** on macOS headless.
- ✅ **`emulate --dppx` (overrideDPPX) works and is independent of width** — you
  can set DPR=3 while width stays at the window width.

## Empirical results (the decisive data)

Firefox 152.0.6, macOS, headless. Two distinct launch modes tested because they
behave completely differently.

### A. Headless `--screenshot` batch mode (Firefox exits after capture)

| Launch flag | PNG width × height | Verdict |
|---|---|---|
| `--window-size=390,844` | **390 × 844** | honored exactly, no floor |
| `--window-size=320,568` | **320 × 568** | honored, even below 500 |
| `-width 390 -height 844` | 1366 × 768 | **ignored** in screenshot mode |

`--window-size=W,H` is the working knob for one-shot headless screenshots. The
`-width/-height` pair does **not** affect the `--screenshot` path.

### B. Normal headless launch + `--start-debugger-server` (ff-rdp's mode), live `innerWidth` read over RDP

| Launch flag | live `innerWidth × innerHeight` | `matchMedia(max-width:400px)` | DPR |
|---|---|---|---|
| `--window-size=390,844` | **1366 × 683** | false | 1 |
| `-width 390 -height 844` | **500 × 759** | false | 1 |
| `-width 320 -height 568` | **500 × 483** | false | 1 |

Two findings: `--window-size` is **ignored** for a debugger-server instance
(defaults to 1366); `-width/-height` are **honored but clamped to a ~500px
floor**. Neither reaches 390, and no media query below 500px ever fires.
`screen.width` reports the host (1366×768) in all cases.

### C. `customViewport` over RDP `updateConfiguration` (the "new claim" mechanism)

ff-rdp already ships `TargetConfigurationFront::set_custom_viewport_size` which
sends `{"customViewport": {"width", "height"}}`. Tested against a live instance
via geckordp (raw `updateConfiguration`):

```
sent:  {"customViewport": {"width": 390, "height": 844}}
echo:  {"configuration": {}, "from": "...target-configuration4"}   ← empty, stripped
live:  innerWidth=1366, mq400=false, dpr=1                          ← unchanged
```

Positive control (mixed patch) proves the echo reliably reports accepted options:

```
sent:  {"overrideDPPX": 3.0, "customViewport": {"width": 390, "height": 844}}
echo:  {"configuration": {"overrideDPPX": 3}}                       ← only DPPX kept
live:  innerWidth=1366, mq400=false, dpr=3                          ← DPR changed, width did not
```

`overrideDPPX` is echoed and takes effect; `customViewport` is dropped from the
echo and has no effect. **Definitive: the target-configuration actor does not
support viewport sizing.** `set_custom_viewport_size` is a dead primitive —
it wires to a field the server ignores.

## Source citations (searchfox, mozilla-central)

### target-configuration has no viewport field

- **`devtools/server/actors/target-configuration.js`** — `SUPPORTED_OPTIONS`
  (lines 33–67) enumerates every accepted option:
  `animationsPlayBackRateMultiplier, cacheDisabled, colorSchemeSimulation,
  customFormatters, customUserAgent, enabledHighlighters, isTracerFeatureEnabled,
  javascriptEnabled, networkBodyLimit, overrideDPPX, printSimulationEnabled,
  rdmPaneMaxTouchPoints, rdmPaneOrientation, recordAllocations,
  reloadOnTouchSimulationToggle, restoreFocus, serviceWorkersTestingEnabled,
  setTabOffline, touchEventsOverride, tracerOptions,
  useSimpleHighlightersForReducedMotion`. **No width/height/customViewport.**
  The RDM-prefixed keys are `rdmPaneOrientation` (→ `setOrientationOverride`) and
  `rdmPaneMaxTouchPoints` (→ `setRDMPaneMaxTouchPoints`) — orientation and touch
  points only, never size.
- **`devtools/shared/specs/target-configuration.js`** — the
  `target-configuration.configuration` dict (lines 15–28) lists the same fields
  as `nullable:*`. No viewport field.

### RDM internals: size is a parent-process CSS resize, NOT an RDP call

Traced the full RDM chain (manager.js → ui.js → index.js + the server responsive
actor), because "target-configuration has no size field" alone doesn't rule out
some *other* RDM mechanism. It does not exist over RDP. Evidence:

- **`devtools/client/responsive/manager.js`** — `openIfNeeded` (lines 78–93)
  just instantiates `ResponsiveUI` and calls `ui.initialize()`. No sizing here;
  it delegates everything to `ui.js` in the parent chrome process.

- **`devtools/client/responsive/ui.js`** — `updateViewportSize` (lines 2055–2062)
  applies the pixel size **purely by setting CSS custom properties** on the
  `.browserStack` container element and then posting a message to the tool
  window — **no RDP command carries the size**:
  ```js
  this.browserStackEl.style.setProperty("--rdm-width",  `${width}px`);
  this.browserStackEl.style.setProperty("--rdm-height", `${height}px`);
  this.browserStackEl.style.setProperty("--rdm-zoom",   zoom);
  ```
  The content is a real `<iframe class="browser" remote>` inside `.browserStack`;
  those CSS vars resize its containing box, and the layout engine reflows the
  content docshell so `innerWidth` and media queries genuinely follow the box.
  **That reflow is exactly the "true emulation with real media-query
  evaluation" — and it is a local layout consequence of resizing a frame element
  the parent chrome owns, reachable only from inside the chrome, never over the
  wire.**

- **Every RDP call `ui.js` makes is size-free.** The complete list of
  `this.commands.*Command` / front calls in ui.js: `targetCommand`
  (watch/unwatch/destroy, lines 1347–1444), `resourceCommand`
  (watch/unwatch), `networkFront` (throttling, lines 2099–2111), and
  `targetConfigurationCommand.updateConfiguration(...)` carrying only
  `overrideDPPX` (line 2082), `setTabOffline` (2100), `customUserAgent` (2127),
  `touchEventsOverride` + `reloadOnTouchSimulationToggle` (2148),
  `rdmPaneOrientation` (2165), and `rdmPaneMaxTouchPoints` (2180).
  **Not one call carries width/height/viewport size.** DPR, offline, UA, touch,
  orientation, and max-touch-points go over RDP; the *size* never does.

- **`devtools/server/actors/responsive.js`** (the server-side actor reachable
  over RDP) exposes only `setElementPickerState`; its spec
  **`devtools/shared/specs/responsive.js`** (methods block, lines ~16–28)
  declares exactly two RDP-callable methods:
  ```js
  methods: {
    toggleTouchSimulator:  { request: { options: Arg(0,"json") },
                             response: { valueChanged: RetVal("boolean") } },
    setElementPickerState: { request: { state: Arg(0,"boolean"),
                                         pickerType: Arg(1,"string") },
                             response: {} },
  }
  ```
  Touch simulation and picker state only — **no viewport/size/width/height/resize
  method exists in the wire protocol**, so a remote RDP TCP client cannot invoke
  one even in principle.

- **`InspectorUtils.setDynamicToolbarMaxHeight(browsingContext, …)`** (ui.js
  ~line 365) is the only browsingContext-touching call in the open path — it
  sets the mobile dynamic-toolbar height, not the viewport size, and
  `InspectorUtils` is a chrome-privileged JS binding, not an RDP actor method.

**Conclusion (RDM-internals-grounded):** RDM's viewport sizing is 100%
parent-process client-side privilege — a CSS resize of a chrome-owned frame
element. The layout reflow that gives real media queries is a free consequence
of that resize, not a protocol feature. No link in the RDM chain (manager, ui,
index, server responsive actor, target-configuration actor) is addressable over
the RDP TCP wire for setting a viewport size.

### CLI window-sizing flags are cross-platform

`-width` / `-height` / `--window-size` / `--headless` are handled in
toolkit/browser command-line handling (`BrowserContentHandler`'s
`getFeatures()` and the headless/gfx layer), not behind per-OS `#ifdef`s.
Verified empirically on macOS; the flags are documented cross-platform Firefox
CLI args (Windows/Linux/macOS). `--window-size` is a headless-shell arg; the
`-width/-height` window-feature args pre-date headless mode and work on all
desktop platforms.

## geckordp / foxdriver corroboration

- **geckordp** (`geckordp/actors/target_configuration.py`) `update_configuration`
  enumerates the supported fields — **no `customViewport`** or any
  viewport/width/height field. A grep of the whole geckordp tree for
  `viewport|responsive|setViewport|dimension|rdm` turns up only
  `node.getOwnerGlobalDimensions` (read-only) and screenshot's
  `browsingContextID` — nothing that *sets* a viewport. A complete Python RDP
  client having no viewport-set path is strong independent evidence.
- **`window.resizeTo()`** is silently ignored in headless mode (already noted in
  ff-rdp's own `responsive.rs` `SET_VIEWPORT_CSS_JS` doc comment).

## dppx interaction (question 4)

`emulate --dppx N` (wire field `overrideDPPX`) is orthogonal to window width. It
sets `window.devicePixelRatio` independently, confirmed live (DPR=3 while width
stayed 1366). For a mobile screenshot you compose:
- window/CSS width for the layout width, and
- `--dppx` for the pixel density that scales the rendered screenshot.

There is no interaction/conflict: DPR is applied by the compositor regardless of
the content width. A 390-wide layout at `--dppx 2` yields a 780px-wide raster.

## Recommendation for iteration-133

Rank the plan's four themes by feasibility:

1. ✅ **`launch --window-size` (recommended primary).** Add a
   `--window-size W,H` (or `--width`/`--height`) flag to `launch` that forwards
   `-width`/`-height` to Firefox. This is **sonnet-implementable**: it is one
   arg pass-through in `build_command` (`crates/ff-rdp-cli/src/commands/launch.rs`)
   plus a live test. **Caveat: the ~500px floor.** For layouts ≥500px CSS width
   this gives *true* emulation (real `innerWidth`, real media queries, real
   layout). Below 500px it clamps to 500. Pair with `emulate --dppx` for density.
   Document the floor explicitly; do not promise 390px live media queries.

2. ✅ **`screenshot --window-size` via a dedicated batch capture (recommended
   for true <500px mobile shots).** For a genuine 390-wide (or 320-wide) mobile
   *screenshot*, shell out to Firefox `--headless --window-size=W,H --screenshot`
   (proven exact PNG dimensions, no floor) as a one-shot, separate from the
   debugger-server session. This is the only way to get a true sub-500px raster.
   Sonnet-implementable but is a new capture path, not the RDP screenshot actor.

3. ❌ **`emulate --viewport` true-emulation over RDP — NOT feasible.** Drop it.
   No RDP actor supports it. Remove/deprecate the dead
   `set_custom_viewport_size` primitive (it sends an ignored field). If a
   `--viewport` UX flag is still wanted, back it by the CSS-constraint approach
   (theme 4) and label it "layout-only", never "true emulation".

4. ⚠️ **`responsive` CSS-constraint integration (already exists, keep as-is).**
   The current `responsive` command constrains layout via inline
   `document.documentElement.style.width`. It is honest about being layout-only
   (`offsetWidth` reflects the constraint; `innerWidth`/media queries do not).
   Fine for geometry/breakpoint auditing; not viewport emulation. No change
   needed beyond clearer docs distinguishing it from themes 1–2.

**Scope hinge:** iter-133 should be reframed from "true viewport emulation over
RDP" (impossible) to "launch/screenshot window-size + dppx composition, with a
documented 500px live floor and a batch-screenshot escape hatch for sub-500px
mobile rasters." That reframed scope **is sonnet-implementable**.

## Related

- [[project_viewport_protocol]] — prior conclusion (confirmed): no RDP viewport
  actor; CSS width constraints for responsive testing.
- [[iteration-103-emulate]] — target-configuration front (overrideDPPX etc.).
