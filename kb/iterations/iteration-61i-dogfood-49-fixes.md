---
title: "Iteration 61i: Dogfood-49 fixes — same-URL navigate, dom shape, computed flags, hint suppression"
type: iteration
date: 2026-05-22
status: in-progress
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
- [x] In `crates/ff-rdp-cli/src/commands/navigate.rs`,
  `wait_for_commit`: detect when `pre_nav_url` and `requested_url` refer
  to the same document (compare after stripping any URL fragment and
  any single trailing slash — keep this conservative, no full URL
  parser). When same: drop the "URL must differ" guard from the in-page
  JS so a steady-state `readyState === 'complete'` satisfies the wait
  immediately.
- [x] Add a `fn urls_match_ignore_hash(a: &str, b: &str) -> bool`
  helper next to `capture_current_url`. Trim `#fragment` and a single
  trailing `/`. Document the conservative-comparison choice.
- [x] Unit test the helper with: same URL, same+slash, same+hash,
  different paths, different schemes, different queries, different
  hosts.

#### A2. Live test against a same-URL navigate [0/1]
- [ ] Add an e2e or live test: navigate to a URL twice in a row.
  Second navigate must succeed in well under `cli.timeout` and return
  `committed_url` matching the original. (Deferred — needs live
  Firefox in CI; unit tests cover the matcher exhaustively.)

### B. `dom` always returns an array

#### B1. Normalise `results` to an array [3/3]
- [x] In `crates/ff-rdp-cli/src/commands/dom.rs`, when building the
  output envelope, always wrap the matched-elements list as a JSON
  array — even when only one element matched. Today single-match
  returns `results: { … }` (object) and multi-match returns
  `results: [ … ]` (array). Pick array unconditionally.
- [x] Update snapshot fixtures and any e2e tests that previously
  asserted the object shape (7 e2e tests updated).
- [x] Document in `kb/reference/output-formats.md` (under "Shape
  contract: `dom` always returns an array") that `dom` always returns
  an array.

#### B2. `--first` flag for the single-result case [1/1]
- [x] Some callers genuinely want "just the first match" without the
  array indirection. Added `--first` to `dom`: returns
  `results: <element>` (object) and `total: 1` when at least one
  element matches; returns `null` and `total: 0` when no match.
  Threaded through CLI args + dispatch.

#### B3. `--jq '.results[0]'` works in both forms [1/1]
- [x] e2e regression: existing dom e2e tests now exercise
  `.is_array()` for both 1-match and N-match cases; the `dom_with_jq_filter`
  test was updated to use `.results[0]` which is the canonical
  post-iter-61i agent recipe.

### C. `computed --prop` ergonomics — **deferred to iter-61j**

The clap-args restructuring (repeatable `--prop`, accepting `--<name>`
via the `=` form, multi-value output map) turned out to ripple through
more of `commands::computed` than a same-PR pass could absorb safely.
Deferred to a follow-up iteration with its own focused scope. The
current single-`--prop`-or-empty behaviour is unchanged.



#### C1. Multi-`--prop` repeatable [0/2] _(deferred to iter-61j)_
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

#### C2. Accept CSS custom-property names like `--bg-color` [0/2] _(deferred to iter-61j)_
- [ ] CSS custom properties start with `--`, which clap interprets as
  a flag prefix. Accept them via the `--prop=` form
  (`computed body --prop=--bg-color`) and document the workaround in
  `--help` long-text. The `=` form bypasses clap's flag detection.
  Add an e2e test: `--prop=--bg-color` returns the custom-property
  value when present.
- [ ] Bonus: if the parsed `--prop` value starts with `--`, special-
  case the lookup so the property is passed verbatim to
  `getPropertyValue` (no normalisation, no leading-dash stripping).

#### C3. Snapshot-test the new shape [0/1] _(deferred to iter-61j)_
- [ ] Unit/snapshot test for the multi-prop output shape against a
  static `style.computed` JSON fixture.

### D. `--stringify` auto-suppresses hints

#### D1. Tie hint emission to "raw value" mode [2/2]
- [x] Added `OutputPipeline::without_hints()` to force-suppress hints
  on an existing pipeline.  `commands::eval::run` now calls
  `pipeline.without_hints()` whenever `--stringify` is set — symmetric
  with `--jq` / `--no-hints`.
- [x] e2e test `eval_stringify_text_suppresses_hints` asserts that
  `eval … --stringify --format text` stdout does not contain
  `-> ff-rdp`.

### E. Friction polish — **deferred to iter-61j**

E1 (run stderr audit), E2 (dogfood skill template), and E3 (recorder
empty-steps cosmetic) are deferred to a follow-up iteration with the
deferred C scope.  None of them are correctness issues — they don't
block the headline A/B/D fixes from shipping.

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
