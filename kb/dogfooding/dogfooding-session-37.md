---
title: "Dogfooding Session 37"
type: dogfooding
date: 2026-04-16
status: completed
site: https://www.sbb.ch
commands_tested: [tabs, navigate, eval, page-text, dom, screenshot, scroll, type, click, wait, perf, network, a11y, cookies, storage, sources, snapshot, geometry, responsive, computed, reload, styles, recipes, llm-help]
tags: [dogfooding, regression, scroll, eval, screenshot]
---

# Dogfooding Session 37

First dogfooding on sbb.ch (Swiss Federal Railways). Focused on new features from iter-41 through iter-43: scroll commands, eval --file/--stdin, screenshot --full-page, computed command, reload --wait-idle. Also regression-tested cookies (broken in [[dogfooding/dogfooding-session-32]]).

## What's New Since Last Session

- **iter-40**: Daemon bypass for cookies, storage, a11y, sources
- **iter-41**: `scroll` command (page-up/down, by dx/dy, to selector, text search)
- **iter-42**: Site-audit skill
- **iter-43**: `eval --file`/`--stdin`, `screenshot --full-page`, `computed` command, `reload --wait-idle`
- **iter-44**: Release pipeline (CI/CD, not CLI-facing)
- `--version` / `-V` flag added

## Regression Checks

| Command | Previous Status | Current Status | Notes |
|---------|----------------|----------------|-------|
| `cookies` | BROKEN (TypeError crash, session 32) | **FIXED** | Returns 10 cookies cleanly. No crash. Daemon bypass (iter-40) resolved the actor issue. |
| `responsive` | FIXED (session 32) | **STILL WORKS** | Body widths 320/768/1024 all correct. |
| `a11y` | PASS with JS fallback | **STILL WORKS** | Fallback debug messages still on stdout (not stderr). |
| `sources` | PASS with JS fallback | **STILL WORKS** | Same fallback debug noise on stdout. |

## Smoke Test Results

| Command | Status | Notes |
|---------|--------|-------|
| `navigate` | PASS | sbb.ch loads cleanly |
| `tabs` | PASS | Correct tab state |
| `page-text` | PASS | Meaningful German content extracted |
| `screenshot -o` | PASS | Clean 1366x683 viewport capture |
| `snapshot` | PASS | Semantic tree with web component structure |
| `dom stats` | PASS | 840 nodes, 407KB, 24 inline scripts |
| `dom "input"` | PASS | Found timetable inputs by selector |
| `type "input#origin" "Zürich"` | PASS | Value set correctly |
| `scroll by --page-down` | PASS | y=0 → y=581, screenshots confirm |
| `scroll to "sbb-footer"` | PASS | Scrolled to footer element |
| `scroll text "Kontakt"` | PASS | Text-based scroll discovery works great |
| `eval --stdin` | PASS | `echo 'document.title' \| ff-rdp eval --stdin` → correct title |
| `eval --file` | PASS | JS file execution works seamlessly |
| `computed "sbb-header"` | PASS | Non-default props returned |
| `computed --prop display` | PASS | Returns `"block"` |
| `reload --wait-idle` | PASS (with caveat) | Reloads, but `requests_observed: 0` — see issues |
| `perf vitals` | PASS | FCP 106ms, TTFB 21ms, CLS 0.0 — plausible |
| `perf summary` | PASS | 35 requests to sbb.ch (752KB) |
| `perf audit` | PASS | Clean text output, no flagged issues |
| `network --limit 20` | PASS | Tabular output, 20 of 125 requests |
| `a11y contrast --fail-only` | PASS | Found real WCAG violations (3.32:1 ratio text) |
| `cookies` | PASS | 10 cookies including consent and tracking |
| `storage localStorage` | PASS | 1 New Relic session entry |
| `sources --limit 10` | PASS (with caveat) | Returns all 27 — `--limit` ignored |
| `styles "a" --limit 1` | PASS | Clean tabular CSS output |
| `geometry` | PASS (empty) | No standard `header`/`nav` — SBB uses web components |
| `responsive "body" --widths 320,768,1024` | PASS | Correct breakpoint widths |
| `--jq` filters | PASS | `perf vitals --jq '.results \| {fcp: .fcp_ms}'` works |
| `-V` / `--version` | PASS (local build) | `ff-rdp 0.1.0`. Note: installed binary at `~/.cargo/bin` is outdated and lacks this flag. |

## Findings

### Issues Found

1. **HIGH: `screenshot --full-page` does not capture full page.** The flag is accepted but silently ignored — output is viewport-sized (683px) instead of full scroll height (3841px). The PNG file confirms only viewport content captured. This was the headline feature of [[iterations/iteration-43-dx-fixes]] and needs a fix.

2. **MEDIUM: `--format text` inconsistency across commands.** `perf summary`, `a11y`, and `responsive` output JSON even when `--format text` is specified. Other commands (`network`, `cookies`, `a11y contrast`, `styles`) correctly produce tabular text. Inconsistent UX.

3. **MEDIUM: `--limit` ignored by `a11y` and `sources`.** Both commands return all results regardless of `--limit` value. Other commands (`network`, `styles`) respect it.

4. **LOW: `scroll by --dy -99999` fails with arg parsing error.** Negative numbers after `--dy` (space-separated) are parsed as flags (`-9`). Workaround: `--dy=-99999` (equals syntax). Known clap limitation with negative numeric arguments.

5. **LOW: `reload --wait-idle` reports `requests_observed: 0`.** The network watcher may not attach before the reload fires, missing all requests. The reload itself works — only the observation count is wrong.

6. **LOW: `a11y` and `sources` emit debug messages to stdout.** "accessibility walker root methods unrecognized" and "sources thread actor failed" appear in stdout output. Should go to stderr or be `--verbose` only. (Known from [[dogfooding/dogfooding-session-32]], still present.)

7. **INFO: `a11y contrast` false positives on web components.** Many sbb-block-link / sbb-navigation elements report 1:1 ratio (white on white) — likely shadow DOM internals where computed styles don't reflect actual rendering.

### What Works Well

- **`scroll` command family** — page-up/down, dy offsets, selector-based and text-based scrolling all work. `scroll text "Kontakt"` is especially nice for content discovery. Output includes useful metadata (scrolled, atEnd, viewport position, scrollHeight).
- **`eval --file` and `--stdin`** — seamless. This fully resolves the shell quoting issues from [[dogfooding/dogfooding-session-nova-template-jsonforms-index]].
- **`cookies` fixed** — clean 10-cookie result with proper flags (httpOnly, secure, sameSite). Major improvement over the TypeError crash in session 32.
- **`perf audit`** — excellent single-command overview. Text format is well-structured.
- **`computed --prop`** — quick CSS debugging. Smart default of showing only non-default properties.
- **`snapshot`** — good LLM-friendly semantic tree even with web components.
- **Error messages** — `scroll to "footer"` failure message was helpful: "Element not found: footer — use ff-rdp dom SELECTOR --count to verify".

### Feature Gaps

- `scroll` could use `--top` / `--bottom` shortcuts instead of requiring `--dy=99999` / `--dy=-99999`
- `computed` only accepts single `--prop` — multi-property queries (`--prop display,position,color` or repeated `--prop` flags) would be useful
- The negative-number clap issue (`--dy -99999`) should at least be documented in `--help`

## Summary

- **29 commands tested**, 29 PASS (some with caveats), 1 high-severity bug (`screenshot --full-page`), 6 lower-severity issues (2 medium, 3 low, 1 info)
- **Cookies regression fixed** — the session 32 TypeError is gone thanks to daemon bypass
- **New features (scroll, eval --file/--stdin, computed) work well** — but `screenshot --full-page` is broken
- Key takeaway: The CLI is maturing nicely for real-world use. The scroll + eval --stdin combination is powerful. Fix `--full-page` before release.
