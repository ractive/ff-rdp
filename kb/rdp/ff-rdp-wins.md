---
title: "RDP findings that should drive ff-rdp improvements"
type: rdp-note
tags: [rdp, ff-rdp, action-items, derived-from-wiki]
date: 2026-05-23
---

# RDP findings that should drive `ff-rdp` improvements

Distilled from the wiki build (kb/rdp/) on 2026-05-23. Items listed roughly in descending impact for our current open bugs (iter-61l scope and beyond).

## 1. `--full-page` screenshot — actually fix it (bug-of-record across sessions 48/49/51/52/53)

**Finding** ([[take-screenshot]] · [[screenshot]] · [[screenshot-content]]):
The DevTools "Take full-page screenshot" command is **two** RDP round-trips across **two different actor scopes**:

1. `screenshot-content.prepareCapture({fullpage:true})` on the **content-process** actor (got via the target's form). Returns `{rect, windowDpr, windowZoom}` where `rect = {x:0, y:0, width: innerWidth + scrollMaxX - scrollMinX - scrollbarWidth, height: innerHeight + scrollMaxY - scrollMinY - scrollbarHeight}`.
2. Client computes `snapshotScale = dpr * windowZoom`, stashes `browsingContextID`.
3. `screenshot.capture({fullpage:true, rect, snapshotScale, browsingContextID, …})` on the **root-scoped** `screenshot` actor — `client.mainRoot.getFront("screenshot")`, **NOT** the target. Internally calls `browsingContext.currentWindowGlobal.drawSnapshot(rect, ratio, "rgb(255,255,255)", fullpage)`.

The 4th positional arg `fullpage` to `drawSnapshot` is the actual switch that makes Gecko render outside the viewport. **A custom rect alone does not work** — that's why iter-61j and iter-61k both shipped rect-override code and live verification still showed 800×600 PNGs.

**Action**: rewrite the `--full-page` path in `ff-rdp` to:
- Resolve the **root-scoped** `screenshot` actor (separate from any target's `screenshot-content`).
- Call `prepareCapture` on the content-scope actor first to get the rect.
- Pass `fullpage:true, rect, snapshotScale, browsingContextID` to `capture`.
- Verify with a live test on a ≥5000 px synthetic page.

Refs: `devtools/server/actors/screenshot-content.js:85-103`, `devtools/server/actors/utils/capture-screenshot.js:114-119`, `devtools/client/shared/screenshot.js:97-104`.

## 2. `eval` blocked by CSP — use `mapped: { await: true }`

**Finding** ([[evaluate-js]] · [[console]]):
`webconsoleActor.evaluateJSAsync` with the option `mapped: { await: true }` is awaited on the server using SpiderMonkey's `Debugger` API. Debugger-driven evaluation is privileged and **bypasses page CSP entirely**. The Firefox DevTools console enables this whenever the input parses as top-level await. The flag also turns on Promise resolution — so the long-standing `project_rdp_async_constraints` memo (“evaluateJSAsync won't resolve Promises”) is true only without this flag.

`webconsole.js:944` (`awaitResult = result.unsafeDereference()`) is where the awaited value is unwrapped and grip-encoded back into `response.result`.

**Action**:
- Default `ff-rdp eval` to send `mapped: { await: true }`. The result shape is the same on the wire; we already grip-decode `response.result`.
- If the user's expression is a *statement* (not an expression), `await` wrapping fails — detect by trying the awaited path first and falling back to non-await on parse error.
- Surface `meta.eval_path: "await" | "plain"` so callers know which ran.
- Live-verify against `https://news.ycombinator.com` (CSP `script-src 'self' 'unsafe-inline'` — no `unsafe-eval`).

This addresses iter-61l AC-H and the largest single LLM-friendliness gap in `ff-rdp`.

## 3. WatcherActor — engage targets AND resources

**Finding** ([[watcher]] · [[watch-resources]]):
`watchTargets("frame")` and `watchResources([…])` are **both required** before events arrive. Subscribing to resources without first watching targets yields nothing — the watcher's per-target buffers don't exist yet. Events are throttled at `RESOURCES_THROTTLING_DELAY = 100ms` and batched as `[[type, [resources…]], …]`.

**Action**:
- Audit `ff-rdp`'s `--with-network` path: when we engage the watcher, send `watchTargets("frame")` first, then `watchResources(["network-event"])`. Today we may be skipping the targets step.
- The 100ms throttling means short-lived navigations (`example.com`) can race the first flush. Drain the buffer once on `network` invocation.
- Subscribe to `document-event` too — it gives us `dom-loading`, `dom-interactive`, `dom-complete` we can use to gate `navigate`'s commit detection (relevant to iter-61l AC-G race fix).

Refs: `devtools/server/actors/watcher.js`, `devtools/shared/resources/` and our [[../resources/README|resources/]] index.

## 4. consoleActor staleness — Firefox already invalidates; we should too

**Finding** ([[devtools-client]]):
DevTools' `DevToolsClient` invalidates Front references on `targetDestroyed` events. When a target dies (cross-origin navigation, about:neterror, process switch), its actor IDs become invalid. Firefox handles this transparently via the descriptor's `targetAvailable`/`targetDestroyed` events.

**Action** (relevant to iter-61l AC-K):
- Subscribe to `targetDestroyed` (or the watcher's `target-destroyed-form` event) and drop our cached `consoleActor` on receipt.
- Re-resolve `consoleActor` from the descriptor on the next `eval` call.
- Live-verify with the iter-61l test plan: bad-DNS navigate → about:neterror → next `eval` should succeed via re-resolved actor (assuming about:neterror is a real page, which it is; the CSP separately blocks page-level eval — fix #2 above addresses that).

## 5. `--with-network` → `network --headers` data path

**Finding** ([[rdp/actors/network-event|network-event]] · [[network-content]]):
Response headers come from `NetworkEventActor`'s `getResponseHeaders` request, returning a `headers` array. The `NetworkContentActor` provides response **bodies** separately, via `getResponseContent`. Both require the watcher to be engaged for the event in question — performance-api has no header data path at all.

**Action** (iter-61l AC-C and the N1 regression):
- `network` (no flags) should read the daemon's per-target watcher buffer when available, falling back to performance-api only when truly empty.
- `network --detail --headers` should issue `getResponseHeaders` per entry against the captured `networkEventActor` IDs and embed the results.
- `meta.source` must stay `"watcher"` whenever the underlying entries came from the watcher buffer, regardless of which optional fields the caller asked for.

## 6. Descriptor vs target — modern flow has no explicit `attach`

**Finding** ([[tab-descriptor]] · [[attach-target]]):
The descriptor/target split (landed ~2020) replaced the old `tabAttach` request with `descriptor.getTarget()` which returns an already-attached target. Calling `attach()` on the resulting target is a no-op (kept for back-compat).

**Action**:
- Remove any explicit `attach` request from `ff-rdp` if it's still there.
- Cache the descriptor->target mapping per tab and only re-resolve on `targetDestroyed`.

## 7. Locale pin — `intl.locale.matchOS=false` plus env

**Finding** (Firefox source spelunking, locale prefs):
On macOS, Firefox's UI locale follows the OS unless `intl.locale.matchOS=false` AND a non-OS locale is available. Even then, certain DevTools / quirks-mode strings come from `chrome://global/locale/intl.properties` whose lookup honors `LANG`/`LC_ALL` env vars set on the parent process.

**Action** (iter-61l AC-B):
- Set `LANG=en_US.UTF-8`, `LC_ALL=en_US.UTF-8` on the spawned Firefox process (without overwriting user-set env).
- Keep `intl.locale.matchOS=false` and `intl.locale.requested=en-US` in user.js.
- This is exactly what iter-61l plans — the wiki just confirms why both halves are needed.

## 8. The spec files are our IDL

**Finding** ([[spec-and-front]]):
`devtools/shared/specs/*.js` files define every protocol method with typed args (`Arg`, `Option`) and typed returns (`RetVal`, `nullable:dom-node`, `grip`, …). They're machine-readable and stable across the actor/spec versions.

**Action**:
- When implementing a new actor in `ff-rdp`, find the matching spec under `devtools/shared/specs/` first. Match arg names exactly — Firefox does no positional fallback.
- The spec also tells you which return values are grips (need our grip decoder) vs primitives.

## 9. Bulk packets exist (but we don't need them yet)

**Finding** ([[rdp/protocol/transport|transport]]):
RDP has a `bulk` packet form: `bulk <actor> <type> <length>:<binary-bytes>`. Used for screenshot data (when sent as raw PNG instead of base64 dataURL) and for stylesheet content over a certain size.

**Action**: not urgent — our actors currently return base64 dataURLs which is fine. Worth noting in case the screenshot fix uncovers a perf cliff at large PNG sizes; switching to bulk would help.

## 10. CDP-over-RDP is gone

**Finding** ([[remote-agent-cdp]]):
Firefox's `/remote/` directory used to bridge CDP→RDP for Puppeteer compatibility. CDP support has been **removed**; only Marionette and WebDriver BiDi remain.

**Action**: do not invest in studying `/remote/cdp/` patterns. If we want an external-friendly protocol surface on top of `ff-rdp`, BiDi is the modern reference.

## Next iteration scope (suggestion)

Lift items 1, 2, 3, 4, 5 into iter-61m's scope (after iter-61l merges). Each one is a concrete, live-verifiable behavioral change with a clear test target. The wiki pages cited under each item are the implementation reference.
