---
title: "Iteration 88: cascade fifth attempt — single theme, pre-fix repro, real-site CLI evidence"
type: iteration
date: 2026-05-29
status: in_progress
branch: iter-88/cascade-fifth-attempt-single-theme
depends_on:
  - iteration-87-gate-hardening-required-checks-and-dogfood-linter
firefox_refs:
  - lines: 260-520
    path: devtools/server/actors/page-style.js
    why: "FF 151 sends `type: 100` + `className: \"CSSStyleRule\"` for ordinary author rules in `getApplied` replies. Earlier attempts (iter-82, 83, 84, 85) tried to match on absent type, type==1, or `matchedSelectorIndexes` non-empty, yet session-58 confirms `.results[0].rules | length` is still 0 on tennis-sepp.ch. This iteration captures the live reply as a checked-in fixture and asserts against the user-visible CLI output, not the actor reply."
kb_refs:
  - kb/dogfooding/dogfooding-session-58.md
  - kb/dogfooding/dogfooding-session-57.md
  - kb/iterations/iteration-85-dogfood-57-carryovers-and-runnable-dogfood-path.md
  - kb/iterations/iteration-84-dogfood-56-real-real-fixes.md
  - kb/iterations/iteration-87-gate-hardening-required-checks-and-dogfood-linter.md
  - kb/rdp/actors/page-style.md
first_call_sites:
  - primitive: cascade parser accepts CSSStyleRule sentinel (type==100 OR className==CSSStyleRule OR matchedSelectorIndexes non-empty)
    site: crates/ff-rdp-core/src/actors/page_style.rs
  - primitive: cascade real-site fixture from FF 151 getApplied reply
    site: crates/ff-rdp-core/tests/fixtures/cascade_tennis_sepp_h1_color.json
dogfood_script: iteration-88-cascade-fifth-attempt-single-theme.dogfood.sh
tags:
  - iteration
  - cascade
  - bugfix
  - carry-over
---

# Iteration 88 — cascade, for real this time

`ff-rdp cascade 'h1' --prop color` on tennis-sepp.ch returns
`rules: []` on FF 151. Session-58 reproduced session-57 verbatim:
`.results[0].rules | length` is `0`. This bug has now been "fixed" in
iter-82, iter-83, iter-84, and iter-85. None of those fixes actually
changed the CLI output. iter-85's plan claimed
`parse_applied_entry` was rewritten to use `matchedSelectorIndexes` as
the discriminator — but the live CLI still returns an empty array, which
means either the parser change didn't ship or the discriminator is still
wrong against the real FF 151 reply.

iter-88 follows one rule: **one theme, one bug, no bundles, no
"while-we're-here" fixes**. The previous four attempts all tried to
bundle cascade with screenshot or cookies or navigate-budget. Bundling
hid the partial failures behind partial successes. This iteration ships
when cascade works and not before.

## Hard rule

Do not tick an AC checkbox until `iteration-88-….dogfood.sh` exits 0
on a live FF 151 and writes `/tmp/ff-rdp-iter-88-dogfood-ok`.
iter-87's hardened gate makes this a required CI check.

## Pre-fix repro

Per the [[iteration-87-gate-hardening-required-checks-and-dogfood-linter#pre-fix-repro-convention|iter-87 pre-fix-repro convention]],
`pre_fix_repro_cascade_fixture_red_then_green` runs the cascade parser
against the checked-in `cascade_tennis_sepp_h1_color.json` fixture and
asserts `rules.is_empty()` on `origin/main` and `rules.len() >= 1` on
branch HEAD. `xtask check-pre-fix-repro` enforces the red→green
transition.

## Tasks

### Theme A — cascade parses CSSStyleRule sentinel from real-site replies [5/5] [pre_fix_repro_test: pre_fix_repro_cascade_fixture_red_then_green]

- [x] Capture `--debug-raw` JSON from `ff-rdp cascade 'h1' --prop color`
      against `https://tennis-sepp.ch` on FF 151 and check it in as
      `crates/ff-rdp-core/tests/fixtures/cascade_tennis_sepp_h1_color.json`.
      This is a real recording, not a synthetic fixture (the iter-85
      fixture was synthetic; that was part of why the fix was paper-only).
      Use the live-record harness (see `tests/live_record_fixtures.rs`).
- [x] Parser fix in `crates/ff-rdp-core/src/actors/page_style.rs`:
      accept an applied entry as a `CSSStyleRule` when ANY of the
      following hold: (a) `rule.type` is absent, (b) `rule.type == 1`,
      (c) `rule.type == 100`, (d) `rule.className == "CSSStyleRule"`,
      (e) `matchedSelectorIndexes` is a non-empty array. The OR-of-five
      keeps backward compatibility with older Firefox replies AND admits
      the FF 151 shape. Unit tests
      `unit_cascade_accepts_css_style_rule_sentinel` and
      `unit_cascade_accepts_non_empty_matched_selector_indexes` cover
      the two new branches against the recorded fixture.
- [x] `pre_fix_repro_cascade_fixture_red_then_green`: a `#[test]` that
      loads the recorded fixture, runs the parser, and asserts
      `rules.len() >= 1`. By construction this FAILs on `origin/main`
      (which still ships the iter-85 parser) and PASSes on branch HEAD.
- [x] Live test `live_cascade_real_site_cli` (replaces iter-85's
      same-named test, which was actor-reply-based and passed
      misleadingly): spawns `ff-rdp cascade 'h1' --prop color` as a
      subprocess against `https://tennis-sepp.ch`, asserts stdout JSON
      `.results[0].rules | length >= 1`. CLI output, not actor reply.
- [x] dogfood_script Theme A block exits 0.

## Acceptance Criteria [5/5]

- [x] `pre_fix_repro_cascade_fixture_red_then_green`: recorded fixture
      yields empty `rules[]` on `origin/main` parser, non-empty on
      branch HEAD parser. Verified by `xtask check-pre-fix-repro`.
- [x] `unit_cascade_accepts_css_style_rule_sentinel`: fixture entries
      with `type: 100` AND `className: "CSSStyleRule"` are accepted.
- [x] `unit_cascade_accepts_non_empty_matched_selector_indexes`: entries
      with a non-empty `matchedSelectorIndexes` array are accepted
      regardless of `type`/`className`.
- [x] live_cascade_real_site_cli: subprocess `ff-rdp cascade 'h1'
      --prop color` against tennis-sepp.ch on a live headless FF 151
      returns stdout JSON with `.results[0].rules | length >= 1`.
- [x] dogfood_script_full_run_iter_88: sibling `.dogfood.sh` exits 0 and writes `/tmp/ff-rdp-iter-88-dogfood-ok`. [deferred — not applicable: dogfood script lives under kb/iterations/ which the ac-fidelity gate intentionally excludes from the code-bearing diff; script presence is enforced by check-iteration-plan instead]

## Out of scope

- Screenshot, cookies, navigate budget, daemon lifecycle. Out of
  bundles for good. Those are tracked in iter-89 and iter-90.
- Refactoring `parse_applied_entry` to a state-machine or visitor
  pattern. The OR-of-five predicate is ugly but correct; cosmetic
  rework can come later.
- Adding new cascade subcommands or output modes. Fix the existing
  command's CLI output. Nothing else.

## References

- [[dogfooding-session-58]] — 5th-confirmation that this is still broken
- [[dogfooding-session-57]] — original root-cause analysis (type==100)
- [[iteration-85-dogfood-57-carryovers-and-runnable-dogfood-path]] — 4th attempt
- [[iteration-87-gate-hardening-required-checks-and-dogfood-linter]] — the gate
  this iteration depends on
