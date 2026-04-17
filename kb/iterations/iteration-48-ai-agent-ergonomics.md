---
title: "Iteration 48: AI-Agent Ergonomics"
type: iteration
date: 2026-04-17
status: completed
branch: iter-48/ai-agent-ergonomics
tags:
  - iteration
  - feature
  - dx
  - ai
  - token-efficiency
  - eval
  - styles
  - dom
---

# Iteration 48: AI-Agent Ergonomics

Reduce token waste and improve usability for LLM agents consuming ff-rdp output. Findings from [[dogfooding/dogfooding-session-38]] — the session specifically evaluated AI-agent ergonomics, output verbosity, and help text gaps.

## Motivation

`--format text` is consistently 3-10x more compact than JSON, but nothing recommends it to AI agents. The two biggest token-waste sources are:
1. `eval` returning actor grips (~62 lines of metadata) instead of values for complex objects
2. `styles` computed mode dumping all ~500 CSS properties (49KB) with no way to filter

These ergonomics improvements make ff-rdp dramatically more efficient for LLM agent workflows without changing any behavior for human users.

## Tasks

### 1. Add `eval --stringify` flag [0/4]

When `eval` returns a non-primitive (object, array), Firefox returns actor grip metadata (actor IDs, class names, frozen/sealed flags) instead of the actual values. LLM agents must manually wrap expressions in `JSON.stringify()` — this is the #1 trap for new agent integrations.

- [x] Add `--stringify` flag to `eval` subcommand
- [x] When `--stringify` is set, wrap the user's expression in `JSON.stringify(...)` before sending to Firefox
- [x] Handle edge cases: expressions that already call `JSON.stringify`, expressions with syntax errors, circular references (catch and return error)
- [x] Update `llm-help` and `eval --help` to document actor grips and recommend `--stringify`

### 2. Add `styles --properties` filter [0/3]

`styles` computed mode returns all ~500 CSS properties (49KB JSON). There's no way to request just `color,display,font-size`. The existing `--limit` works but is positional (first N properties alphabetically), not semantic.

- [x] Add `--properties <comma-separated-list>` flag (e.g., `--properties color,display,font-size,position`)
- [x] Filter the computed styles response to only include the requested properties
- [x] Update help text and `llm-help` to warn about the 500+ property default and recommend `--properties` or `--limit`

### 3. Add `dom` combined text+attrs mode [0/3]

`--text` and `--attrs` are mutually exclusive, but LLM agents often need both (e.g., link text + href). Currently requires two separate commands or an `eval` with `JSON.stringify()`.

- [x] Add `--text-attrs` flag (or make `--text` and `--attrs` combinable) that returns both `textContent` and attributes per element
- [x] Output format: array of `{textContent, attrs: {key: value}}` objects
- [x] Update help text to document the mutual exclusivity and the combined mode

### 4. Fix `recipes` accuracy [0/2]

Some `--jq` recipes assume `--detail` mode implicitly. E.g., `ff-rdp network --jq '[.results[] | select(.status >= 400)]'` fails in default summary mode where `.results` is an object, not an array.

- [x] Audit all recipes — add `--detail` flag to recipes that require it
- [x] Add a note to `recipes` output explaining when `--detail` is needed

### 5. Improve `llm-help` for AI-agent workflows [0/4]

The `llm-help` content is excellent but has gaps for AI-agent-specific guidance.

- [x] Add a section recommending `--format text` as the default for AI agents, with token-savings estimates (3-10x)
- [x] Add warning about `eval` actor grips and recommend `--stringify`
- [x] Add warning about `styles` computed mode size and recommend `--properties` or `--limit`
- [x] Fix `llm-help --format text` to output raw markdown without surrounding quotes (currently wraps in `"..."`)

### 6. Add `a11y summary` subcommand [0/3]

The full `a11y` tree is often too verbose for AI agents (400+ lines). A flat summary of landmarks, headings, and interactive elements would be far more useful for most agent workflows (page orientation, form discovery, navigation).

- [x] Add `a11y summary` subcommand that returns a flat list: landmarks (banner, navigation, main, contentinfo), headings (h1-h6 with text), and interactive elements (links, buttons, inputs with name/role)
- [x] Support `--format text` (compact table) and JSON
- [x] Support `--limit` to cap the number of interactive elements returned

## Acceptance Criteria

- [x] `eval --stringify 'document.querySelectorAll("a")'` returns actual data, not actor grips
- [x] `styles "h1" --properties color,display,font-size` returns only 3 properties
- [x] `dom "a" --text-attrs --limit 5` returns text + href for 5 links
- [x] All recipes in `ff-rdp recipes` work as documented (no implicit --detail requirement)
- [x] `llm-help` includes AI-agent workflow recommendations
- [x] `a11y summary` returns a flat list of landmarks, headings, and interactive elements
- [x] All quality gates pass: `cargo fmt`, `cargo clippy`, `cargo test`
