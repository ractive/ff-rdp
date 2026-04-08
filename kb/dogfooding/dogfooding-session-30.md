---
title: "Dogfooding Session 30: Re-test in Daemon Mode (Post Iteration 30+31 Fixes)"
date: 2026-04-08
tags: [dogfooding, testing, ux, bugs, regression, daemon]
status: complete
firefox_version: "149.0"
tool_version: ff-rdp 0.1.0
---

# Dogfooding Session 30: ff-rdp on comparis.ch (Daemon Mode Re-run)

Systematic re-test of all ff-rdp commands against comparis.ch using headless Firefox 149. Originally run with `--no-daemon`, then re-run in default daemon mode to compare behavior.

## Setup Notes

- Firefox 149 headless running on port 6000 with Consent-O-Matic extension installed.
- Auto-consent extension visible in tabs (`moz-extension://...options.html`).
- `window.cmp_noscreen = true` still used as belt-and-suspenders consent suppression.
- **Daemon mode** (default) used for all commands in the re-run.

## Command Results Summary

### Commands That Worked Perfectly

| # | Command | Notes |
|---|---------|-------|
| 1 | `navigate https://www.comparis.ch` | Fast, reliable. Same as no-daemon. |
| 2 | `tabs` | Lists 2 tabs (comparis + Consent-O-Matic settings). Same as no-daemon. |
| 3 | `perf vitals` | Clean output with ratings. lcp_ms shows 0.0 with explanatory note. tbt_ms=0.0. |
| 4 | `perf vitals --format text` | Clean key-value output. Same as no-daemon. |
| 5 | `perf vitals --jq '.results.fcp_ms'` | Returns `104.0`. Works perfectly. |
| 6 | `perf audit` | Comprehensive audit data with vitals, navigation, resources, DOM stats. |
| 7 | `perf audit --format text` | Structured text with sections for vitals, issues, resources, DOM stats. |
| 8 | `perf compare <url1> <url2>` | Works. Navigates and compares. lcp_ms still null in compare mode. |
| 9 | `dom stats` | 2071 nodes, 627KB document. |
| 10 | `dom stats --format text` | Clean key-value output. |
| 11 | `dom "h1" --text` | Returns "Vergleichen mit Comparis." |
| 12 | `dom "h1" --count` | Returns count=1. |
| 13 | `dom "a" --attrs --limit 5` | Returns 5 of 204 links with attributes. Hint message helpful. |
| 14 | `dom tree --depth 2` | Native WalkerActor tree. Works. |
| 15 | `snapshot --depth 3 --max-chars 500` | Good structured snapshot for LLM consumption. |
| 16 | `snapshot --format text` | Beautiful indented tree with semantic info, interactive markers, text content. |
| 17 | `page-text` | Full page text extraction. Useful. |
| 18 | `eval 'document.title'` | Returns page title. |
| 19 | `eval 'window.cmp_noscreen'` | Returns `true`. |
| 20 | `click "a[href]"` | Response flattened: `{"clicked": true, "tag": "A", "text": ""}`. |
| 21 | `type "input" "test@example.com"` | Works but response still shows raw actor/grip info (not flattened like click). |
| 22 | `wait --selector "body"` | Instant match, 0ms. |
| 23 | `wait --text "Comparis"` | Matched in 1ms. |
| 24 | `cookies` | Full cookie details including sameSite, httpOnly. |
| 25 | `storage local` | Returns localStorage entries. |
| 26 | `storage localStorage` | Alias accepted, returns same data as `storage local`. |
| 27 | `storage session` | Returns sessionStorage entries. |
| 28 | `storage sessionStorage` | Alias accepted. |
| 29 | `geometry "header" --visible-only` | Reduces elements to visible-only subset. Works correctly. |
| 30 | `geometry "header"` | Returns headers (20 shown by default with hint). |
| 31 | `styles "h1"` | Full computed styles returned. |
| 34 | `a11y contrast` | JS-based contrast check works. 7 elements checked, all pass AA. |
| 38 | `navigate --with-network https://www.comparis.ch/hypotheken` | Works. Captured 196 requests. |
| 41 | `reload` | Instant. |
| 42 | `back` | Works. |
| 43 | `forward` | Works. |
| 45 | `inspect <actor>` | **NOW WORKS in daemon mode.** Grip actor ID persists across commands. Returns full object properties and prototype chain. |
| 46 | `recipes` | Curated jq one-liners. |
| 47 | `llm-help` | Complete CLI reference. Excellent. |
| 48 | `perf vitals` (after navigation) | tbt_ms=0.0 (not -0.0). |
| 49 | `network --follow` | **NOW WORKS in daemon mode.** Streams NDJSON network events in real time during navigation. Captured extensive event data. |

### Commands That Failed or Had Issues

| # | Command | Error | Severity | Changed from no-daemon? |
|---|---------|-------|----------|------------------------|
| 33 | `a11y --depth 3` | `getRootNode` unrecognized by Firefox 149 AccessibilityWalker | **HIGH** | Same. Firefox 149 compat. |
| 35 | `screenshot` | `captureScreenshot` unrecognized by Firefox 149 | **HIGH** | Same. Firefox 149 compat. |
| 36 | `sources` | `undefined passed where a value is required` | **MEDIUM** | Same. Firefox 149 compat. |
| 37 | `network --limit 20` | Returns 0 results (no Performance API fallback in daemon mode) | **MEDIUM** | **REGRESSION** — was Pass in no-daemon (Performance API fallback). |
| 39 | `navigate --with-network --network-timeout 5` | Only 1 beacon request captured, `total_transfer_bytes: -0.0` | **MEDIUM** | **WORSE** — no-daemon captured more requests in the 5s window. |
| 44 | `console --follow` | No output even after generating console.log via eval | **MEDIUM** | Same. Still broken. |

### UX Friction Points

1. **`type` response not flattened** — `click` response was flattened to `{"clicked": true, "tag": "A", "text": "..."}` but `type` still returns raw actor/grip info with `preview.ownProperties`. Should be flattened like click: `{"typed": true, "value": "test@example.com"}`.

2. **`responsive` does not actually resize viewport** — Requested widths 320 and 1024 both show viewport.width=1366 and identical element rects. The viewport resize is not taking effect. Same in both daemon and no-daemon modes.

3. **`navigate --with-network --network-timeout 5` captures far fewer requests in daemon mode** — Only 1 beacon request vs. many more in no-daemon. The WatcherActor event buffer in daemon mode seems slower to populate, or events arrive after the timeout window closes. Also shows `total_transfer_bytes: -0.0`.

4. **`network --limit 20` returns 0 results in daemon mode** — The Performance API fallback only triggers in `--no-daemon` mode. In daemon mode, `network` relies solely on the WatcherActor buffer, which has no events if no `--follow` or `navigate --with-network` was recently used. This is a significant usability gap.

5. **`lcp_ms` always 0.0 or null** — In `perf vitals`, lcp_ms is 0.0 with a note about DOM approximation. In `perf compare`, lcp_ms is null. Neither provides a useful LCP value.

6. **`perf compare` lcp_ms inconsistency** — `perf vitals` returns `lcp_ms: 0.0` with `lcp_approximate: true`, but `perf compare` returns `lcp_ms: null`. These should be consistent.

7. **`console --follow` still produces no output** — Even after generating console.log/warn/error messages via eval in parallel daemon commands, the --follow stream captures nothing. Same in both modes.

8. **Transient `listTabs` error** — Observed one `invalid packet: listTabs response missing 'tabs' field` error when running eval immediately after navigate. Subsequent retry succeeded. May be a race condition in daemon connection reuse after navigation.

### Missing Features / Suggestions

1. **Flatten `type` response** — Like `click` was flattened, `type` should return `{"typed": true, "value": "..."}`.
2. **Fix responsive viewport resize** — The responsive command needs to actually resize the viewport to each requested width.
3. **Enable Performance API fallback in daemon mode** — `network --limit 20` should fall back to Performance API in daemon mode too, not just in `--no-daemon`.
4. **LCP fallback** — Consider using `largest-contentful-paint` Performance entries or a mutation-observer-based approach for LCP estimation in headless mode.
5. **Screenshot fallback for Firefox 149** — Investigate `browsingContext.captureScreenshot` (WebDriver BiDi) or canvas-based screenshot as a fallback.
6. **Sources fallback** — Consider using `threadActor.sources()` or a JS-based approach to list loaded scripts.

### Performance Assessment

- All working commands are fast: most complete in under 100ms.
- `navigate --with-network` without `--network-timeout` works well (captured 196 requests).
- `navigate --with-network --network-timeout 5` captures far fewer requests in daemon mode vs no-daemon.
- `perf compare` with 2 URLs completed quickly.
- `network --follow` works excellently in daemon mode — streams NDJSON events in real time.

### Firefox 149 Compatibility

Three RDP actor types remain broken in Firefox 149 (same as previous sessions):
1. `accessiblewalker` — `getRootNode` unrecognized
2. `screenshotContentActor` — `captureScreenshot` unrecognized
3. `sources` — returns undefined error

**Improvement:** All three have good error messages that explain the incompatibility and suggest workarounds.

## Regression Check

Explicitly checking each issue from session 29:

| # | Session 29 Issue | Status | Notes |
|---|-----------------|--------|-------|
| 1 | `storage localStorage` / `sessionStorage` not accepted | **FIXED** | Both aliases work. Same in daemon. |
| 2 | `--format text` on `perf audit` outputs JSON | **FIXED** | Beautiful structured text. Same in daemon. |
| 3 | `snapshot --format text` outputs JSON | **FIXED** | Indented tree format. Same in daemon. |
| 4 | `click` returns raw actor/preview objects | **FIXED** | Flattened response. Same in daemon. |
| 5 | `tbt_ms: -0.0` | **FIXED** | Now shows 0.0 consistently. Same in daemon. |
| 6 | `lcp_ms: null` always | **IMPROVED** | Now shows 0.0 with note in vitals; still null in compare. Same in daemon. |
| 7 | `network --limit 20` returns 0 results | **FIXED in no-daemon / REGRESSED in daemon** | Performance API fallback only works in no-daemon mode. |
| 8 | `navigate --with-network` takes 17 seconds | **FIXED** | `--network-timeout` flag works. Daemon captures fewer requests though. |
| 9 | `geometry "header"` returns 43 matches, no way to filter | **FIXED** | `--visible-only` works. Same in daemon. |
| 10 | `a11y --depth 3` broken in Firefox 149 | **NOT FIXED** | Same error. Firefox compat. |
| 11 | `screenshot` broken in Firefox 149 | **NOT FIXED** | Same error. Firefox compat. |
| 12 | `sources` broken in Firefox 149 | **NOT FIXED** | Same error. Firefox compat. |
| 13 | `console --follow` no output | **NOT FIXED** | Still produces no output in both modes. |
| 14 | `inspect` fails with --no-daemon | **FIXED in daemon mode** | Works perfectly — grip actor IDs persist across daemon commands. |
| 15 | `--fields` flag has no visible effect | **NOT TESTED** | Not re-verified. |

**Summary:** 9 of 15 issues fixed or improved in no-daemon. In daemon mode: 10 of 15 fixed/improved (inspect now works), but network --limit 20 regresses (no Performance API fallback).

## Daemon vs No-Daemon Differences

| Command | No-Daemon | Daemon | Winner |
|---------|-----------|--------|--------|
| `inspect <actor>` | **FAIL** — grip actor expires when connection closes | **PASS** — grip persists across commands | Daemon |
| `network --follow` | Not tested (macOS lacks timeout) | **PASS** — streams NDJSON events in real time | Daemon |
| `network --limit 20` | **PASS** — Performance API fallback returns ~96 entries | **FAIL** — returns 0 (no fallback) | No-Daemon |
| `navigate --with-network --network-timeout 5` | **PASS** — captures many requests | **PARTIAL** — only 1 beacon request captured | No-Daemon |
| `navigate --with-network` (no timeout) | **PASS** — 166 requests | **PASS** — 196 requests | Daemon (more) |
| `console --follow` | **FAIL** — no output | **FAIL** — no output | Same |
| `responsive` | **FAIL** — viewport not resized | **FAIL** — viewport not resized | Same |
| All other commands | Pass | Pass | Same |

### Key Takeaways

1. **Daemon mode enables `inspect`** — the persistent connection keeps grip actor IDs valid across commands. This was the primary motivation for daemon mode and it works.
2. **Daemon mode enables `network --follow`** — real-time NDJSON streaming of network events works well.
3. **Daemon mode breaks `network --limit 20`** — the Performance API fallback does not trigger in daemon mode, so `network` returns 0 results unless `--follow` or `navigate --with-network` is used. This should be fixed.
4. **`navigate --with-network --network-timeout 5` captures fewer events in daemon mode** — the short timeout misses most WatcherActor events. Without `--network-timeout`, daemon captures slightly more requests than no-daemon.
5. **`console --follow` is broken in both modes** — this is likely a Firefox 149 protocol issue, not a daemon/no-daemon issue.

## Test Matrix

| Command | Tested | Daemon Result | No-Daemon Result | Changed? |
|---------|--------|---------------|------------------|----------|
| navigate | Yes | Pass | Pass | Same |
| tabs | Yes | Pass | Pass | Same |
| perf vitals | Yes | Pass | Pass | Same |
| perf vitals --format text | Yes | Pass | Pass | Same |
| perf vitals --jq | Yes | Pass | Pass | Same |
| perf audit | Yes | Pass | Pass | Same |
| perf audit --format text | Yes | Pass | Pass | Same |
| perf compare | Yes | Pass | Pass | Same |
| dom stats | Yes | Pass | Pass | Same |
| dom stats --format text | Yes | Pass | Pass | Same |
| dom selector --text | Yes | Pass | Pass | Same |
| dom selector --count | Yes | Pass | Pass | Same |
| dom selector --attrs | Yes | Pass | Pass | Same |
| dom tree | Yes | Pass | Pass | Same |
| snapshot | Yes | Pass | Pass | Same |
| snapshot --format text | Yes | Pass | Pass | Same |
| page-text | Yes | Pass | Pass | Same |
| eval | Yes | Pass | Pass | Same |
| click | Yes | Pass | Pass | Same |
| type | Yes | Pass | Pass | Same |
| wait --selector | Yes | Pass | Pass | Same |
| wait --text | Yes | Pass | Pass | Same |
| cookies | Yes | Pass | Pass | Same |
| storage local | Yes | Pass | Pass | Same |
| storage localStorage | Yes | Pass | Pass | Same |
| storage session | Yes | Pass | Pass | Same |
| storage sessionStorage | Yes | Pass | Pass | Same |
| geometry --visible-only | Yes | Pass | Pass | Same |
| geometry | Yes | Pass | Pass | Same |
| styles | Yes | Pass | Pass | Same |
| responsive | Yes | **Fail** | **Fail** | Same |
| a11y --depth 3 | Yes | **Fail** | **Fail** | Same |
| a11y contrast | Yes | Pass | Pass | Same |
| screenshot | Yes | **Fail** | **Fail** | Same |
| sources | Yes | **Fail** | **Fail** | Same |
| network --limit 20 | Yes | **Fail** | Pass | **Daemon worse** |
| navigate --with-network | Yes | Pass | Pass | Same |
| navigate --network-timeout 5 | Yes | **Partial** | Pass | **Daemon worse** |
| network --follow | Yes | **Pass** | Skipped | **Daemon better** |
| reload | Yes | Pass | Pass | Same |
| back | Yes | Pass | Pass | Same |
| forward | Yes | Pass | Pass | Same |
| console --follow | Yes | **Fail** | **Fail** | Same |
| inspect | Yes | **Pass** | **Fail** | **Daemon better** |
| recipes | Yes | Pass | Pass | Same |
| llm-help | Yes | Pass | Pass | Same |
| perf vitals (post-nav) | Yes | Pass | Pass | Same |
