---
title: "Dogfooding Session 39"
type: dogfooding
date: 2026-04-17
status: completed
site: "https://github.com/anthropics/claude-code"
commands_tested: [tabs, navigate, eval, page-text, dom, screenshot, scroll, click, back, perf, network, a11y, cookies, storage, sources, snapshot, geometry, responsive, computed, reload, styles, recipes, llm-help, launch]
tags: [dogfooding, regression, iter-47, iter-48, ai-ergonomics]
---

# Dogfooding Session 39

First dogfooding on GitHub (anthropics/claude-code repo page). Focused on verifying iter-47 bug fixes and iter-48 new AI-agent ergonomics features. Previous session: [[dogfooding/dogfooding-session-38]].

## What's New Since Last Session

- **iter-47**: Fixed `launch` prefs, scroll viewport, geometry/network/dom-tree `--format text`, responsive rect.y, recipes help flags
- **iter-48**: Added `eval --stringify`, `styles --properties`, `dom --text-attrs`, `a11y summary`, improved recipes and llm-help

## Regression Checks (iter-47/48 fixes)

| Issue (from session 38) | Status | Notes |
|--------------------------|--------|-------|
| `launch` ignores remote debug prefs | **FIXED** | Fresh profile connects on first try. No manual user.js needed. |
| `scroll top/bottom` stale viewport | **NOT FIXED** | Now returns Promise actor grip instead of viewport position data. See bug #1. |
| `geometry --format text` outputs JSON | **FIXED** | Clean tabular: selector, tag, x, y, width, height, visible, in_viewport |
| `network --format text` (summary) JSON | **FIXED** | Proper tabular output |
| `dom tree --format text` outputs JSON | **FIXED** | Proper indented tree |
| `reload --wait-idle` 0 requests | **NOT FIXED** | Still `requests_observed: 0` with `idle_at_ms: 10049`. Watcher timing issue persists. |
| `responsive` negative rect.y | **FIXED** | All y values positive or 0. Was -117096 before. |
| `recipes --help` irrelevant flags | **FIXED** | Only shows usage line, no --host/--port/--timeout |

## New Feature Tests (iter-48)

| Feature | Status | Notes |
|---------|--------|-------|
| `eval --stringify` | **PASS** | Plain objects/arrays work perfectly. DOM NodeLists serialize as empty objects (expected). |
| `styles --properties color,display,font-size` | **PASS** | Returns exactly 3 properties (375 bytes vs 137KB without filter). **373x size reduction!** |
| `styles --properties --format text` | **PASS** | Clean 3-row table |
| `dom --text-attrs --limit 10` | **PASS** | Returns combined textContent + attrs per element. 218 links found, properly limited to 10. |
| `a11y summary` (JSON) | **PASS** | Landmarks (8), headings (34), interactive (50 of 283). Well-structured with truncation hints. |
| `a11y summary --format text` | **PASS** | Beautiful output with indented heading levels and role/name/href for interactive elements. ~67 lines. |
| `a11y summary --limit 10` | **PASS** | Limits interactive section to 10 (landmarks/headings unaffected). Sensible behavior. |
| Improved `llm-help` | **PASS** | Has "AI agent recommendations" section with 4 actionable bullet points (--format text, --stringify, --properties, a11y summary). |
| Fixed recipes | **PASS** | Network recipes mention `--detail` with explanation. |

## Smoke Test Results

| Command | Status | Notes |
|---------|--------|-------|
| `launch --headless` | PASS | Fresh profile auto-connects |
| `navigate` | PASS | GitHub SPA loads correctly |
| `tabs --format text` | PASS | Clean tabular |
| `page-text --limit 50` | PASS | Content extracted |
| `snapshot --format text --depth 4` | PASS | Clean tree format |
| `screenshot -o` | PASS | 1366x683 PNG |
| `screenshot --full-page` | PASS | 1366x3623 PNG (851KB) |
| `dom stats --format text` | PASS | 1892 nodes, 337KB |
| `dom "a" --text-attrs --limit 10` | PASS | Combined text+attrs works |
| `dom tree --format text --depth 3` | PASS | Proper tree |
| `scroll bottom` | PASS (with bug) | Scrolls correctly, returns Promise grip |
| `scroll top` | PASS (with bug) | Scrolls correctly, returns Promise grip |
| `scroll text "README"` | PASS (with note) | Found script tag match instead of visible text |
| `eval --stringify '{...}'` | PASS | Returns actual JSON data |
| `eval --stdin` (pipe) | PASS | Works correctly |
| `click` + `back` | PASS | Navigation works |
| `perf vitals --format text` | PASS | Clean key-value format |
| `perf summary --format text` | PASS | Well-structured sections |
| `perf audit --format text` | PASS | Comprehensive report |
| `perf vitals --jq` | PASS | Compact filtered output |
| `network --format text --limit 10` | PASS | Tabular summary |
| `network --detail --format text` | PASS | Tabular detail |
| `reload --wait-idle` | FAIL | requests_observed: 0 |
| `a11y --format text --depth 3` | PASS | Tree output |
| `a11y summary` | PASS | Excellent new feature |
| `a11y contrast --fail-only` | PASS | 0 failures out of 6 |
| `geometry "h1" "nav" --format text` | PASS | Tabular output |
| `responsive "h1" --format text` | PASS | Breakpoint sections, positive rect.y |
| `styles "h1" --properties` | PASS | 373x size reduction |
| `styles "h1" --applied --limit 5` | PASS (with note) | Rules found but properties arrays empty |
| `computed "h1" --prop display` | PASS | Single property works |
| `cookies` | PASS | 6 cookies (gh_sess, _octo, etc.) |
| `storage localStorage` | PASS | 2 entries |
| `sources --limit 5 --format text` | PASS | Tabular |
| `console --level error --format text` | PASS | Many Referrer Policy errors |
| Error cases (click, dom, navigate) | PASS | Clear, actionable error messages |
| `recipes` | PASS | 183 lines, --detail documented |
| `recipes --help` | PASS | No irrelevant flags |
| `llm-help --format text` | PASS | 507 lines, AI agent section present |
| `-V` / `--version` | PASS | ff-rdp 0.1.0 |

## Findings

### Issues Found

1. **MEDIUM: `scroll top/bottom/by` returns Promise actor grip instead of viewport position.** The commands execute correctly (page scrolls) but the return value is `{"class": "Promise", "type": "object", "actor": "..."}` instead of the expected viewport position data. The iter-47 fix may have changed the scroll JS to use async `scrollTo()` but the Promise isn't being awaited before reading the result.

2. **MEDIUM: `reload --wait-idle` still reports `requests_observed: 0`.** Carryover from sessions 37 and 38. After reloading GitHub (151+ requests), reports 0 observed and times out at 10s. The network event subscription doesn't hook up fast enough.

3. **LOW: `styles --applied` returns empty properties arrays.** Applied CSS rules show selectors and source locations but `properties: []` for all rules. Pre-existing, not a regression.

4. **LOW: `scroll text "README"` matches script tags instead of visible text.** The TreeWalker should prefer visible text nodes over script content.

5. **INFO: `computed` lacks multi-property filter parity with `styles --properties`.** `computed` only supports `--prop <single>`, while `styles --properties color,display,font-size` takes a comma-separated list. Design gap, not a bug.

### What Works Well

- **`styles --properties`** — The standout feature. Reduces 137KB to 375 bytes (373x). Essential for AI agents. This alone justifies iter-48.
- **`a11y summary --format text`** — Beautiful output with indented heading hierarchy and role/name/href for interactive elements. Perfect for page orientation.
- **`eval --stringify`** — Eliminates the actor grip trap. Plain objects work perfectly.
- **`geometry --format text`** — Clean tabular output. The iter-47 fix works well.
- **`launch` auto-prefs** — Fresh profile just works. Major onboarding improvement.
- **Error messages** — Consistently helpful with actionable suggestions (e.g., "use ff-rdp dom SELECTOR --count to verify").
- **`llm-help` AI agent section** — Four concise, actionable recommendations. Well-written.
- **`--format text` consistency** — Almost all commands now have text formatters. Big improvement over session 38.

### Feature Gaps

- `computed --properties color,display,font-size` for multi-property queries (parity with `styles --properties`)
- `scroll text` should prefer visible text nodes over script content
- `styles --applied` should populate the properties arrays (currently always empty)

## Summary

- **38 commands tested**, 36 PASS, 2 FAIL (`reload --wait-idle`, scroll return value)
- **5 of 7 session-38 regressions confirmed FIXED** (launch prefs, geometry text, network text, dom tree text, responsive rect.y, recipes help)
- **2 bugs persist** (scroll Promise grip, reload --wait-idle 0 requests)
- **All 6 iter-48 features work well** — `styles --properties` (373x reduction) and `a11y summary` are standout additions
- Key takeaway: The CLI is now highly AI-agent-friendly. `styles --properties`, `a11y summary`, and `eval --stringify` dramatically reduce token waste. The two remaining bugs (scroll return value, reload --wait-idle) are the only blockers to a clean bill of health.
