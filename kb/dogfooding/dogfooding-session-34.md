---
title: "Dogfooding Session 34: Pre-implementation LLM Ergonomics"
type: dogfooding
iteration: 39
date: 2026-04-09
status: completed
---

# Dogfooding Session 34: Pre-implementation LLM Ergonomics

Pre-implementation dogfooding for [[iteration-39-llm-ergonomics]]. Goal: identify pain points
where better help text, hints, or recipes would save an LLM user a round-trip.

## Methodology

Simulated common LLM workflows: navigating to a page, inspecting structure, checking performance,
and handling errors. Noted every instance where guessing was required.

## Findings

### 1. Help text gaps

| # | Command | Issue | Impact |
|---|---------|-------|--------|
| 1 | `ff-rdp network --help` | No mention of default limit (20) or sort order (duration desc) | LLM may not realize results are truncated or sorted |
| 2 | `ff-rdp console --help` | No mention of default limit (50) or sort order | Same as above |
| 3 | `ff-rdp navigate --help` | `--wait-text` and `--wait-selector` descriptions are terse; unclear when they run relative to navigation | LLM may use them incorrectly |
| 4 | `ff-rdp --help` | `--no-daemon` says "Don't use or start a daemon" — doesn't explain *why* you'd want this | LLM defaults to daemon but doesn't understand the tradeoff |
| 5 | All commands | No output format examples in `--help` | LLM has to run a command blind to discover the JSON structure |

### 2. Missing hints in error messages

| # | Scenario | Current Error | What Would Help |
|---|----------|--------------|-----------------|
| 6 | `click "button.nonexistent"` | `Element not found: button.nonexistent` | Suggest: `use ff-rdp dom "button.nonexistent" --count to verify` |
| 7 | `--tab 99` with 3 tabs | `tab index 99 out of range (1–3 tabs available)` | Suggest: `use ff-rdp tabs to list available tabs` |
| 8 | `navigate "javascript:alert(1)"` | `URL scheme 'javascript:' is not allowed; permitted schemes: ...` | Mention `--allow-unsafe-urls` flag |
| 9 | `wait --selector ".loaded" --wait-timeout 1000` (times out) | `wait timed out after 1000ms — condition not met: selector=".loaded"` | Suggest increasing `--wait-timeout` |

### 3. Missing zero-result guidance

| # | Command | Scenario | What Would Help |
|---|---------|----------|-----------------|
| 10 | `cookies` | Returns 0 cookies | Hint: page may not set cookies, or consent banner blocking |
| 11 | `console` | Returns 0 messages | Hint: use --follow for live, or generate with eval |
| 12 | `network` (no-daemon, page already loaded) | Returns 0 events | Hint: navigate first or use --follow |

### 4. Missing recipes

| # | Workflow | Notes |
|---|----------|-------|
| 13 | Fill and submit a form | No recipe for multi-step form interaction |
| 14 | Check element exists before clicking | No error-handling recipe |
| 15 | Full page audit pipeline | No cross-command workflow recipe |
| 16 | Fallback indicators | a11y/sources fall back to JS eval silently (only stderr debug msg); no structured indicator in output |

## Summary

16 issues identified across 4 categories. All addressable in iteration 39:
- 5 help text improvements (Part A)
- 4 error message improvements + 3 zero-result hints + 1 fallback indicator (Part B)
- 3 recipe gaps (Part C)
