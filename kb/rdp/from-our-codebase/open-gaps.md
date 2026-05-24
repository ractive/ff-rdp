---
title: Open Protocol-Level Gaps in ff-rdp
type: rdp-note
tags: [rdp, from-codebase, gaps]
date: 2026-05-23
closed-in:
  - iter-61n
  - iter-61q
  - iter-61r
  - iter-61v
  - iter-61w
---

# Open Protocol-Level Gaps

Catalog of known RDP-layer gaps as of 2026-05-23, drawn from dogfooding sessions 48–53 and iterations 61g–61l. Each item: symptom, where the gap lives in the protocol, suggested investigation. Excludes UX-only issues — see the dogfooding session notes for the full list.

## with-network-fallthrough

**Symptom**: `navigate --with-network` engages the WatcherActor and inline-returns proper `{source: "watcher", status: 200, method: "GET", transfer_size: ...}`. The *next* standalone `network` call falls back to `source: performance-api` with `status: null, method: null` — even though `daemon status` shows `buffer_sizes: {network-event: 209}` (data IS captured).

**Protocol layer**: The watcher subscription appears to be torn down (or its data made unreachable) between the navigate response and the next CLI invocation. The buffer exists but the `network` command's source-selection logic picks performance-api.

**Sessions**: [[dogfooding-session-51]] #5, [[dogfooding-session-52]] AC-C, [[dogfooding-session-53]] AC-C.

**Effect**: response headers (CSP, HSTS, X-Frame-Options, Set-Cookie attributes) are completely unreachable in the security-audit workflow that motivated session 51. This is the single biggest protocol-level gap for the security-audit use case.

## shadow-dom-piercing

**Symptom**: `dom 'selector'` now correctly flags `hasShadowRoot: true` / `shadowMode: "open"` on host nodes (iter-61k) but does not traverse *into* the shadow root. SPAs that use shadow DOM heavily (Lit, web components) are opaque past the host.

**Protocol layer**: WalkerActor has shadow-DOM traversal support; we just don't call it. Need `--include-shadow` flag plumbed through.

**Sessions**: [[dogfooding-session-52]] gap #6, [[dogfooding-session-53]] feature gaps.

## actor-leak-in-daemon

**Status (iter-70)**: still partial. `ScopedGrip` is used in `commands/eval.rs:295` for daemon eval grips, but other grip-consuming call sites (inspector, network response body) still leak.  No soak test has been added.

**Symptom**: Each `evaluateJSAsync` returning an object/longString allocates server-side actor IDs that are never released in long-running daemons. iter-54 task 4 landed `ObjectActor::release` + `ScopedGrip` wrapper as building blocks but didn't wire them into daemon-mode call sites or add a soak test.

**Protocol layer**: We never send `release` to grip actors. Firefox's per-connection actor pool grows without bound.

**Sessions**: surfaced in [[iteration-54-protocol-correctness]] task 4 (deferred sub-tasks 2 & 3); no dogfooding session has reproduced an OOM yet but a 1000-eval soak test was planned.

## legacy-startlisteners-coexistence

**Status (iter-70)**: still open pending iter-71 deduplication work — no parallel-listen experiment has merged yet.

**Symptom**: Console flow uses both `WebConsoleActor.startListeners(["PageError", "ConsoleAPI"])` *and* `WatcherActor.watchResources(["console-message", "error-message"])`. Running both risks double-delivery; iter-54 task 6 wanted to drop the legacy path.

**Protocol layer**: The watcher-only path was found to drop pushes for some actor states during earlier iterations, so the legacy listener was left wired. Needs a parallel-listen experiment + dedup before the legacy can be removed safely.

**Sessions**: noted in [[iteration-54-protocol-correctness]] task 6 (deferred). No live dogfooding session has caught a duplicate.

## viewport-sizing

**Symptom**: No way to programmatically change the viewport via RDP. `ResponsiveActor` does not expose `setViewportSize` — it was never part of the protocol. Memory note `project_viewport_protocol.md`.

**Protocol layer**: DevTools RDM sizes the viewport via `synchronouslyUpdateRemoteBrowserDimensions` on the browser chrome layer, which is unreachable from RDP (chrome process, not content/parent-process RDP scope). Our workaround is CSS-width simulation. A proper solution would require either a new actor in Firefox or driving the chrome via `chromeContext` eval.

**Sessions**: surfaced during responsive-design iteration; no dogfooding hit it as a blocker yet.

## sources-actor-fallback

**Symptom**: `sources` command falls back to JS-eval enumeration of `document.scripts` rather than using the Source actor / sources walker. iter-61g added the fallback after the Source-actor path was found unreliable in some Firefox versions.

**Protocol layer**: ThreadActor's `sources` method + per-source SourceActor exists but we don't wire it through. Fallback works fine but bypasses the canonical path.

**Sessions**: [[dogfooding-session-48]] #3 (resolved non-issue), tracked in iter-61g.

## summary

| Gap | Severity | Sessions broken | Pure-protocol? |
|---|---|---|---|
| with-network-fallthrough | major | 51, 52, 53 | yes (source selection + state) |
| shadow-dom-piercing | moderate | 52, 53 | no (walker API not called) |
| actor-leak-in-daemon | moderate | — | yes |
| legacy-startlisteners | latent | — | yes |
| viewport-sizing | known limitation | — | yes (RDP scope) |
| sources-actor-fallback | minor | — | yes |

## Closed gaps

The following gaps were closed by the iter-61m..61v stability roadmap and the
iter-61w refresh.  Kept here as a historical record; cross-link from
[[lessons-learned]] where each was originally surfaced.

### full-page-screenshot

closed-in: iter-61v

iter-61r reworked `screenshot --full-page` to call the root-scoped `screenshot`
actor with `fullpage:true, rect, snapshotScale, browsingContextID` (the
4th-positional `fullpage` to `drawSnapshot` is the actual switch).  iter-61v
added the live regression `live_screenshot_full_page_dpr2` asserting
PNG height ≥ `scrollHeight × DPR` on a ≥5000 px synthetic page.

### csp-eval-fallback

closed-in: iter-61r

`evaluateJSAsync` now sends `mapped: { await: true }` on every call.  The
SpiderMonkey `Debugger` API used for awaited evaluation is privileged and
bypasses page CSP entirely, so the dedicated `chromeContext: true` retry is
no longer needed for the CSP case.  See [[lessons-learned#async-eval-doesnt-resolve-promises]]
and [[evaluate-js]].

### headers-source-regression

closed-in: iter-61q

The full WatcherActor engagement work in iter-61q removed the source
downgrade.  `meta.source` now stays `"watcher"` regardless of which optional
fields the caller requests, and `getResponseHeaders` is issued per-entry
against the captured `networkEventActor` IDs.

### navigate-success-on-bad-dns

closed-in: iter-61v

`navigate` is now orchestrated as a multi-actor Command (iter-61r) and gated
on `document-event` resources (iter-61v).  The default daemon path invokes
`neterror_error_for_commit`, inspecting the post-navigate URL and the next
`target-available-form` event; a bad-DNS navigate returns a structured
`error_type: "neterror"` instead of false-success.

### navigate-race-timeout

closed-in: iter-61v

iter-61v's document-event gating replaces the previous `wait_for_commit`
timeout heuristic with a deterministic wait on `dom-loading` /
`dom-interactive` / `dom-complete` resources delivered through the
ResourceCommand bus.  Throttle on the bus was set to zero so a fast
cross-origin navigate cannot race the wait setup.

### locale-pin

closed-in: iter-61w
needs verification

`intl.locale.requested=en-US` plus `LANG=en_US.UTF-8 LC_ALL=en_US.UTF-8`
env-var injection at Firefox launch was identified as the required fix
combination in iter-61l.  We believe the env-var half landed in one of the
iter-61m..61v iterations as part of the broader stability work, but no
specific iteration plan explicitly claims it — needs verification by a live
re-run on a German-locale machine.
