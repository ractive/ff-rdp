---
title: "Iteration 39: LLM Ergonomics — Help Text, Hints & Recipes"
type: iteration
status: completed
date: 2026-04-09
branch: iter-39/llm-ergonomics
tags:
  - iteration
  - ux
  - llm
  - help
  - documentation
  - dogfooding
---

# Iteration 39: LLM Ergonomics — Help Text, Hints & Recipes

## Goal

Make ff-rdp easier for LLMs (and humans) to use correctly on the first try. Focus on three areas: better help text, actionable hints in output/errors, and documented recipes for common workflows.

## Pre-implementation dogfooding

Before writing code, do a focused dogfooding session as an LLM user:
- [x] Navigate to a complex page (e.g. comparis.ch) and try to accomplish real tasks: find all broken images, measure performance, extract form fields, check accessibility
- [x] Note every time you have to guess a flag, re-run a command, or read source code to understand output
- [x] Record specific instances where a hint, better help text, or recipe would have saved a round-trip

Save findings in [[dogfooding-session-34]].

## Part A: Help Text Improvements [0/5]

### A1: Add output format examples to subcommand help [0/1]
- [x] For the 10 most-used commands (`tabs`, `navigate`, `eval`, `cookies`, `screenshot`, `network`, `console`, `perf vitals`, `dom`, `snapshot`), add an `Output:` section to the clap `long_about` showing the JSON structure. Keep it short — just the key fields, not every possible field. Example:
  ```json
  Output: {"results": [{"url": "...", "title": "...", "index": 1}], "total": N}
  ```

### A2: Document defaults and limits in help text [0/1]
- [x] Add default limits to help where they're not shown: `network` (default 20, sorted by duration), `console` (default 50), `perf resources` (default 20). Mention sort order where applicable.

### A3: Clarify `--wait-text` vs `--wait-selector` [0/1]
- [x] Expand navigate help to explain: `--wait-text` waits for visible text content, `--wait-selector` waits for a CSS selector to match. Both run *after* navigation completes. Mention `--wait-timeout`.

### A4: Expand `--no-daemon` help [0/1]
- [x] Change from "Don't use or start a daemon" to something like: "Connect directly to Firefox, bypassing the daemon. Use when you need a fresh connection or for one-off commands. The daemon (default) keeps a persistent connection and buffers events for streaming commands."

### A5: Update `llm-help` command [0/1]
- [x] Add output structure examples for the 5 most common commands
- [x] Add a "Troubleshooting" section: zero results, timeout errors, tab not found
- [x] Add a "Workflow patterns" section with 3-4 multi-command sequences

## Part B: Actionable Hints in Output & Errors [0/6]

### B1: Error message improvements [0/3]
- [x] "Element not found" errors → append "use `ff-rdp dom <selector> --count` to verify the selector matches"
- [x] "Tab not found" errors → append "use `ff-rdp tabs` to list available tabs"
- [x] URL validation rejection → mention `--allow-unsafe-urls` flag in the error message

### B2: Zero-result hints [0/1]
- [x] When any command returns `"total": 0`, add a `"hint"` field with a command-specific suggestion. Examples:
  - `cookies` with 0 results: `"hint": "No cookies found. The page may not set cookies, or try navigating to the page first."`
  - `console` with 0 results: `"hint": "No console messages. Use --follow to stream live messages, or generate some with: ff-rdp eval 'console.log(\"test\")'"`
  - `network` with 0 results: `"hint": "No network events captured. Events are buffered by the daemon; try navigating first with: ff-rdp navigate <url> --with-network"`

### B3: Standardize fallback indicators [0/1]
- [x] When `a11y` or `sources` falls back to JS eval, add `"fallback": true, "fallback_method": "js-eval"` to the output `meta` object (currently only logged to stderr)

### B4: Timeout suggestion in errors [0/1]
- [x] When a wait condition times out (`--wait-text`, `--wait-selector`), include the current timeout value and suggest increasing it: `"timed out after 5000ms waiting for selector '.loaded'; increase with --wait-timeout"`

## Part C: Recipes & Cookbook [0/3]

### C1: Add interaction workflow recipes [0/1]
- [x] Add to the `recipes` command: "Fill and submit a form", "Wait for dynamic content", "Navigate and verify"

### C2: Add error handling recipes [0/1]
- [x] Add to recipes: "Check if element exists before clicking", "Retry on timeout", "Verify navigation succeeded"

### C3: Add cross-command workflow recipes [0/1]
- [x] Add: "Full page audit" (navigate → perf → a11y → network → screenshot), "Monitor console in background" (console --follow → eval → check output)

## Post-implementation dogfooding

- [x] Re-run the same tasks from the pre-implementation session
- [x] Verify hints appear when expected and are helpful
- [x] Verify help text answers questions without needing to read source
- [x] Save findings in [[dogfooding-session-35]]

## Test Fixtures

No new fixtures needed — this is primarily help text and output metadata changes. Existing e2e tests should be updated to verify hint fields appear in zero-result cases.

## Acceptance Criteria

- [x] All 10 key commands show output format in `--help`
- [x] Error messages for element-not-found, tab-not-found, URL-rejected include actionable suggestions
- [x] Zero-result output includes `"hint"` field for at least cookies, console, network
- [x] `llm-help` includes output examples, troubleshooting, and workflow patterns
- [x] `recipes` includes interaction, error handling, and cross-command workflows
- [x] Pre/post dogfooding sessions documented
- [x] `cargo fmt`, `cargo clippy`, `cargo test` pass
