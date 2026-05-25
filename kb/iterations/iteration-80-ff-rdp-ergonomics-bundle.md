---
title: "Iteration 80: ff-rdp ergonomics bundle — help grouping, reload --hard, eval --unwrap, dom --include-style, a11y --critical"
type: iteration
date: 2026-05-25
status: done
branch: iter-80/ff-rdp-ergonomics-bundle
depends_on:
  - iteration-77-spec-drift-and-windows-reparse-points
firefox_refs:
  - lines: 1-120
    path: devtools/server/actors/page-style.js
    why: >-
      PageStyle.getComputed / getApplied — backs the new `dom --include-style`
      per-match getComputedStyle round-trip (Theme D).  Confirms the response shape we re-use
      from `ff-rdp computed`.
kb_refs:
  - kb/rdp/actors/styles.md
  - kb/rdp/from-our-codebase/open-gaps.md
first_call_sites:
  - "WindowGlobalTarget::reload (force param): crates/ff-rdp-cli/src/commands/nav_action.rs"
  - "commands::a11y::run_critical: crates/ff-rdp-cli/src/dispatch.rs"
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
tags:
  - iteration
  - cli-help
  - ergonomics
  - dogfood
---

Five small CLI-ergonomics fixes bundled from the 2026-05-25 dogfooding
report.  All are additive and independent; one branch keeps the PR small
and reviewable.

## Tasks

### Theme A — help grouping
- [x] Restructure the top-level `--help` text in
      `crates/ff-rdp-cli/src/cli/args.rs` into sections:
      **Inspect** (`dom`, `styles`, `computed`, `a11y`, `snapshot`,
      `page-text`, `perf`),
      **Navigate** (`navigate`, `reload`, `click`, `type`, `screenshot`),
      **Trace** (`console`, `network`, `eval`),
      **Lifecycle** (`launch`, `daemon`).
      Implemented as a hand-rolled grouping block prepended to the root
      `about`/`long_about` so the section names appear at the top of
      `ff-rdp --help` regardless of clap's subcommand listing order.

### Theme B — reload --hard
- [x] Add `--hard` flag to `ff-rdp reload` that sets the RDP
      `options.force` (Firefox's `LoadFlags::BYPASS_CACHE` equivalent)
      so the request bypasses HTTP cache. Default remains soft reload.
      `WindowGlobalTarget::reload(transport, target, force)` gained the
      `force: bool` parameter; the wait-idle paths reuse `build_reload_packet`
      to attach `options.force` consistently.

### Theme C — eval --unwrap
- [x] Add `--unwrap` to `ff-rdp eval`: when the result value is a string
      that round-trips through `serde_json::from_str` as a JSON object or
      array, replace `results` with the parsed value before printing.
      Extracted into `try_unwrap_json_string` so the behaviour is unit-tested
      end-to-end. Non-JSON strings and JSON primitives are left unchanged.

### Theme D — dom --include-style
- [x] Add `--include-style <PROPS>` to `ff-rdp dom` (comma-separated CSS
      property list). For each matched ARIA-tree node, attach a `style` field
      with the named getComputedStyle values via a second JS roundtrip that
      mirrors the `ff-rdp computed` build_js path (`props.length` loop with
      `cs.getPropertyValue` + bracket fallback).
- [x] `--include-style-limit <N>` guard (default 50) caps the per-call cost
      on large match sets; truncated runs set `meta.style_truncated: true`
      and `meta.style_limit`.

### Theme E — a11y --critical
- [x] Add `--critical` to `ff-rdp a11y` that surfaces nodes that fail a
      basic WCAG audit (img without alt, button/link/form controls without
      an accessible name). Returns a flat array of `{role, selector,
      violation, severity}` records; empty when nothing is critical.

## Acceptance Criteria [5/5]

- [x] `cli_help_groups_commands_by_role` (crates/ff-rdp-cli/tests/cli_help_groups.rs): a unit/e2e test
      runs `ff-rdp --help` and asserts the output contains the section
      headers `Inspect`, `Navigate`, `Trace`, `Lifecycle`
      (case-insensitive) — proving the grouping survived a clap refactor.
- [x] `live_reload_hard_bypasses_cache` (crates/ff-rdp-cli/tests/live_reload_hard.rs):
      loads a page served with `Cache-Control: public, max-age=3600` from
      a local counting HTTP server, reloads with `--hard`, asserts the
      response carries `force: true` and the server observes a second
      origin hit. Gated `FF_RDP_LIVE_TESTS=1`.
- [x] `eval_unwrap_parses_json_string`
      (crates/ff-rdp-cli/src/commands/eval.rs::tests::eval_unwrap_parses_json_string):
      exercises `try_unwrap_json_string` on `{"a":1}` and asserts the
      structured object is returned. Companion tests cover plain strings,
      JSON primitives, arrays, and non-string values.
- [x] `dom_include_style_attaches_computed_values`
      (crates/ff-rdp-cli/tests/live_dom_include_style.rs): navigates to
      a data URL with two `<p style="color:red">` nodes, runs
      `dom 'p' --include-style color`, and asserts every match has
      `style.color == "rgb(255, 0, 0)"`. Gated `FF_RDP_LIVE_TESTS=1`.
- [x] `a11y_critical_filters_to_violations`
      (crates/ff-rdp-cli/tests/live_a11y_critical.rs): on a fixture page
      with `<img>` missing alt, asserts `a11y --critical` returns the
      offending node; clean page returns an empty array. Gated
      `FF_RDP_LIVE_TESTS=1`.

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
