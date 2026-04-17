---
title: "Dogfooding Session 38"
type: dogfooding
date: 2026-04-17
status: completed
site: "https://developer.mozilla.org/en-US/"
commands_tested: [tabs, navigate, eval, page-text, dom, screenshot, scroll, type, click, wait, perf, network, a11y, cookies, storage, sources, snapshot, geometry, responsive, computed, reload, back, forward, styles, recipes, llm-help, launch]
tags: [dogfooding, help-text, output-verbosity, ai-ergonomics]
---

# Dogfooding Session 38

First dogfooding on MDN Web Docs. Special focus on **help text quality for AI agents**, **output verbosity / token waste**, and **missing tools**. Tested against iter-45/46 fixes (scroll, full-page screenshot, format text, limit, stderr debug). Previous session: [[dogfooding/dogfooding-session-37]].

## What's New Since Last Session

- **iter-45**: Fixed `screenshot --full-page`, `--format text` consistency, `--limit` on a11y/sources, debug messages → stderr, added `scroll --top`/`--bottom`
- **iter-46**: Consolidated 29 e2e test binaries into one (internal, not CLI-facing)

## Critical Finding: Stale Binary Trap

The installed `ff-rdp` binary at `~/.cargo/bin/ff-rdp` was dated April 13 — before iter-45 merged. All iter-45 features (`scroll top/bottom`, `screenshot --full-page`, fixed `--limit`) were missing until a manual `cargo install --path crates/ff-rdp-cli` was run. The `ff-rdp launch` command should check/warn about binary staleness, or the build/merge workflow should include a rebuild step.

## Critical Finding: `ff-rdp launch` Ignores Remote Debug Prefs

`ff-rdp launch --headless --port 6000` fails to connect because Firefox's remote debugging prefs aren't set in a fresh profile. Manual workaround was to create a `user.js` with:
```
user_pref("devtools.debugger.remote-enabled", true);
user_pref("devtools.debugger.prompt-connection", false);
user_pref("devtools.chrome.enabled", true);
```
The `launch` command should write these prefs automatically.

## Regression Checks (iter-45 fixes)

| Issue (from session 37) | Status | Notes |
|--------------------------|--------|-------|
| `screenshot --full-page` broken | **FIXED** | Produces 1366×10000px image. Works correctly. |
| `--format text` inconsistent | **MOSTLY FIXED** | Works for: perf summary/audit, a11y, responsive, network detail. Still broken for: `geometry`, `network` summary, `dom tree`. |
| `--limit` ignored by a11y/sources | **FIXED** | `a11y --limit 5` and `sources --limit 3` now truncate correctly with hints. |
| Debug messages on stdout | **FIXED** | Both a11y and sources debug correctly goes to stderr. |
| `scroll --top`/`--bottom` missing | **FIXED** | `scroll top` and `scroll bottom` work (but see viewport bug below). |
| `reload --wait-idle` 0 requests | **NOT FIXED** | Still reports `requests_observed: 0` after full timeout. Watcher attaches after reload. |
| `scroll by --dy -100` parsing | **FIXED** | Both `--dy -100` and `--dy=-100` now work. |

## Smoke Test Results

| Command | Status | Notes |
|---------|--------|-------|
| `navigate` | PASS | MDN loads cleanly, `--wait-text` works |
| `tabs` | PASS | Correct state |
| `page-text` | PASS | Clean content extracted |
| `screenshot -o` | PASS | Clean viewport capture |
| `screenshot --full-page` | PASS | Full 10000px height capture |
| `snapshot` | PASS | Excellent semantic tree |
| `dom stats` | PASS | Clean stats |
| `dom "nav"` | PASS | Returns HTML (very verbose — see below) |
| `scroll bottom/top/by` | PASS (with bug) | Scrolling works but viewport position is stale |
| `eval --file / --stdin` | PASS | Both work seamlessly |
| `computed "body"` | PASS | Returns non-default properties |
| `perf vitals/summary/audit` | PASS | All work, text format excellent |
| `network` | PASS | Summary and detail modes work |
| `a11y` | PASS | Tree and contrast modes work, `--limit` works |
| `cookies` | PASS | Zero results with helpful consent hint |
| `storage localStorage` | PASS | Returns Glean telemetry data |
| `sources --limit 5` | PASS | Correctly truncated |
| `styles "a" --limit 1` | PASS | Shows 1 of 513 properties |
| `geometry` | PASS | Correct rects, no text format |
| `responsive` | PASS | Works, but verbose |
| `click "nonexistent"` | PASS | Clear actionable error message |
| `navigate "not-a-url"` | PASS | Good error with scheme hint |
| `back/forward` | PASS | Navigation works |
| `llm-help` | PASS | Comprehensive reference |
| `recipes` | PASS | Useful jq one-liners |
| `reload --wait-idle` | FAIL | 0 requests observed |
| `--version` / `-V` | PASS | Shows ff-rdp 0.1.0 |

## New Bugs Found

### 1. MEDIUM: `scroll top/bottom/by` reports stale viewport position

`scroll bottom` output shows `viewport.y: 0` when the actual scroll position is 24381. `scroll top` then shows `viewport.y: 24381` when actual is 0. The scroll JS executes correctly but the viewport position is read before the scroll takes effect. Likely fix: add a small delay or read position after scrollTo returns.

### 2. MEDIUM: `geometry --format text` silently outputs JSON

`ff-rdp geometry "h1" "nav" --format text` produces JSON. No warning. Same for `network --format text` in summary mode and `dom tree --format text`. These three commands need text formatters.

### 3. MEDIUM: `reload --wait-idle` still reports 0 requests (unfixed from session 37)

`reload --wait-idle` waits the full timeout then reports `requests_observed: 0` and `idle_at_ms: 10027`. The watcher attaches after the reload fires, missing all traffic.

### 4. LOW: `responsive` reports implausible negative `rect.y` values

At width=320, h1 shows `rect.y: -117096.5`. At width=1024, `rect.y: -3671`. The geometry capture happens before layout stabilizes after viewport resize.

### 5. LOW: `recipes` shows irrelevant global flags in `--help`

`recipes --help` shows `--host`, `--port`, `--timeout` which are meaningless for this static-text command. Same for `llm-help --help`.

## Help Text Review for AI Agents

### Top-level `--help`
**Verdict: GOOD.** 27 commands with clear one-line descriptions. An LLM can scan and pick the right command. The description for `snapshot` ("Dump structured page snapshot for LLM consumption") is excellent self-documentation.

### Per-command `--help` quality

| Command | Quality | Notes |
|---------|---------|-------|
| `navigate` | EXCELLENT | Output schema documented, wait flags clear |
| `network` | EXCELLENT | Best help text — explains daemon vs direct, fallback behavior, recommended workflows |
| `tabs` | GOOD | Output schema documented |
| `console` | GOOD | Output schema, `--follow` documented |
| `screenshot` | GOOD | Both output modes (file/base64) documented |
| `perf` | GOOD | Subcommands listed, `--type` options enumerated |
| `a11y` | GOOD | `--interactive`, `--depth`, `--max-chars` clear. Missing: note that `--limit` doesn't apply to tree depth |
| `snapshot` | GOOD | `--depth` and `--max-chars` with defaults |
| `dom` | GOOD | Mode flags clear. Missing: `--text` and `--attrs` mutual exclusivity not documented |
| `cookies` | GOOD | Output schema documented |
| `eval` | **NEEDS WORK** | Missing critical note: complex objects return actor grips, not values. LLMs MUST use `JSON.stringify()` — this is the #1 trap |
| `click` | OK | No mention of what happens when multiple elements match |
| `type` | OK | No example usage |
| `geometry` | OK | No example of overlap detection output |
| `responsive` | OK | Description is a single run-on sentence |
| `styles` | OK | No warning that computed mode returns 500+ properties (49KB). Missing: mention `--limit` for property filtering |
| `sources` | OK | `--filter` vs `--pattern` difference unclear (substring vs regex?) |

### `llm-help` (14KB JSON / 18KB text)
**Verdict: EXCELLENT content**, but the JSON format wraps markdown in `{"results": "..."}` which means all newlines are `\n`-escaped. An LLM wastes tokens parsing this. `--format text` should strip the JSON envelope cleanly.

**Gaps in llm-help content:**
- No warning that `eval` returns actor grips for objects
- No warning that `styles` (computed) produces 49KB+ output
- No mention of `--format text` as recommended default for AI agents

### `recipes` (4.5KB text)
**Verdict: GOOD.** Well-organized by category with practical `--jq` examples.

**Issues:**
- Some recipes assume `--detail` mode implicitly. E.g., `ff-rdp network --jq '[.results[] | select(.status >= 400)]'` fails in default summary mode where `.results` is an object.
- `recipes` outputs plain text regardless of `--format` flag, but help still shows `--format` as an option.

## Output Verbosity Analysis

### Token Efficiency Table

| Command | JSON lines | Text lines | Ratio | Verdict |
|---------|-----------|------------|-------|---------|
| `tabs` | 16 | 3 | 5x | Lean |
| `dom stats` | 14 | 5 | 3x | Lean |
| `page-text` | 136 | 129 | 1x | OK (text is the content) |
| `perf vitals` | 21 | 12 | 2x | Lean |
| `perf audit` | 137 | 34 | 4x | **Text is 4x smaller** |
| `snapshot --depth 6` | 565 | 95 | 6x | **Text is 6x smaller!** |
| `snapshot --depth 4` | 202 | 29 | 7x | **Text is 7x smaller!** |
| `network --limit 10` | 131 | 14 | 9x | **Text is 9x smaller** |
| `a11y --depth 3` | 55 | 52 | 1x | Similar |
| `a11y contrast --fail-only` | 14 | — | — | Lean |
| `sources` | 142 | 29 | 5x | Text wins |
| `cookies` | 9 | 2 | 5x | Lean |
| `geometry` | 44 | N/A | — | No text mode |
| `responsive (3 widths)` | 128 | ~50 | 3x | Bloated in both |
| `styles (computed, no limit)` | 2,565 | 513 | 5x | **WASTEFUL (49KB JSON!)** |
| `a11y contrast (all)` | 2,316 | 179 | 13x | **WASTEFUL (56KB JSON)** |
| `dom "nav"` | 60KB+ | N/A | — | **WASTEFUL (raw HTML)** |

### Key Verbosity Findings

1. **`styles` (computed, no `--limit`) is the worst offender**: 49KB JSON / 126KB text for ALL ~500 CSS properties of a single element. Desperately needs a `--properties <list>` filter flag.

2. **`a11y contrast` without `--fail-only`**: 56KB for 177 elements. The `--fail-only` flag (521 bytes) is the right solution and works well.

3. **`snapshot --format text` is the biggest win**: 5-7x smaller than JSON. Should be the recommended default for LLM agents.

4. **`dom` returns raw HTML which is extremely verbose**: An `h1` selector is fine, but `nav` returns the full navigation with SVGs, inline styles, etc. For LLM agents, `snapshot` or `a11y` are far better for understanding structure.

5. **`--format text` should be the default recommendation for AI agents**: Consistently 3-10x more compact across all commands.

6. **`eval` with complex objects returns actor grips** (actor IDs, class names, frozen/sealed flags) instead of actual values. 62 lines of metadata for a 5-line data extraction. Auto-stringify would save huge amounts of tokens.

## Feature Gaps / Missing Tools

### High Priority

1. **`eval --stringify` flag**: Auto-wrap results in `JSON.stringify()` to avoid actor grips. This is the #1 footgun for LLM agents.

2. **`styles --properties <list>` filter**: Get just `color,display,font-size` instead of all 500+ properties. Would reduce 49KB to ~100 bytes.

3. **`dom --text-and-attrs` combined mode**: `--text` and `--attrs` are mutually exclusive, but LLM agents often need both (e.g., link text + href). Even having `textContent` in `--attrs` output would help.

### Medium Priority

4. **`--format text` for geometry, network summary, dom tree**: These three commands silently fall through to JSON.

5. **`responsive --diff` mode**: Show only properties that changed between breakpoints. Current output repeats identical values at every width.

6. **`a11y landmarks` / `a11y summary`**: A flat list of landmarks, headings, and interactive elements instead of the full tree. The tree is often too verbose for AI consumption.

7. **`network --exclude-pattern <regex>`**: Filter out telemetry/tracking URLs that clutter output.

### Low Priority

8. **`screenshot --selector <css>`**: Screenshot of a specific element.

9. **`llm-help` output format**: The JSON wraps markdown in escaped newlines. `--format text` should output raw markdown without quotes.

10. **`recipes` accuracy**: Some `--jq` recipes assume `--detail` mode. Should note the requirement or add `--detail` to those recipes.

## What Works Well

- **Error messages are excellent**: `click` on missing selector says "use ff-rdp dom SELECTOR --count to verify". `navigate` with bad URL lists permitted schemes. This guidance is exactly what LLM agents need to self-correct.
- **Truncation hints**: `"hint": "showing 5 of 61, use --all for complete list"` — perfect for agents to know when they're missing data.
- **`cookies` zero-results hint**: Proactive guidance about consent banners and `--auto-consent`.
- **`snapshot --format text`**: The indented semantic tree with `[interactive]` markers is the best LLM-friendly page representation.
- **`perf audit --format text`**: Beautiful 34-line sectioned report vs 137 lines of JSON.
- **`a11y contrast --fail-only`**: Reduces 56KB to 521 bytes. Perfect filter.
- **`navigate --wait-selector/--wait-text`**: Combining navigation with wait conditions in one command is a major usability win.
- **`--jq` filtering**: Works perfectly across all commands. The best way to extract exactly what you need.
- **`llm-help` content**: Comprehensive, well-structured, includes troubleshooting and workflow patterns.

## Recommendations for AI-Agent Ergonomics

1. **Default to `--format text` in AI workflows**: Document this as the recommended approach in `llm-help`. Text is consistently 3-10x more compact.
2. **Add `eval --stringify`**: The actor grip problem wastes more tokens than any other issue.
3. **Add `styles --properties`**: The 49KB computed styles dump is the largest single output.
4. **Warn about large outputs in help text**: `styles --help` should mention that computed mode returns 500+ properties and suggest `--limit`.
5. **Complete `--format text` coverage**: The 3 commands without text formatters create inconsistency.
6. **Fix scroll viewport staleness**: The scroll commands work but misleading position output erodes trust.
7. **Fix `ff-rdp launch` to set debug prefs**: The manual user.js workaround is a major onboarding friction point.

## Summary

- **28 commands tested**, 27 PASS, 1 FAIL (`reload --wait-idle`)
- **5 of 7 iter-45 regression fixes confirmed** — `screenshot --full-page`, `--format text`, `--limit`, stderr debug, negative scroll args all fixed
- **5 new bugs found** — scroll viewport staleness (MEDIUM), geometry/network text format (MEDIUM), reload 0 requests (MEDIUM, unfixed carryover), responsive negative rects (LOW), recipes irrelevant flags (LOW)
- **Key insight**: `--format text` is 3-10x more token-efficient than JSON and should be the recommended default for AI agent workflows. The `styles` computed mode (49KB) and `eval` actor grips are the biggest token waste sources.
- **Key gap**: `eval --stringify` and `styles --properties` would dramatically reduce token waste for AI agents.
