---
title: "Iteration 3: Navigate + Eval JS"
type: iteration
date: 2026-04-06
tags:
  - iteration
  - navigate
  - eval
  - javascript
status: active
branch: iter-3/navigate-eval
---

# Iteration 3: Navigate + Eval JS

The two most critical commands for debugging: navigating to a URL and evaluating JavaScript expressions.

## Tasks

- [x] Extend `ff-rdp-core/src/actors/tab.rs` — `get_target()` returning WindowGlobalTargetActor info with consoleActor, threadActor, inspectorActor IDs
- [x] Extend `ff-rdp-core/src/actors/tab.rs` — `get_watcher()` returning WatcherActor ID
- [x] Implement `ff-rdp-core/src/actors/target.rs` — `WindowGlobalTarget` with `navigate_to(url)`, `reload()`, `go_back()`, `go_forward()`
- [x] Implement `ff-rdp-core/src/actors/console.rs` — `WebConsoleActor` with `evaluate_js_async(text)` handling the async response pattern (resultID correlation)
- [x] Implement grip deserialization in `types.rs` — handle object, longString, null, undefined, NaN, Infinity, function grips
- [x] Implement grip-to-JSON conversion: serialize grips as useful JSON values (objects as `{class, actor}`, strings inline, numbers inline, etc.)
- [x] Implement `ff-rdp-cli/src/commands/navigate.rs` — `ff-rdp navigate <url> [--tab ...]` with optional `--wait-load` flag
- [x] Implement `ff-rdp-cli/src/commands/eval.rs` — `ff-rdp eval <js-expression> [--tab ...]` returning result as JSON
- [x] Handle evaluation errors: exception field in response → structured error output
- [x] Handle long strings: detect `type: "longString"`, fetch full content from actor
- [x] Tests for grip type deserialization (all variants)
- [x] Tests for JS result formatting (primitive values, objects, errors, undefined)

## Acceptance Criteria

- `ff-rdp navigate https://example.com` opens the URL in the active tab
- `ff-rdp eval 'document.title'` returns the page title as a JSON string
- `ff-rdp eval '({a: 1, b: [2,3]})'` returns a serialized object representation
- `ff-rdp eval 'undefined'` returns `{"type": "undefined"}`
- `ff-rdp eval 'throw new Error("test")'` returns error info on stderr, exits non-zero
- `ff-rdp eval 'document.title' --jq '.results'` outputs just the title string
- Tab targeting works for both navigate and eval
