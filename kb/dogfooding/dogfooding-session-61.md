---
title: Dogfooding Session 61 — v0.3.0/FF152 regression sweep, security testbeds, comparis.ch
type: dogfooding
date: 2026-07-18
status: completed
site: www.comparis.ch, demo.testfire.net, public-firing-range.appspot.com, httpbin.org, news.ycombinator.com, example.com
commands_tested: [navigate, tabs, page-text, snapshot, screenshot, dom, geometry, computed, styles, cascade, perf, network, a11y, console, eval, cookies, storage, click, type, scroll, wait, reload, back, forward, inspect, sources, doctor, daemon, responsive]
tags: [dogfooding, v0.3.0, firefox-152, regression-verification, security-audit, comparis, daemon, multi-agent]
---

# Dogfooding Session 61

First dogfood since the v0.3.0 release (session 60 was still 0.2-era, iter-92/93/94). Ran three
agents in parallel — each pinned to its own headless Firefox instance — covering (1) a regression
sweep of the old security-session bugs, (2) a security-scanner workout on the intentionally-vulnerable
testbeds `demo.testfire.net` + `public-firing-range.appspot.com`, and (3) a deep dogfood of the heavy
`www.comparis.ch` SPA. Then a **clean single-instance verification pass** to separate genuine ff-rdp
bugs from artifacts of the multi-instance harness. Headline: **4 of 5 old security-session bugs are
fixed**, ff-rdp is a genuinely strong security-scanning and site-audit tool now, but two core-workflow
bugs regressed — `cookies` lost its StorageActor path (misses httpOnly), and default `navigate` costs
~7 s per call on simple pages under FF152.

## Method note — why a verification pass was needed

To parallelize, I launched three Firefox instances (ports 6000/6001/6002). ff-rdp's daemon registry is
a **single global slot**, so all three shared it and non-owning ports fell back to per-command direct
connections. That confound produced *plausible-but-false* findings in the parallel run (most notably
"`network` degraded to performance-api with null status" on comparis). Every load-bearing finding below
was **re-verified on a clean single Firefox instance** and is tagged CONFIRMED (reproduced clean) or
ARTIFACT (only under the parallel harness). This is exactly the failure mode the skill warns about —
don't report your own test harness as a product bug.

## Regression Checks (sessions 54/55 bugs → v0.3.0 / FF152)

| Bug (v0.2.0 / FF151) | Prev status | Current | Evidence |
|---|---|---|---|
| **screenshot** ("actor not found in FF151 root form") | ❌ broken | ✅ **FIXED** | `screenshot -o` → valid PNG; `--full-page` on HN → 1366×1285 matching `scrollHeight`, verified visually |
| **cascade empty rules** (`{computed:null, rules:[]}`) | ❌ broken | ✅ **FIXED** | `cascade a --prop color` on HN → `computed:"#828282"` + 2 matched rules w/ specificity + `winner:true` |
| **perf vitals LCP contradiction** (`0.0`+`"good"`+"unavailable") | ❌ broken | ✅ **FIXED** | `perf vitals` now → `lcp_ms:null, rating:"unavailable"` + honest note. **But `perf audit` still has the old contradiction — see Bug 4.** |
| **styles --applied over-filter** (returned `[]`) | ❌ broken | ✅ **FIXED** | `styles a --applied` on HN → 2 real rules with properties + matched selectors |
| **cookies missing JS cookies** | ⚠ partial | ❌ **REGRESSED DIFFERENTLY** | Default now surfaces JS-visible cookies (improved), but the StorageActor path went dead → httpOnly missed entirely, flags null. See Bug 1. |

**4 of 5 fixed.** The cookies fix over-rotated: it gained the `document.cookie` merge but lost the actor
enumeration that was the command's entire reason to exist over `document.cookie`.

## Security scanner workout (testbeds)

ff-rdp is a legitimately effective web-security probe. `eval --unwrap` (drive an async `fetch()`
fan-out, return structured JSON) + `--stringify` (defeat actor-grip opacity for objects/arrays) are the
killer primitives — most checks are one-liners. Surfaced on the deliberately-vulnerable testbeds:

1. **Reflected XSS, 5 sink contexts** (firing range) — 1 command; PROBE reflected into body / quoted-attr / `<iframe srcdoc>` / `<head>` / `<noscript>`.
2. **Reflected XSS with confirmed code execution** — 2 commands; `window.XSS_FIRED===true`.
3. **DOM-XSS via `location.hash`** — 4 commands end-to-end (navigate → `dom 'a'` shows `href="javascript:…"` → `click` → assert flag).
4. **Vulnerable library: jQuery 1.8.1** — 2 commands; exposed to CVE-2012-6708, CVE-2015-9251, CVE-2020-11022/11023.
5. **Missing SRI** on the cross-origin jQuery `<script>` (`integrity:null`).
6. **No security headers anywhere** (XFO / CSP / HSTS / X-Content-Type-Options / Referrer-Policy all null) on both testbeds.
7. **testfire `search.jsp` reflects the payload unencoded** into the body.
8. **testfire is plaintext HTTP** — login POSTs creds over `http://`, password field has no `autocomplete=off`, `Server: Apache-Coyote/1.1` leaks an outdated Tomcat connector.

Creative one-liners that worked pleasantly: SRI-gap audit over `document.scripts`, inline-event-handler
inventory (`dom '[onclick]'`), reverse-tabnabbing audit (`a[target=_blank]` sans `rel=noopener`),
password-autocomplete + insecure-form-action audit, mixed-content check.

## comparis.ch site findings (independent of tool state)

**Performance** — first-party delivery is genuinely fast (FCP 156 ms, TTFB 118 ms, HTTP/2), but the page
is tracking-bound: only **10 of 110 requests are first-party**; the rest hit **34 external domains** (the
consent banner discloses up to **777 third parties**). The **5 slowest requests are all trackers**
(clarity.ms 955 ms, doubleclick 934 ms, tiktok 858 ms, bing 813 ms) — each slower than the critical-path
document. 16 requests exceed 500 ms.

**Accessibility** — `<html>` has **no `lang` attribute** (WCAG 3.1.1 A) on a German-language site;
**43 `<header>` → 43 unlabeled `banner` landmarks** (+6 nav) makes landmark nav useless; **no skip-link**
across 212 links; a **white-on-white `h1`** ("Schau auf Comparis", `#fff` on resolved `#fff`, ratio 1.0)
that only reads because it overlays a hero SVG with no CSS fallback. Clean: 0/62 imgs missing alt.

**Security headers** — **no CSP at all** (header or meta), **no X-Frame-Options / frame-ancestors**
(clickjackable), missing Referrer-Policy / Permissions-Policy / COOP-COEP. Present: HSTS
(`max-age=31536000; includeSubDomains`, no preload) + `X-Content-Type-Options: nosniff`. 7 `target=_blank`
links without `rel=noopener`; legacy `p3p` header; Azure-Blob origin fingerprint leaked (`x-ms-*`).

**SEO/structure** — title 36 chars (good), single h1, canonical + 5 hreflang + JSON-LD present, but
**no OpenGraph tags** (weak link previews) and the missing `lang` also hurts language targeting. DOM is
heavy: 2146 nodes, depth 21, 207 inline `<style>` + 208 inline SVG (Emotion CSS-in-JS bloat).

**Console** — 109 errors, but essentially **all third-party tracker noise** (partitioned-cookie /
deprecated `uuid2` cross-site cookies from adnxs/doubleclick); no first-party application errors.

## ff-rdp bugs — CONFIRMED (reproduced clean, single instance)

1. **[MAJOR] `cookies` StorageActor path is dead — only `document.cookie` fallback fires.** Misses
   httpOnly cookies entirely and always returns `secure`/`httpOnly`/`sameSite`/`domain` = `null`;
   `cookies --storage-only` → 0. On httpbin.org, 4 stored cookies (2 httpOnly) → `cookies` returns `[]`;
   on comparis all entries are `source:"document.cookie"` with no flag fields. `--help` promises
   "includes httpOnly, secure, sameSite" — never true. Exit 0 (silently wrong). Guts the security-audit
   use case.
2. **[MAJOR] Default `navigate` costs ~7 s/call on simple pages** — `dom-complete` document-event never
   fires on FF152, so `--wait-strategy both` burns the timeout then falls back to readystate. example.com
   7.26 s, HN 7.14 s; `--no-wait` 0.06 s (page is already loaded). Overhead scales with `--timeout`. Every
   multi-step workflow pays this.
3. **[MODERATE] `navigate` `elapsed_ms` is off by ~7000×** — reports `1`–`4` ms for a ~7 s operation.
   Measures the wrong quantity.
4. **[MODERATE→MAJOR] `perf audit` fabricates a false "good" 0 ms LCP.** On comparis (LCP unavailable),
   `perf vitals` correctly says `lcp_ms:null, rating:"unavailable"`, but `perf audit` on the same page says
   `lcp_ms:0.0, rating:"good"`. `audit` doesn't apply `vitals`' LCP-unavailable logic → dangerous false
   all-clear on the exact pages that most need scrutiny. (regression-agent couldn't repro because its test
   pages had measurable LCP ~587 ms; the bug is specific to the unmeasurable case.)
5. **[MODERATE] `navigate` `committed_url` is always `about:blank` on SPAs.** comparis: `navigated`,
   `ready_state:complete`, and `eval location.href` all confirm the real URL landed, but `committed_url:
   "about:blank"`. A caller trusting `committed_url` thinks navigation failed. Reproduced on 4 comparis routes.
6. **[MODERATE] `network` / `navigate --with-network` returns an inconsistent JSON shape** — an **object**
   `{entries,…}` on busy pages, a bare **array** on quiet ones. `.results.entries` / `.results.total_requests`
   throw `cannot index array` half the time; the documented summary fields are unreachable via `--jq` on the
   array path (and it re-serializes the whole ~110-entry array, ~13 KB, to stdout).
7. **[MODERATE] `a11y contrast --fail-only` top-level `total` reports sampled count, not failures.**
   `total:4` while `results` is `[]` (0 failures) on HN; regression-agent saw `total:500` vs 447 failures.
   Consumers asserting on `total` get a lie. Contrast detection itself is accurate.
8. **[MODERATE] The persistent daemon never starts in this environment.** `daemon_autostart_failed`:
   "spawn died before the registry write / timed out after 5 s"; `daemon status` → `running:false`. Every
   command silently uses a per-command direct connection — fine for stateless commands (network correctly
   used `source:"watcher"` with real methods/status/TLS), but it breaks `inspect` (grips are per-connection)
   and cross-command `--follow`. The warning is also noisy and unreliable: only on some commands
   (`navigate`/`cascade`/`perf`, not `tabs`), only via `--jq` (not `--format text`), and it flip-flops the
   reported registry port (6001↔6002) even on a single instance. There is no `daemon start` subcommand to
   force it up (only `status`/`stop`).
9. **[MINOR] `snapshot --max-chars` is near-no-op** on structure-heavy pages (HN: default / 100 / 5000 all
   ~1742 bytes) — it bounds only accumulated leaf text; `--depth` works correctly.
10. **[MINOR] `dom` `attrs.value` reports the static HTML attribute, not the live value** (range slider
    showed `"0"` while the live value was `3.9`).
11. **[LOW] `responsive` promises a media-query-mismatch warning it never emits** (help says "when `matches`
    is false a warning is attached" — no such key appears).
12. **[LOW] `wait --timeout` prints a deprecation warning** steering to `--timeout-ms`.
13. **[COSMETIC] Malformed `--jq` leaks a raw Rust `Debug` struct** (`Lex(\n [\n (\n Delim("["…`) instead
    of a clean parse-error message. Exit 1 is correct.

## ff-rdp findings — ARTIFACTS (multi-instance harness, NOT real bugs — do not chase)

- `network` "degraded to performance-api, `status`/`method` null, `transfer_size` 0" on comparis → clean
  single-instance run uses `source:"watcher"` with real methods/status. Caused by the shared daemon registry.
- `committed_url` wrong / stale eval reads on *simple* pages under contention (the SPA `about:blank` case,
  Bug 5, is separately real).
- `perf vitals` vs `perf audit` LCP disagreement on *measurable-LCP* pages (the unmeasurable case, Bug 4, is real).
- `eval` "operation timed out after 0ms (phase:recv)" — 0/20 when hammered serially; fresh-connection flake.

## Feature gaps (wishlist)

- **`--insecure` / `--ignore-cert`** for `navigate`/`eval` — bad-cert HTTPS targets (common on staging and in
  security testing) land on Firefox's `about:certerror` interstitial and are completely unscannable today
  (had to fall back to `http://` on testfire).
- **`daemon start`** subcommand + a `--quiet-daemon-warning` (or auto-suppress when the direct fallback
  succeeds), plus render the warning consistently across `--format text`.
- **`ff-rdp doctor --staleness-check`** (carried from session 60) — compare the installed binary's embedded
  SHA against `HEAD`.

## What works well

- **Regression wins are real**: screenshot (basic + full-page), cascade (specificity/winner/origin), styles
  `--applied`, and honest `perf vitals` LCP all landed.
- **Security scanning**: `eval --unwrap` + `--stringify` make almost any DOM/header/library check a one-liner.
- **Interaction on a real React SPA**: `type` correctly invalidates React's value tracker (`50000` → `50'000`),
  `click` drives tab controls (rendered a 62-row amortization table), `wait`/`--wait-for`/`--settle` behave,
  not-found exits 124.
- **`network` (watcher source)** — real statuses, durations, transfer sizes, full headers, and `--security`
  with complete TLS cert detail (TLSv1.3, cipher, issuer, SHA256).
- **`geometry`** overlap/z-index/visibility, **`a11y contrast`** ratio accuracy + actionable selectors,
  **`eval` error handling** (line/column/stack, exit 1), and the **exit-code taxonomy** (1 runtime / 2 usage /
  124 timeout / 7 DNS / 12 denied port) are all solid.

## Summary

- **~35 commands exercised across 3 agents + a clean verification pass.** 4/5 old bugs fixed; **13 confirmed
  issues** (2 major, 5 moderate) + **4 artifacts correctly ruled out**; 3 feature gaps.
- **Top 2 to fix**: `cookies` StorageActor (Bug 1) and the ~7 s default-`navigate` penalty (Bug 2) — both
  core-workflow, both CONFIRMED, both silently exit 0. **`perf audit`'s false "good" LCP** (Bug 4) is the
  most dangerous *audit* failure mode.
- **Key process takeaway**: parallelizing across multiple Firefox instances is fast but the single-slot daemon
  registry manufactures false "silent degradation" findings — always verify tool bugs on one clean instance
  before filing. A corrected memory ([[project_actor_silent_degradation]]) captures which half was real.

## References

- [[dogfooding-session-60]]
- [[dogfooding-session-55]] · [[dogfooding-session-54]] (prior security-testbed sessions)
- [[iteration-115-cascade-rule-actor-id]] · [[iteration-116-console-cache-start-listeners]]
