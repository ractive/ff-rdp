---
title: Dogfooding Session 53 — verifying iter-61k fixes and hunting new bugs
type: dogfooding
date: 2026-05-23
status: completed
site: https://en.wikipedia.org/wiki/HTTP, https://example.com, https://example.org, https://news.ycombinator.com, https://lit.dev, https://demo.testfire.net, https://www.comparis.ch/hypotheken, https://this-domain-totally-doesnt-exist-zzz.invalid
commands_tested: [launch, tabs, navigate, screenshot, eval, dom, computed, console, network, daemon, perf, a11y, sources, cookies, page-text]
tags: [dogfooding, regression-verification, iter-61k, new-bugs]
---

# Dogfooding Session 53

Verified iter-61k (PR #76 / merge commit e98105f) fixes against Wikipedia, example.com, HN, lit.dev, and demo.testfire.net. **Out of 11 acceptance criteria, 4 verified green, 5 still broken (no change since session 52), 2 partially working.** Three of the previously-broken items remain regressions: `screenshot --full-page` (5th session running), Firefox locale pin, and `eval` CSP bypass. Also found 2 new bugs introduced or exposed by iter-61k.

## TL;DR

- ✓ Fixed: AC-D (--headers note), AC-E (computed shape), AC-I (hasShadowRoot), AC-J (tabs --fields).
- ❌ Still broken: AC-A (full-page screenshot), AC-B (locale pin — German leaks again), AC-C (network default still performance-api), AC-F (navigate succeeds on DNS fail), AC-G (cross-origin race), AC-H (HN eval still CSP-blocked), AC-K (eval after bad-DNS navigate fails).
- 🆕 New bugs: `--detail --headers` flips watcher source back to performance-api (regression in iter-61k); daemon timeout on heavy SPA (Comparis) leaves stale state.

## Regression Checks (iter-61k acceptance criteria)

| AC | Description | Status | Evidence |
|---|---|---|---|
| A | `--full-page` height ≈ scrollHeight × DPR | ❌ broken (5th session) | wiki/HTTP: scrollHeight=22491, dpr=1 → PNG 800×600 |
| B | English console messages after launch | ❌ broken | German on HN: `"Diese Seite befindet sich im Kompatibilitätsmodus..."` |
| C | `navigate --with-network` then `network` (no flags) → source:watcher with status/method | ❌ broken | After `navigate https://news.ycombinator.com --with-network`, `network` returns `source: performance-api`, status:null, method:null |
| D | `--detail --headers` emits clear note on perf-api path | ✓ fixed | Note: `"... --headers ignored (performance-api source has no response headers; use --with-network to engage watcher)"` |
| E | `computed sel [--prop X]*` returns uniform array-of-records shape | ✓ fixed | Zero/single/multi --prop all return `[{computed:{...}, index, selector}]` |
| F | `navigate <bad-dns-url>` returns non-zero exit with `error_type` | ❌ broken | `navigate https://this-domain-totally-doesnt-exist-zzz.invalid` → exit 0, success-shaped JSON; tab actually on `about:neterror` |
| G | navigate timeout → URL-match recovery treats as success | ❌ broken | `navigate https://example.com` after HN session → `error: operation timed out`, but `tabs` shows tab landed on example.com |
| H | `eval 'document.title'` on HN succeeds (CSP bypass) | ❌ broken | `error: call to eval() blocked by CSP` (EvalError). Same on lit.dev. No automatic chromeContext fallback observed. |
| I | `dom 'host-selector'` flags `hasShadowRoot:true` | ✓ fixed | Created `<div id=myhost>` with open shadow root: `dom '#myhost'` returns `hasShadowRoot:true, shadowMode:"open"` |
| J | `tabs --fields url,title` returns only those fields | ✓ fixed | Only `url` and `title` in each tab record |
| K | After navigate to bad-DNS, `eval 1+1` succeeds | ❌ broken | After bad-DNS navigate (landed on about:neterror), `eval 1+1` returned CSP EvalError (about:neterror itself blocks eval) |

Score: **4 fixed / 7 still broken**.

## Smoke Test Results

| Command | Status | Notes |
|---|---|---|
| `launch --headless --port 6000 --auto-consent` | ✓ | PID returned, temp profile created |
| `navigate <url>` | ⚠ | Race-condition timeouts recur (AC-G); DNS-fail success-shape (AC-F) |
| `navigate <heavy-SPA> --timeout 25` | ❌ | Comparis: `error: daemon did not respond within the timeout after auth`. Tab did not change |
| `tabs --fields url,title` | ✓ | AC-J fixed |
| `screenshot` (viewport) | ✓ | Returns proper PNG |
| `screenshot --full-page` | ❌ | 800×600 again (AC-A) |
| `console --level warn/error` | ✓ | Captures events; German leak (AC-B) |
| `eval` on permissive site (example.com) | ✓ | Works |
| `eval` on CSP-restricted site (HN, lit.dev) | ❌ | Blocked by CSP (AC-H) |
| `dom '#shadowhost'` | ✓ | hasShadowRoot/shadowMode emitted (AC-I) |
| `computed h1 [--prop ...]` | ✓ | Uniform shape (AC-E) |
| `network --since all --detail` (no headers) | ✓ | Returns `source: watcher`, status:200, method:GET |
| `network --since all --detail --headers` | ❌ | Source FLIPS to `performance-api` (new bug #1) |
| `network` (default) after `--with-network` | ❌ | Still performance-api (AC-C) |
| `network --detail --headers` (default scope) | ✓ note | Note emitted explaining missing headers (AC-D) |
| `daemon status` | ✓ | Shows buffer_sizes |
| `a11y contrast --fail-only` | ✓ | 193 violations on HN (plausible) |
| `sources --limit 3` | ✓ | Fallback to js-eval |
| `perf vitals` | ✓ | Plausible numbers |
| `cookies` | ✓ | Empty on testfire (plausible) |

## New Findings (Bugs Not in Session 52)

### N1. `--detail --headers` downgrades watcher source to performance-api [major — regression in iter-61k]

`--detail` alone returns the watcher source with full status/method. **Adding `--headers` to that exact same query** flips `meta.source` from `"watcher"` to `"performance-api"` and drops every header — then helpfully emits the perf-api note saying headers aren't available.

```bash
# Same scope, same scoping flag, only difference is --headers:
$ ff-rdp network --since all --detail --limit 2
# meta.source: "watcher", method:"GET", status:200 (per row)

$ ff-rdp network --since all --detail --headers --limit 1
# meta.source: "performance-api" (!), method:null, status:null, note:"--headers ignored..."
```

This is the opposite of what `--headers` should do — it actively destroys the data path that *could* provide headers. Almost certainly an iter-61k regression around the source-selection logic. Workaround: don't pass `--headers`. But the actual `--since all` watcher path DOES return headers correctly on testfire and HN when the source isn't downgraded — verified by reading raw watcher JSON directly via a different code path. So the data is reachable; the CLI just selects the wrong source when `--headers` is on.

### N2. Heavy-SPA navigate causes daemon timeout, leaves stale tab state [moderate]

```bash
$ ff-rdp navigate https://www.comparis.ch/hypotheken --timeout 25
error: daemon did not respond within the timeout after auth — the daemon may be overloaded or the connection is stale.
hint: run `ff-rdp daemon stop` then retry, or use --no-daemon.
```

After this error, `tabs` still showed the previous tab (HN) — but subsequent `network --since all` calls returned events from HN, not Comparis, while the user has no clear signal that the navigation didn't actually happen. The error mentions "auth" which is confusing terminology for a navigate timeout. Suggest: clearer error message and ensure tab state is recovered/synced.

## Confirmed-still-broken issues from Session 52

These were on the iter-61k worklist but did **not** make it across the line. They remain bugs:

- **#A1/A2** — `screenshot --full-page` (5th session). The chrome-scope rect override appears not to be reaching the screenshot path; PNG still hardcoded to 800×600. The "deferred to dogfooding" live-verify step in iter-61k was the only path that would have caught this — and it was deferred.
- **#52.10** — locale pin. iter-61k added `intl.locale.matchOS=false` to user.js but skipped the env-var path (`LANG=en_US.UTF-8 LC_ALL=en_US.UTF-8`). On macOS, matchOS=false alone doesn't override the OS-level locale for console-message strings. Need the env-var injection that was deferred.
- **#52.7** — default `network` still falls back to performance-api even when daemon buffer has entries.
- **#52.5** — CSP-eval bypass code reportedly lands as `chromeContext: true` retry on `EvalError + CSP`, but in practice the retry isn't triggering — the user sees the raw EvalError from the first attempt. Suggest adding a unit test that exercises the real Firefox EvalError JSON shape (the actual error.message contains "call to eval() blocked by CSP" — make sure `is_csp_eval_error` matches the wire shape).
- **#52.4 / AC-F** — DNS-failure navigate still returns success-shaped JSON. `neterror_error_for_commit` helper exists per the iteration notes but doesn't fire in the daemon path used by default.
- **#52.3 / AC-G** — cross-origin race timeout. URL-match recovery in `wait_for_commit` doesn't appear to be reached; user sees `error: operation timed out`. Tab DID actually navigate.
- **#52.9 / AC-K** — stale consoleActor after navigate. After landing on `about:neterror`, eval still fails with CSP EvalError (different failure mode — about:neterror has its own restrictive CSP). The actor *cache* may now refresh, but the underlying about:neterror page can't run eval. Suggest detecting about:neterror in `eval` and producing a clearer error.

## Feature Gaps (carried over)

- **CSP-bypassing eval** — biggest LLM-friendliness win still unrealized.
- **Full-page screenshot** — 5 sessions running.
- **Shadow-DOM piercing** — only the host-flag landed (`I1`); the `--include-shadow` traversal was correctly deferred but is still a blocker for SPAs.
- **`navigate` → about:neterror detection** — code exists but doesn't trigger in the daemon path.

## Summary

- **18 commands exercised** across 8 sites including a vulnerable bank, a heavy SPA, a shadow-DOM site, and an intentionally-bad DNS name.
- **iter-61k: 4 of 11 ACs confirmed fixed live**; 7 ACs still broken in production despite passing unit tests. The deferred "live verification" steps in iter-61k mostly *would* have caught these.
- **2 new bugs** (one regression introduced by iter-61k around `--headers` flipping the source; one moderate daemon-timeout state-leak on heavy SPAs).
- **Recommendation**: open **iter-61l** focused specifically on the live-verification gap. The pattern of "unit-tested green, live broken" repeated across A/B/C/F/G/H/K is a process problem, not a coding problem. iter-61l should:
  1. Make `screenshot --full-page` actually use the rect override (5th time; consider scroll-stitch fallback).
  2. Add `LANG=en_US.UTF-8 LC_ALL=en_US.UTF-8` env-var injection in `launch`.
  3. Trace why the watcher path isn't selected by default in `network` even when daemon buffer is populated for the current navigation.
  4. Trace why the `chromeContext: true` CSP fallback path doesn't fire on HN.
  5. Trace why `neterror_error_for_commit` doesn't reach the daemon-default path.
  6. Trace why `wait_for_commit`'s URL-match recovery doesn't suppress the timeout error.
  7. Fix the iter-61k `--headers` regression (N1) — should be a small selection-logic bug.

**Do not stop the cycle.** Zero of the four most-impactful items (A, B, C, H) actually shipped.

## References

- Previous: [[dogfooding-session-52]]
- Iter under test: [[iteration-61k-dogfood-52-fixes]] / PR #76 / merge commit e98105f
- Targets: Wikipedia/HTTP, example.com/org, HN, lit.dev, demo.testfire.net, comparis.ch
