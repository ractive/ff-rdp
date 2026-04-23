---
title: "Iteration 50: Contextual Command Hints"
type: iteration
status: completed
date: 2026-04-24
branch: iter-50/contextual-hints
tags:
  - iteration
  - feature
  - dx
  - ai
  - hints
---

# Iteration 50: Contextual Command Hints

## Motivation

AI agents using ff-rdp often don't know the natural next command after running one. They trial-and-error through the CLI instead of following established workflows. Adding contextual hints (like [[../hyalo]]) gives every command output a "what next?" suggestion — copy-pasteable commands tailored to what just happened.

Human users benefit too: hints surface flags and subcommands they might not know exist.

## Design (modelled on hyalo's hints system)

### Data model

```rust
struct Hint {
    description: String,  // "Check for console errors"
    cmd: String,          // "ff-rdp console --level error"
}
```

Hints appear in the JSON envelope as `"hints": [{"description": "...", "cmd": "..."}, ...]` (always present, empty `[]` when suppressed). In `--format text` mode, rendered after the main output:

```
  -> ff-rdp console --level error  # Check for console errors
  -> ff-rdp screenshot -o page.png  # Capture a screenshot
```

### CLI flags

- `--hints` — force hints on (default in text mode)
- `--no-hints` — force hints off (default in JSON mode)
- Suppressed when `--jq` is active (pipeline needs clean data)

JSON-off-by-default avoids breaking existing scripts/agents that parse the envelope. Text-on-by-default helps humans and agents using `--format text`.

### Hint generation

Centralised in a new `hints.rs` module. A `HintSource` enum identifies which command produced the output. A `HintContext` carries the source + any state needed to build contextual commands (selector, URL, tab info, etc.). `generate_hints(ctx, data) -> Vec<Hint>` dispatches to per-command generators.

`MAX_HINTS = 5` to keep output concise.

### Hint map

| After command | Hints |
|---|---|
| `launch` | `tabs`, `navigate <URL>` |
| `navigate` | `snapshot --depth 3`, `console --level error`, `screenshot -o page.png`, `dom "h1" --text` |
| `tabs` | `navigate <URL> --tab 1` (using first tab) |
| `dom "sel"` | `click "sel"`, `styles "sel" --properties ...`, `computed "sel" --prop ...` |
| `dom stats` | `dom tree --depth 3`, `snapshot` |
| `click "sel"` | `wait --selector ...`, `screenshot -o after-click.png` |
| `type "sel" "text"` | `click "button[type=submit]"`, `wait --text ...` |
| `wait` | `snapshot`, `screenshot`, `dom "sel" --text` |
| `console` (has errors) | `console --follow --level error` |
| `console` (no errors) | `console --follow` |
| `network` (summary) | `network --detail`, `perf audit` |
| `network` (detail, has failures) | `network --detail --jq '[.results[] \| select(.status >= 400)]'` |
| `perf` | `perf vitals`, `perf audit` |
| `perf vitals` | `perf audit`, `perf summary` |
| `perf audit` | `a11y contrast --fail-only`, `screenshot -o audit.png` |
| `screenshot` | `snapshot --depth 3` |
| `snapshot` | `a11y summary`, `dom "sel" --text` |
| `a11y` | `a11y contrast --fail-only`, `a11y summary` |
| `a11y contrast` (has failures) | `a11y contrast --fail-only` (if not already filtered) |
| `a11y summary` | `a11y --interactive`, `a11y contrast` |
| `styles "sel"` | `computed "sel" --prop NAME`, `styles "sel" --applied`, `styles "sel" --layout` |
| `computed "sel"` | `styles "sel" --applied`, `geometry "sel"` |
| `geometry` | `responsive "sel"` |
| `responsive` | `screenshot -o responsive.png` |
| `reload` | `console --level error`, `network` |
| `cookies` | `storage local` |
| `storage` | `cookies` |
| `sources` | `eval --file script.js` |

### Output pipeline changes

1. `OutputPipeline` gains an optional `hints: Vec<Hint>` field
2. `finalize()` injects hints into the JSON envelope before printing
3. Text mode appends hint lines after the main output
4. When `--jq` is active, hints are not generated (skip the `generate_hints` call entirely)

## Tasks

### 1. Add hints data model and CLI flags
- [x] Create `crates/ff-rdp-cli/src/hints.rs` with `Hint`, `HintSource`, `HintContext`, `generate_hints()` stub
- [x] Add `--hints` / `--no-hints` global flags to `Cli` in `args.rs`
- [x] Default: hints on for text mode, off for JSON mode

### 2. Wire hints into the output pipeline
- [x] Add `hints: Vec<Hint>` to `OutputPipeline` or pass through `finalize()`
- [x] Modify `output::envelope()` and `envelope_with_truncation()` to accept and include hints
- [x] Render hints in text mode after main output: `-> cmd  # description`
- [x] Suppress hint generation when `--jq` is active
- [x] Add unit tests for envelope with hints, text rendering

### 3. Implement hint generators for navigation commands
- [x] `launch` → tabs, navigate
- [x] `navigate` → snapshot, console, screenshot, dom
- [x] `tabs` → navigate with tab index
- [x] `reload` → console, network
- [x] `back` / `forward` → snapshot, console

### 4. Implement hint generators for inspection commands
- [x] `dom` → click, styles, computed (carry selector through)
- [x] `dom stats` → dom tree, snapshot
- [x] `snapshot` → a11y summary, dom
- [x] `page-text` → dom, snapshot
- [x] `console` → console --follow (context-sensitive: errors vs empty)
- [x] `network` → network --detail, perf audit (context-sensitive: summary vs detail)
- [x] `sources` → eval --file

### 5. Implement hint generators for performance & accessibility
- [x] `perf` → perf vitals, perf audit
- [x] `perf vitals` → perf audit, perf summary
- [x] `perf audit` → a11y contrast, screenshot
- [x] `a11y` → a11y contrast, a11y summary
- [x] `a11y contrast` → context-sensitive (has failures vs all pass)
- [x] `a11y summary` → a11y --interactive, a11y contrast

### 6. Implement hint generators for interaction & CSS commands
- [x] `click` → wait, screenshot
- [x] `type` → click submit, wait
- [x] `wait` → snapshot, screenshot, dom
- [x] `styles` → computed, styles --applied, styles --layout
- [x] `computed` → styles --applied, geometry
- [x] `geometry` → responsive
- [x] `responsive` → screenshot

### 7. Implement hint generators for storage & utility commands
- [x] `cookies` → storage local
- [x] `storage` → cookies
- [x] `screenshot` → snapshot
- [x] `inspect` → (no hints — terminal command)

### 8. Update help text
- [x] Update `after_long_help` COMMAND REFERENCE to mention hints
- [x] Add hints section to AI AGENT TIPS
- [x] Document `--hints` / `--no-hints` in the global flags section

### 9. E2e tests
- [x] Test that `--format text` output includes hint lines
- [x] Test that `--format json` output includes `"hints": []` by default
- [x] Test that `--format json --hints` output includes populated hints
- [x] Test that `--jq` suppresses hints
- [x] Test that `--no-hints` suppresses hints in text mode

## Acceptance Criteria

- [x] All commands produce contextual hints when hints are enabled
- [x] Hints are on by default in `--format text`, off in JSON
- [x] `--jq` suppresses hints entirely (not generated, not in output)
- [x] `--hints` / `--no-hints` override defaults
- [x] JSON envelope always has `"hints"` key (empty array when suppressed)
- [x] Text mode renders hints as `-> cmd  # description` lines
- [x] `MAX_HINTS = 5` — no command produces more than 5 hints
- [x] All quality gates pass (fmt, clippy, tests)
