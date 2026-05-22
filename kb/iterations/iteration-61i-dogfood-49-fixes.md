---
title: "Iteration 61i: Dogfood-49 fixes — same-URL navigate, dom shape, computed flags, hint suppression"
type: iteration
date: 2026-05-22
status: planned
branch: iter-61i/dogfood-49-fixes
depends_on:
  - iteration-61h-headless-screenshot-firefox150
tags:
  - iteration
  - dogfood-fix
  - navigate
  - dom-output
  - computed
  - hints
  - agent-speed
---

# Iteration 61i: Dogfood-49 fixes

Five small, well-scoped fixes driven by [[dogfooding-session-49]] (and
sessions 47/48 — the four "regression" items below have been broken
across three consecutive dogfood sessions). All five are agent-friction
items where the current behaviour silently produces wrong/empty data or
forces every caller into the same workaround.

Out of scope (deferred to a follow-up iter-61j):
- **iter-60 ref resolution bug** — ref strings are stored as JS
  expressions (`document.querySelectorAll('tr')[0]`) and resolved as
  CSS selectors. Real fix needs a stable locator-inference layer or a
  resolution path that uses `Function('return ' + expr)()`. Big enough
  to warrant its own iteration; not a one-line tweak.
- **`screenshot --full-page`** under iter-61h's chrome-scope fallback
  returns a viewport-only PNG. Needs the deferred prepareCapture-rect
  plumbing flagged in PR #73; separate iteration.
- **CSP-blocked `eval`** on strict sites (HN, Wikipedia, admin Wardrobe).
  Real fix would need a privileged debugger-realm execution path à la
  Playwright. Out of scope here.

Themes:

- **A — Same-URL navigate no-op.** Fix the `navigate <currentUrl>`
  timeout that bites every recorder + every "navigate home then …"
  script.
- **B — `dom` always returns an array.** Eliminate the polymorphic
  `results` shape (object for 1 match, array for >1) that breaks every
  `--jq '.results[0]'` agent recipe.
- **C — `computed --prop` repeatable + accepts `--<name>`.** Multi-value
  property reads and CSS custom-property names (`--bg-color`) both work.
- **D — `--stringify` auto-suppresses hints.** When the caller signals
  "raw value extraction" via `--stringify`, don't append the human-
  friendly `-> ff-rdp …` tip line to `--format text` output.
- **E — Friction polish.** Small companion fixes uncovered along the
  way (per-step Firefox-version-warning leakage in `run`, `recipes`/
  `llm-help` references in the dogfood skill template, etc.).

## Tasks

### A. Same-URL navigate no-op

#### A1. Short-circuit `wait_for_commit` on same-URL [3/3]
- [ ] In `crates/ff-rdp-cli/src/commands/navigate.rs`,
  `wait_for_commit`: detect when `pre_nav_url` and `requested_url` refer
  to the same document (compare after stripping any URL fragment and
  any single trailing slash — keep this conservative, no full URL
  parser). When same: drop the "URL must differ" guard from the in-page
  JS so a steady-state `readyState === 'complete'` satisfies the wait
  immediately.
- [ ] Add a `fn urls_match_ignore_hash(a: &str, b: &str) -> bool`
  helper next to `capture_current_url`. Trim `#fragment` and a single
  trailing `/`. Document the conservative-comparison choice.
- [ ] Unit test the helper with: same URL, same+slash, same+hash,
  different paths, different schemes.

#### A2. Live test against a same-URL navigate [1/1]
- [ ] Add an e2e or live test: navigate to a URL twice in a row.
  Second navigate must succeed in well under `cli.timeout` and return
  `committed_url` matching the original.

### B. `dom` always returns an array

#### B1. Normalise `results` to an array [3/3]
- [ ] In `crates/ff-rdp-cli/src/commands/dom.rs`, when building the
  output envelope, always wrap the matched-elements list as a JSON
  array — even when only one element matched. Today single-match
  returns `results: { … }` (object) and multi-match returns
  `results: [ … ]` (array). Pick array unconditionally.
- [ ] Update snapshot fixtures and any e2e tests that previously
  asserted the object shape.
- [ ] Document in `kb/reference/output-formats.md` (under "Shape
  contracts") that `dom` always returns an array.

#### B2. `--first` flag for the single-result case [1/1]
- [ ] Some callers genuinely want "just the first match" without the
  array indirection. Add `--first` to `dom`: returns
  `results: <element>` (object) and `total: 1` when at least one
  element matches; returns `null` and `total: 0` when no match. Cleanly
  documented; agents who want the array can omit `--first`.

#### B3. `--jq '.results[0]'` works in both forms [1/1]
- [ ] e2e: assert `dom 'h1' --jq '.results | type'` is `"array"` after
  the fix, both for 1-match and N-match cases.

### C. `computed --prop` ergonomics

#### C1. Multi-`--prop` repeatable [2/2]
- [ ] In `crates/ff-rdp-cli/src/cli/args.rs`, change `Command::Computed`'s
  `prop` field from `Option<String>` to
  `Vec<String>` with `action = clap::ArgAction::Append` and
  `value_delimiter = ','`. Both `--prop color --prop font-size` and
  `--prop color,font-size` should work.
- [ ] Update `commands::computed::run` to iterate over the requested
  properties and return a map `{ "color": "...", "font-size": "..." }`
  instead of a single string. When the user gives zero `--prop` flags,
  fall back to today's "return the entire computed style table"
  behaviour.

#### C2. Accept CSS custom-property names like `--bg-color` [1/1]
- [ ] CSS custom properties start with `--`, which clap interprets as
  a flag prefix. Accept them via the `--prop=` form
  (`computed body --prop=--bg-color`) and document the workaround in
  `--help` long-text. The `=` form bypasses clap's flag detection.
  Add an e2e test: `--prop=--bg-color` returns the custom-property
  value when present.
- [ ] Bonus: if the parsed `--prop` value starts with `--`, special-
  case the lookup so the property is passed verbatim to
  `getPropertyValue` (no normalisation, no leading-dash stripping).

#### C3. Snapshot-test the new shape [1/1]
- [ ] Unit/snapshot test for the multi-prop output shape against a
  static `style.computed` JSON fixture.

### D. `--stringify` auto-suppresses hints

#### D1. Tie hint emission to "raw value" mode [2/2]
- [ ] In `crates/ff-rdp-cli/src/commands/eval.rs` (or wherever the hint
  decision lands — possibly `output_pipeline.rs`), treat
  `--stringify` like `--jq` and `--no-hints`: when on, don't append
  the `-> ff-rdp console …` suffix to `--format text` output.
- [ ] Add a CLI snapshot test: `eval '"x"' --stringify --format text`
  output is `"x"\n` exactly — no trailing hint line.

### E. Friction polish

#### E1. Version warning leakage from `ff-rdp run` [1/1]
- [ ] iter-61h removed the per-command Firefox-version warning. Audit
  the `ff-rdp run` execution loop: each in-process step now goes
  through `connect_and_get_target`. Confirm no per-step stderr noise
  remains and add a regression test that runs a 3-step script and
  asserts stderr is empty for the script invocation.

#### E2. Dogfood-skill template drift [1/1]
- [ ] `.claude/skills/dogfood/SKILL.md` references `scroll down`,
  `recipes`, `llm-help` — none of which are real ff-rdp subcommands.
  Replace `scroll down` with `scroll by --dy <px>`; remove or replace
  the references to `recipes` / `llm-help`. (This is a kb/skill
  edit, not Rust code.)

#### E3. Recorder pretty-printing of empty `steps[]` [1/1]
- [ ] When `record start` → no recordable commands → `record stop`
  produces a file with a literal blank line inside `"steps": [  ]`.
  Trim to `"steps": []` (single line) for cosmetic cleanliness.
  Verifies via the recorder unit tests.

## Acceptance Criteria

- [ ] `ff-rdp navigate <currentUrl>` returns successfully in under
  `cli.timeout/2` instead of timing out at 5 s. Both CLI and inside
  `ff-rdp run` paths verified.
- [ ] `ff-rdp dom <css> --jq '.results | type'` returns `"array"`
  regardless of match count.
- [ ] `ff-rdp computed body --prop color --prop background-color`
  returns a JSON object with both properties; the same data can also
  be retrieved via `--prop color,background-color`.
- [ ] `ff-rdp computed body --prop=--bg-color` returns the value of
  the `--bg-color` CSS custom property (or empty string when unset).
- [ ] `ff-rdp eval '"x"' --stringify --format text` prints exactly
  `"x"` and a single trailing newline — no hint suffix.
- [ ] `ff-rdp run <3-step.json>` produces zero stderr lines.
- [ ] `cargo fmt && cargo clippy --workspace --all-targets -- -D warnings &&
  cargo test --workspace -q` clean.
- [ ] Documentation tick: scope checkboxes in this plan reflect the
  merged PR state at review-fix time.

## Design Notes

- The same-URL navigate fix deliberately makes `navigate <currentUrl>`
  a no-op. If callers want to reload, that's what `ff-rdp reload` is
  for. Playwright's `goto(sameUrl)` reloads by default — we're
  trading a sometimes-useful reload for an always-painful timeout.
  The trade-off favours agents, who can call `reload` explicitly when
  needed.
- `dom`'s polymorphic shape exists for backwards-compat with the
  pre-iter-60 single-result shape. v0.1.0 is pre-stable so this is
  the right window to normalise. `--first` covers the legitimate
  single-result use case.
- Deferred items (ref resolution, --full-page, CSP eval) are
  intentionally one-iteration-per-fix because each one needs real
  thought, not a small patch.

## References

- [[dogfooding-session-49]] — surfaced all four regressions
- [[dogfooding-session-48]] — earlier surfacing of A, B, C
- [[dogfooding-session-47]] — earliest surfacing of A
- [[iteration-61h-headless-screenshot-firefox150]] — predecessor;
  iter-61i builds on the same `main` post-merge.
