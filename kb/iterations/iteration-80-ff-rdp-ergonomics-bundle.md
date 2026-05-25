---
title: "Iteration 80: ff-rdp ergonomics bundle — help grouping, reload --hard, eval --unwrap, dom --include-style, a11y --critical"
type: iteration
date: 2026-05-25
status: planned
branch: iter-80/ff-rdp-ergonomics-bundle
depends_on:
  - iteration-77-spec-drift-and-windows-reparse-points
firefox_refs:
  - path: devtools/server/actors/styles.js
    lines: "1-120"
    why: "PageStyle.getComputed / getApplied — backs the new `dom --include-style` per-match getComputedStyle round-trip (Theme D).  Confirms the response shape we re-use from `ff-rdp computed`."
kb_refs:
  - kb/rdp/actors/styles.md
  - kb/rdp/from-our-codebase/open-gaps.md
first_call_sites: []
dogfood_path: |
  # A — help grouping
  ff-rdp --help | head -40                                     # must show Inspect / Navigate / Trace sections
  # B — reload --hard
  ff-rdp launch --auto-consent --headless
  ff-rdp navigate https://example.com
  ff-rdp reload --hard                                         # bypasses HTTP cache
  # C — eval --unwrap
  ff-rdp eval 'JSON.stringify({a:1})' --unwrap | jq .a         # returns {"a":1} unwrapped, jq sees 1
  # D — dom --include-style
  ff-rdp dom 'p' --include-style color,display                 # each match carries a `style` field
  # E — a11y --critical
  ff-rdp a11y --critical                                       # only WCAG violations, not the full aria tree
  pkill -f 'firefox.*ff-rdp-profile'
tags: [iteration, cli-help, ergonomics, dogfood]
---

Five small CLI-ergonomics fixes bundled from the 2026-05-25 dogfooding
report.  All are additive and independent; one branch keeps the PR small
and reviewable.

## Tasks

### Theme A — help grouping
- [ ] Restructure the top-level `--help` text in
      `crates/ff-rdp-cli/src/cli/args.rs` into sections:
      **Inspect** (`dom`, `styles`, `computed`, `a11y`, `snapshot`,
      `page-text`, `perf`),
      **Navigate** (`navigate`, `reload`, `click`, `type`, `screenshot`),
      **Trace** (`console`, `network`, `eval`),
      **Lifecycle** (`launch`, `daemon`).
      Implement via clap's `help_heading` per-subcommand or a hand-rolled
      `after_help`/`long_about` on the root, whichever yields the cleanest
      output.

### Theme B — reload --hard
- [ ] Add `--hard` flag to `ff-rdp reload` that sets the RDP `force_cache`
      / `LoadFlags::BYPASS_CACHE` equivalent so the request bypasses
      HTTP cache.  Default remains soft reload.

### Theme C — eval --unwrap
- [ ] Add `--unwrap` to `ff-rdp eval`: when the result value is a string
      that round-trips through `serde_json::from_str` as a JSON object or
      array, replace `results` with the parsed value before printing.
      No magic when the string is not valid JSON.

### Theme D — dom --include-style
- [ ] Add `--include-style <PROPS>` to `ff-rdp dom` (comma-separated CSS
      property list).  For each matched node, attach a `style` field
      with the named getComputedStyle values.  Re-use the
      `WebConsoleActor::evaluate_js` + `getComputedStyle` path that
      `ff-rdp computed` already exercises, or call PageStyle.getComputed
      per match — whichever is cheaper for N matches.
- [ ] If the round-trip becomes expensive on large match sets, add a
      `--include-style-limit <N>` guard (default 50) and document it in
      `long_about`.

### Theme E — a11y --critical
- [ ] Add `--critical` to `ff-rdp a11y` that filters the output to nodes
      with a WCAG violation (use whatever the existing a11y actor surfaces
      as severity / audit-result).  Empty array when nothing is critical.

## Acceptance Criteria [0/5]

- [ ] `cli_help_groups_commands_by_role`: a unit/e2e test in
      `crates/ff-rdp-cli/tests/` runs `ff-rdp --help` and asserts the
      output contains the section headers `Inspect`, `Navigate`,
      `Trace`, `Lifecycle` (case-insensitive) — proving the grouping
      survived a clap refactor.
- [ ] `reload_hard_bypasses_cache`: live test
      `crates/ff-rdp-cli/tests/live_reload_hard.rs::live_reload_hard_bypasses_cache`
      that loads a page with a `Cache-Control: public, max-age=3600`
      asset, reloads with `--hard`, and asserts the request is re-issued
      to the origin (e.g. by observing the network actor or by serving
      a counter from a local fixture).  Gated `FF_RDP_LIVE_TESTS=1`.
- [ ] `eval_unwrap_parses_json_string`: unit test
      `crates/ff-rdp-cli/src/commands/eval.rs::tests::eval_unwrap_parses_json_string`
      that runs `eval` with `--unwrap` on a result whose string value is
      `"{\"a\":1}"` and asserts `results == {"a": 1}`.  Negative case
      with `"hello"` asserts the result is left unchanged.
- [ ] `dom_include_style_attaches_computed_values`: live test
      `crates/ff-rdp-cli/tests/live_dom_include_style.rs::dom_include_style_attaches_computed_values`
      that navigates to a fixture page with `<p style="color:red">` and
      runs `dom 'p' --include-style color`; asserts each result element
      has `style.color == "rgb(255, 0, 0)"`.  Gated `FF_RDP_LIVE_TESTS=1`.
- [ ] `a11y_critical_filters_to_violations`: live test
      `crates/ff-rdp-cli/tests/live_a11y_critical.rs::a11y_critical_filters_to_violations`
      on a fixture page with a known WCAG violation (e.g. an `<img>` with
      no alt); asserts `a11y --critical` returns exactly the offending
      node and an empty result on a clean page.  Gated `FF_RDP_LIVE_TESTS=1`.

## Out of scope

- The `launch --url` one-shot — user declined again.
- Default `navigate --wait` level — stays `dom-complete`; iter-79 fixes
  the subscription bug so the existing default works on real pages.
- The cascade inspector — filed separately as iter-81.

## References

- [[iteration-79-navigate-readiness-and-dom-help-discoverability]] — sibling
  ergonomics work, kept separate because iter-79 is a bug fix and this
  is feature polish.
- Dogfooding report (2026-05-25, in-session chat).
