---
title: "Iteration 52: Input & Eval Ergonomics"
type: iteration
date: 2026-05-06
status: planned
branch: iter-52/input-eval-ergonomics
tags:
  - iteration
  - dx
  - ai
  - bugfix
  - type
  - eval
  - react
---

# Iteration 52: Input & Eval Ergonomics

Second of three iterations addressing [[../dogfooding/dogfooding-session-40]]. Depends on [[iteration-51-onboarding-fixes]] landing first. Companion: [[iteration-53-stability-fixes]].

Three surface-level papercuts that compound during interactive testing of modern web apps:

1. `type` mutates `input.value` directly — invisible to React/Vue/Svelte value trackers, so framework state never updates.
2. `type` only accepts selector/text positionally; everywhere else in the CLI uses `--selector`. Reaching for `--selector` on `type` produces a clap error with an unhelpful "tip."
3. `eval` shares global scope across invocations, so `const x = ...` fails on the second call with "redeclaration of const x."

## Tasks

### 1. Make `type` work on React/Vue/Svelte inputs [3/3]

`type` currently sets `input.value = ...` directly. Modern frameworks track values via React's value-tracker / Vue's v-model, so the change is silently discarded — the page looks unresponsive after `ff-rdp type`. Fix once for every framework.

- [ ] In the `type` JS payload, use the native prototype setter (`Object.getOwnPropertyDescriptor(window.HTMLInputElement.prototype, "value").set`, plus the equivalent for `HTMLTextAreaElement` and `HTMLSelectElement`) to invalidate React's value tracker before assigning.
- [ ] After the value mutation, dispatch `input` and `change` events with `{ bubbles: true }`.
- [ ] E2e test against a fixture page with a React-style controlled input (or a vanilla input wrapped in a tracker that throws on direct value assignment) — assert the bound state actually updates.

### 2. Improve `type` flag-vs-positional ergonomics [2/2]

`ff-rdp type --selector ... --text ... --clear` failed with a generic clap "tip" telling the user how to escape `--selector` as a value, not how to use the command. Other commands (`dom`, `wait`) accept `--selector`, so reaching for it was natural.

- [ ] Accept `--selector` and `--text` as named flags on `type` (in addition to the positional form). When both positional and named are provided, error clearly: `error: pass selector and text either positionally or via --selector/--text, not both`.
- [ ] Override the generic clap "unexpected argument" error for `type` with a tailored hint: `hint: \`type\` takes selector and text positionally — try \`ff-rdp type 'input[type=search]' 'Krankenkasse'\`. The --selector/--text flag form also works.`
- [ ] E2e tests for both invocation forms and the conflict case.

### 3. Wrap `eval` user code in an IIFE by default [3/3]

`const x = ...` in two consecutive `eval` calls fails with "redeclaration of const x" because Firefox's console actor shares a global scope across invocations. Surprising default — fix it once.

- [ ] Wrap the user-supplied JS in `(function(){ "use strict"; <user code> })()` by default. Preserve the existing return-value semantics (last expression returned; explicit `return` inside the IIFE works).
- [ ] Expressions like `1 + 1` (no statements, no `return`) must keep working. Detect the single-expression case before wrapping, or use a wrapping form that returns the trailing expression.
- [ ] Add `--no-isolate` flag to opt out (when the user *wants* to share state across calls — e.g. building up a helper across an interactive debugging session).
- [ ] Document the default + opt-out in `eval --help`.
- [ ] E2e tests: two consecutive `eval 'const x = 1; x'` calls succeed by default; with `--no-isolate` the second one errors; expressions like `eval '1 + 1'` still return `2`.

## Acceptance Criteria

- [ ] `type` works against React-style controlled inputs without manual `eval` workarounds.
- [ ] `type --selector 'sel' --text 'val'` works as a synonym for the positional form; conflict between forms is reported clearly.
- [ ] Consecutive `eval 'const x = 1; x'` calls succeed by default; `--no-isolate` preserves old shared-scope behavior.
- [ ] `eval` expression mode (no statements) continues to return the expression value.
- [ ] All quality gates pass: `cargo fmt`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace -q`.
