---
title: Open Protocol-Level Gaps in ff-rdp
type: rdp-note
tags: [rdp, from-codebase, gaps]
date: 2026-05-23
---

# Open Protocol-Level Gaps

Catalog of known RDP-layer gaps as of 2026-05-23, drawn from dogfooding sessions 48–53 and iterations 61g–61l. Each item: symptom, where the gap lives in the protocol, suggested investigation. Excludes UX-only issues — see the dogfooding session notes for the full list.

## full-page-screenshot

**Symptom**: `screenshot --full-page` produces a viewport-sized PNG (800×600 or 1366×683) regardless of `document.documentElement.scrollHeight`. On a Wikipedia article with scrollHeight=22 491 px, the captured PNG is 600–683 px tall.

**Protocol layer**: The two-step protocol (`screenshotContentActor.prepareCapture` → `screenshotActor.capture`) accepts a `rect` argument that should override the default viewport bounds, plus `fullpage: true`. We send both (see `screenshot.rs:49-80`) but the resulting PNG is viewport-only. iter-61h's chrome-scope fallback fixed *headless* screenshots in general but the rect override apparently doesn't reach `browsingContext.drawSnapshot`.

**Sessions**: [[dogfooding-session-48]] #2, [[dogfooding-session-49]] #3, [[dogfooding-session-51]] #3, [[dogfooding-session-52]] regression table, [[dogfooding-session-53]] AC-A. **Five consecutive sessions broken.**

**Suggested fix**: trace whether the `rect` actually arrives at `drawSnapshot` server-side; if Firefox 149+ has changed the chrome-scope API, fall back to a scroll-and-stitch JS-eval composite.

## csp-eval-fallback

**Symptom**: `eval` on CSP-restricted sites (Hacker News, lit.dev) returns `EvalError: call to eval() blocked by CSP` even though `evaluate_js_async_chrome` (with `chromeContext: true`) is implemented as a CSP-bypass retry.

**Protocol layer**: The first attempt via `evaluateJSAsync` (no flag) is correctly blocked. The retry path with `chromeContext: true` should bypass page CSP because chrome JS isn't subject to `script-src`. Unit tests pass; live the retry isn't triggering.

**Sessions**: [[dogfooding-session-52]] design issue #5, [[dogfooding-session-53]] AC-H, AC-K.

**Suggested fix**: add a test that exercises the *real* Firefox `EvalError` wire shape (the exception object's `preview.message` contains the exact string `"call to eval() blocked by CSP"`); verify `is_csp_eval_error` matches it; ensure the retry actually runs in the daemon code path, not just the direct CLI path.

## with-network-fallthrough

**Symptom**: `navigate --with-network` engages the WatcherActor and inline-returns proper `{source: "watcher", status: 200, method: "GET", transfer_size: ...}`. The *next* standalone `network` call falls back to `source: performance-api` with `status: null, method: null` — even though `daemon status` shows `buffer_sizes: {network-event: 209}` (data IS captured).

**Protocol layer**: The watcher subscription appears to be torn down (or its data made unreachable) between the navigate response and the next CLI invocation. The buffer exists but the `network` command's source-selection logic picks performance-api.

**Sessions**: [[dogfooding-session-51]] #5, [[dogfooding-session-52]] AC-C, [[dogfooding-session-53]] AC-C.

**Effect**: response headers (CSP, HSTS, X-Frame-Options, Set-Cookie attributes) are completely unreachable in the security-audit workflow that motivated session 51. This is the single biggest protocol-level gap for the security-audit use case.

## headers-source-regression

**Symptom**: `network --since all --detail` alone correctly returns `source: watcher`. Adding `--headers` to the *same query* flips `meta.source` back to `performance-api` and drops every header.

**Protocol layer**: Pure source-selection logic bug introduced in iter-61k. The watcher path actually returns headers correctly when not downgraded.

**Sessions**: [[dogfooding-session-53]] N1.

**Suggested fix**: small CLI logic fix; the protocol path is intact.

## shadow-dom-piercing

**Symptom**: `dom 'selector'` now correctly flags `hasShadowRoot: true` / `shadowMode: "open"` on host nodes (iter-61k) but does not traverse *into* the shadow root. SPAs that use shadow DOM heavily (Lit, web components) are opaque past the host.

**Protocol layer**: WalkerActor has shadow-DOM traversal support; we just don't call it. Need `--include-shadow` flag plumbed through.

**Sessions**: [[dogfooding-session-52]] gap #6, [[dogfooding-session-53]] feature gaps.

## actor-leak-in-daemon

**Symptom**: Each `evaluateJSAsync` returning an object/longString allocates server-side actor IDs that are never released in long-running daemons. iter-54 task 4 landed `ObjectActor::release` + `ScopedGrip` wrapper as building blocks but didn't wire them into daemon-mode call sites or add a soak test.

**Protocol layer**: We never send `release` to grip actors. Firefox's per-connection actor pool grows without bound.

**Sessions**: surfaced in [[iteration-54-protocol-correctness]] task 4 (deferred sub-tasks 2 & 3); no dogfooding session has reproduced an OOM yet but a 1000-eval soak test was planned.

## navigate-success-on-bad-dns

**Symptom**: `navigate https://this-does-not-exist.invalid` exits 0 with success-shaped JSON. The tab actually landed on `about:neterror`.

**Protocol layer**: `navigateTo` returns success because Firefox *did* navigate. We need to inspect the post-navigate URL or watch the next `target-available-form` event for an `about:neterror` URL and translate that into an error.

**Sessions**: [[dogfooding-session-52]] #4, [[dogfooding-session-53]] AC-F. Helper `neterror_error_for_commit` exists but isn't invoked on the default daemon path.

## navigate-race-timeout

**Symptom**: Fast cross-origin navigates (HN → example.com) sometimes return `error: operation timed out` while the tab actually navigated successfully — the commit event arrives before the wait setup.

**Protocol layer**: We need a URL-match recovery in `wait_for_commit`: on timeout, check current URL; if it equals target, return success.

**Sessions**: [[dogfooding-session-52]] #3, [[dogfooding-session-53]] AC-G.

## locale-pin

**Symptom**: Console messages still come back in German on a German-locale macOS, despite `intl.locale.requested=en-US` in the launched profile's `user.js`.

**Protocol layer**: Not really protocol — but adjacent. RDP doesn't expose a locale-override actor. `LANG=en_US.UTF-8 LC_ALL=en_US.UTF-8` env-var injection at launch is required in addition to the pref.

**Sessions**: [[dogfooding-session-51]] #10, [[dogfooding-session-52]] AC, [[dogfooding-session-53]] AC-B.

## legacy-startlisteners-coexistence

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
| full-page-screenshot | major | 48, 49, 51, 52, 53 | yes (Firefox API shift) |
| csp-eval-fallback | major | 52, 53 | no (CLI wiring) |
| with-network-fallthrough | major | 51, 52, 53 | yes (source selection + state) |
| headers-source-regression | major | 53 | no (CLI logic) |
| shadow-dom-piercing | moderate | 52, 53 | no (walker API not called) |
| actor-leak-in-daemon | moderate | — | yes |
| navigate-success-on-bad-dns | moderate | 52, 53 | no (post-nav inspection) |
| navigate-race-timeout | moderate | 52, 53 | no (CLI wait logic) |
| locale-pin | minor (LLM-blocker) | 51, 52, 53 | no (launch env) |
| legacy-startlisteners | latent | — | yes |
| viewport-sizing | known limitation | — | yes (RDP scope) |
| sources-actor-fallback | minor | — | yes |
