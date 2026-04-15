---
title: "Iteration 41: Scroll Commands"
date: 2026-04-09
type: iteration
status: completed
branch: iter-41/scroll-commands
tags:
  - iteration
  - feature
  - scroll
  - interaction
  - dogfooding
completed: 2026-04-16
---

# Iteration 41: Scroll Commands

Add dedicated scroll commands to ff-rdp. Currently users must `eval 'window.scrollTo(0,600)'` -- the most-missed feature from [[dogfooding/dogfooding-session-36-comparison]].

**Design principle**: ff-rdp should beat Chrome MCP on scrolling. Chrome MCP can scroll the viewport but can't target overflow containers. We do both, plus smart scroll-until.

## Scroll Subcommands

Implement as `ff-rdp scroll <subcommand>`:

### `scroll to <selector>` — Scroll element into viewport
```bash
ff-rdp scroll to .listing:nth-child(5)
ff-rdp scroll to .listing:nth-child(5) --block center   # top|center|bottom|nearest
ff-rdp scroll to .listing:nth-child(5) --smooth
```
Uses `Element.scrollIntoView({block, behavior})`. Returns element's final `getBoundingClientRect()`.

### `scroll by [--dx <px>] [--dy <px>]` — Viewport scroll
```bash
ff-rdp scroll by --dy 600
ff-rdp scroll by --dy -300 --smooth
ff-rdp scroll by --page-down          # scroll ~85% of viewport height
ff-rdp scroll by --page-up
```
Uses `window.scrollBy()`. `--page-down`/`--page-up` calculate from `window.innerHeight`.

### `scroll container <selector> [--dx <px>] [--dy <px>]` — Overflow container scroll
```bash
ff-rdp scroll container .sidebar --dy 300
ff-rdp scroll container [role=listbox] --dy 200
ff-rdp scroll container .feed --to-end     # scroll to bottom
ff-rdp scroll container .feed --to-start   # scroll to top
```
Sets `element.scrollTop`/`scrollLeft`. Returns `{before, after, scrollHeight, clientHeight, atEnd}`.

**This is the competitive advantage** -- Chrome MCP can't do this.

### `scroll until <selector>` — Scroll until element is visible
```bash
ff-rdp scroll until "#load-more-sentinel"
ff-rdp scroll until ".error-message" --direction up
ff-rdp scroll until ".item:nth-child(50)" --timeout 10000
```
Polls: scroll viewport down by ~80% height, check if selector matches an element in viewport, repeat until found or timeout. Uses `poll_js_condition()` pattern from `wait` command.

### `scroll text <text>` — Find text and scroll to it
```bash
ff-rdp scroll text "Contact Us"
ff-rdp scroll text "CHF 3'400"
```
Uses TreeWalker to find text node, then `scrollIntoView()` on parent element. More reliable than `window.find()` which has quirks.

## Output Format

All scroll subcommands return consistent JSON:
```json
{
  "scrolled": true,
  "viewport": {"x": 0, "y": 600, "width": 1280, "height": 720},
  "target": {"selector": ".item", "rect": {"top": 100, "left": 0, "width": 400, "height": 80}},
  "atEnd": false
}
```

`--format text` shows a one-line summary: `Scrolled to .item (viewport y=600, element at top=100)`.

## Tasks

### Core Implementation
- [x] Add `Scroll` command enum with subcommands in `args.rs`
- [x] Create `crates/ff-rdp-cli/src/commands/scroll.rs` with JS IIFE templates
- [x] Wire up dispatch in `dispatch.rs`
- [x] Implement `scroll to` with `--block` and `--smooth` flags
- [x] Implement `scroll by` with `--dy`, `--dx`, `--page-down`, `--page-up`
- [x] Implement `scroll container` with `--dy`, `--dx`, `--to-end`, `--to-start`
- [x] Implement `scroll until` with polling loop and `--timeout`/`--direction`
- [x] Implement `scroll text` with TreeWalker approach

### Polish
- [x] Add `scroll` section to `recipes.rs`
- [x] Add `scroll` commands to `llm_help.rs`
- [x] `--format text` for all subcommands

### Tests
- [x] Mock server e2e tests for each subcommand
- [x] Live Firefox fixture recording for scroll commands
- [x] Test `scroll container` with overflow:auto div
- [x] Test `scroll until` with lazy-loaded content simulation

### Acceptance Criteria
- [x] `scroll to` brings element into viewport with correct `--block` positioning
- [x] `scroll by --page-down` scrolls ~85% of viewport height
- [x] `scroll container` works on overflow:auto/scroll elements (not just viewport)
- [x] `scroll until` polls and scrolls until selector appears or times out
- [x] `scroll text` finds text across element boundaries
- [x] All subcommands return consistent JSON with viewport/target info
- [x] `--format text` produces readable one-liner summaries
- [x] `recipes` and `llm-help` updated
