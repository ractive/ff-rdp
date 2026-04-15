---
title: "Dogfooding Session 31: Post Iteration 32 Fixes"
date: 2026-04-08
tags:
  - dogfooding
  - testing
  - ux
  - regression
  - daemon
status: completed
firefox_version: "149.0"
tool_version: ff-rdp 0.1.0
---

# Dogfooding Session 31: ff-rdp on comparis.ch (Daemon Mode)

Systematic test of all ff-rdp commands against comparis.ch using headless Firefox 149 in daemon mode (default). This session follows iteration 32 fixes.

## Setup Notes

- Firefox 149 headless running on port 6000 with Consent-O-Matic extension.
- Daemon mode (default) used for all commands.
- `window.cmp_noscreen = true` for belt-and-suspenders consent suppression.

## Command Results Summary

### Commands That Worked Perfectly

| # | Command | Notes |
|---|---------|-------|
| 1 | `navigate https://www.comparis.ch` | Fast, reliable. |
| 2 | `tabs` | Lists 2 tabs (comparis + Consent-O-Matic settings). |
| 3 | `perf vitals` | Clean output. lcp_ms=0.0 with explanatory note. tbt_ms=0.0. fcp_ms=447.0. |
| 4 | `perf vitals --format text` | Clean key-value output. |
| 5 | `perf vitals --jq '.results.fcp_ms'` | Returns `447.0`. |
| 6 | `perf vitals --fields fcp_ms,ttfb_ms` | **NOW WORKS.** Returns only requested fields. Also works with `--format text`. |
| 7 | `perf audit` | Comprehensive audit: vitals, navigation, resources, DOM stats. 130 resources. |
| 8 | `perf audit --format text` | Structured text with sections for vitals, issues, resources, DOM stats. |
| 9 | `perf compare` | Works. Both URLs now show `lcp_ms: 0.0` with `lcp_approximate: true` (consistent with vitals). |
| 10 | `dom stats` / `dom stats --format text` | 2069 nodes, 625KB document. |
| 11 | `dom "h1" --text` | Returns "Vergleichen mit Comparis." |
| 12 | `dom "h1" --count` | Returns count=1. |
| 13 | `dom "a" --attrs --limit 5` | Returns 5 of 204 links with attributes. Hint message helpful. |
| 14 | `dom tree --depth 2` | Native WalkerActor tree. Works. |
| 15 | `snapshot --depth 3 --max-chars 500` | Good structured snapshot for LLM consumption. |
| 16 | `snapshot --format text` | Beautiful indented tree with semantic info, interactive markers, text content. |
| 17 | `page-text` | Full page text extraction. |
| 18 | `eval 'document.title'` | Returns "Vergleichen und sparen – comparis.ch". |
| 19 | `click "a[href]"` | Flattened response: `{"clicked": true, "tag": "A", "text": ""}`. |
| 20 | `type "input" "test"` | **NOW FLATTENED:** `{"typed": true, "tag": "INPUT", "value": "test"}`. |
| 21 | `wait --selector "body"` | Instant match, 1ms. |
| 22 | `wait --text "Comparis"` | Matched in 1ms. |
| 24 | `storage local` | Returns 29 localStorage entries. |
| 25 | `storage localStorage` | Alias works, same data. |
| 26 | `storage session` | Returns 6 sessionStorage entries. |
| 27 | `storage sessionStorage` | Alias works, same data. |
| 28 | `geometry "header" --visible-only` | Returns 6 visible elements (filters out 37 hidden ones). |
| 29 | `geometry "header"` | 43 total, 20 shown with hint. |
| 30 | `styles "h1"` | Full computed styles. |
| 32 | `a11y --depth 3` | **NOW WORKS** with JS fallback. Returns roles (banner, contentinfo, alert). Debug message shows fallback. |
| 33 | `a11y contrast` | 7 elements checked, all pass AA. |
| 35 | `sources` | **NOW WORKS** with JS DOM/Performance API fallback. Returns 51 scripts. |
| 37 | `navigate --with-network` | Captured 161 requests, 25MB transferred. |
| 39 | `network --follow` | 321 lines captured during navigation. Excellent real-time streaming. |
| 40 | `reload` | Instant. |
| 41 | `back` / `forward` | Both work. |
| 43 | `inspect <actor>` | Works in daemon mode. Grip actor IDs persist across commands. |
| 44 | `recipes` | Curated jq one-liners. |
| 45 | `llm-help` | Complete CLI reference. |
| 46 | Final `perf vitals` | tbt_ms=0.0 (not -0.0). Confirmed. |

### Commands That Failed or Had Issues

| # | Command | Error | Severity |
|---|---------|-------|----------|
| 23 | `cookies` | Returns 0 results despite 32 cookies existing (verified via `eval 'document.cookie'`) | **HIGH** |
| 31 | `responsive "body" --widths 320,768,1024` | `setViewportSize` unrecognized by Firefox 149 ResponsiveActor | **MEDIUM** |
| 34 | `screenshot` | All screenshot methods unrecognized by Firefox 149; JS fallback also failed | **HIGH** |
| 36 | `network --limit 20` | Returns 0 results — no Performance API fallback in daemon mode | **MEDIUM** |
| 38 | `navigate --with-network --network-timeout 5` | Only 1 request captured, `total_transfer_bytes: -0.0` | **MEDIUM** |
| 42 | `console --follow` | No output despite eval-generating console.log/warn/error in parallel | **MEDIUM** |

### UX Friction Points

1. **`cookies` returns 0 results** — The StorageActor cookie listing appears broken. `document.cookie` via eval confirms 32 cookies exist. This is a significant regression from session 30 where cookies worked.

2. **`a11y --depth 3` prints debug message to stdout** — The debug line `debug: accessibility walker root methods unrecognized...` is printed before the JSON output. This should go to stderr or be suppressed, as it breaks JSON piping.

3. **`sources` prints debug message to stdout** — Same issue: `debug: sources thread actor failed...falling back to JS` pollutes JSON output.

4. **`navigate --with-network --network-timeout 5` still captures only 1 request** — The short timeout misses most WatcherActor events in daemon mode. `total_transfer_bytes: -0.0` is also wrong.

5. **`network --limit 20` returns 0 results in daemon mode** — No Performance API fallback. The hint message is helpful but the experience is poor.

6. **`lcp_ms` always 0.0** — Still a DOM approximation, not a real LCP value. Consistently 0.0 across all commands.

7. **`responsive` command entirely broken on Firefox 149** — `setViewportSize` not supported. No JS fallback exists.

8. **`console --follow` still produces no output** — Even after generating console messages via eval in a parallel daemon connection.

### Missing Features / Suggestions

1. **Fix `cookies` command** — StorageActor cookie listing returns empty. Either a daemon-mode regression or Firefox 149 compat issue.
2. **Route debug/fallback messages to stderr** — `a11y` and `sources` debug messages pollute stdout JSON. Use `eprintln!` instead of `println!`.
3. **Enable Performance API fallback for `network` in daemon mode** — Currently only works in `--no-daemon`.
4. **Screenshot fallback** — Investigate `browsingContext.captureScreenshot` (WebDriver BiDi) or html2canvas as alternatives.
5. **Responsive fallback** — Use JS `window.resizeTo()` or CSS media query simulation instead of ResponsiveActor.
6. **LCP estimation** — Consider `largest-contentful-paint` Performance entries or mutation-observer approach.

### Performance Assessment

- All working commands are fast: most complete in under 100ms.
- `navigate --with-network` (no timeout) works well: 161 requests captured.
- `navigate --with-network --network-timeout 5` still underperforms in daemon mode (1 request vs many).
- `network --follow` is excellent: 321 NDJSON lines captured during a single navigation.
- `perf compare` with 2 URLs completed quickly.

### Firefox 149 Compatibility

Three areas remain broken due to Firefox 149 protocol changes:
1. **ResponsiveActor** — `setViewportSize` unrecognized (responsive command)
2. **screenshotContentActor** — `captureScreenshot`/`screenshot`/`capture` all unrecognized, JS fallback also fails
3. **consoleActor** — `console --follow` events not delivered (likely protocol change)

Two areas now have working JS fallbacks:
1. **AccessibilityWalker** — `getRootNode`/`getDocument` unrecognized, but JS eval fallback works
2. **Thread sources** — `undefined` error, but JS DOM/Performance API fallback returns scripts

## Regression Check from Session 30

| # | Session 30 Issue | Status in Session 31 | Notes |
|---|-----------------|---------------------|-------|
| 1 | `type` response not flattened | **FIXED** | Now returns `{"typed": true, "tag": "INPUT", "value": "test"}` |
| 2 | `responsive` does not resize viewport | **CHANGED** — now returns clear error | Was silently wrong; now fails with `setViewportSize` unrecognized |
| 3 | `navigate --with-network --network-timeout 5` captures few requests | **NOT FIXED** | Still only 1 request, `total_transfer_bytes: -0.0` |
| 4 | `network --limit 20` returns 0 in daemon | **NOT FIXED** | No Performance API fallback in daemon mode |
| 5 | `lcp_ms` always 0.0 or null | **IMPROVED** | Consistent 0.0 with `lcp_approximate: true` in both vitals and compare |
| 6 | `perf compare` lcp_ms inconsistency | **FIXED** | Now shows `lcp_ms: 0.0` with `lcp_approximate: true`, same as vitals |
| 7 | `console --follow` no output | **NOT FIXED** | Still produces nothing |
| 8 | Transient `listTabs` error | **NOT OBSERVED** | No transient errors during this session |
| 9 | `a11y --depth 3` broken (Firefox 149) | **FIXED** | JS eval fallback now works |
| 10 | `screenshot` broken (Firefox 149) | **NOT FIXED** | All methods + JS fallback fail |
| 11 | `sources` broken (Firefox 149) | **FIXED** | JS DOM/Performance API fallback returns scripts |
| 12 | `--fields` flag not tested | **FIXED** | Works correctly, filters output to requested fields |
| 13 | `cookies` worked in session 30 | **REGRESSED** | Returns 0 results despite cookies existing |

**Summary:** 6 of 12 prior issues fixed or improved. 1 new regression (cookies). 5 remain unfixed (screenshot, console --follow, network daemon fallback, network-timeout, responsive).

## Test Matrix

| Command | Result | Notes |
|---------|--------|-------|
| navigate | Pass | |
| tabs | Pass | |
| perf vitals | Pass | |
| perf vitals --format text | Pass | |
| perf vitals --jq | Pass | |
| perf vitals --fields | Pass | **NEW: works** |
| perf audit | Pass | |
| perf audit --format text | Pass | |
| perf compare | Pass | lcp_ms now consistent |
| dom stats | Pass | |
| dom stats --format text | Pass | |
| dom selector --text | Pass | |
| dom selector --count | Pass | |
| dom selector --attrs | Pass | |
| dom tree | Pass | |
| snapshot | Pass | |
| snapshot --format text | Pass | |
| page-text | Pass | |
| eval | Pass | |
| click | Pass | |
| type | Pass | **NEW: flattened response** |
| wait --selector | Pass | |
| wait --text | Pass | |
| cookies | **Fail** | **REGRESSION: 0 results** |
| storage local | Pass | |
| storage localStorage | Pass | |
| storage session | Pass | |
| storage sessionStorage | Pass | |
| geometry --visible-only | Pass | |
| geometry | Pass | |
| styles | Pass | |
| responsive | **Fail** | Firefox 149 compat |
| a11y --depth 3 | Pass | **NEW: JS fallback works** |
| a11y contrast | Pass | |
| screenshot | **Fail** | Firefox 149 compat |
| sources | Pass | **NEW: JS fallback works** |
| network --limit 20 | **Fail** | No daemon fallback |
| navigate --with-network | Pass | 161 requests |
| navigate --with-network --network-timeout 5 | **Fail** | Only 1 request |
| network --follow | Pass | 321 lines |
| reload | Pass | |
| back | Pass | |
| forward | Pass | |
| console --follow | **Fail** | No output |
| inspect | Pass | |
| recipes | Pass | |
| llm-help | Pass | |
| perf vitals (final) | Pass | tbt_ms=0.0 |
