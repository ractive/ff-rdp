---
title: "Iteration 81: Cascade inspector — explain why a CSS property wins for an element"
type: iteration
date: 2026-05-25
status: done
branch: iter-81/cascade-inspector
depends_on:
  - iteration-77-spec-drift-and-windows-reparse-points
firefox_refs:
  - lines: 1-400
    path: devtools/server/actors/page-style.js
    why: >-
      PageStyle.getMatchedSelectors / getApplied — returns the ordered list of CSS
      rules that match an element, plus origin (UA / user / author), media, and parent
      stylesheet.  This is the backbone of the cascade view.
  - lines: 1-150
    path: devtools/server/actors/style-rule.js
    why: >-
      StyleRuleActor — represents a single CSS rule (selector text, declarations,
      stylesheet, line).  Needed to render per-rule output.
  - lines: 1-120
    path: devtools/shared/specs/page-style.js
    why: >-
      PageStyle spec — confirms the request/response shape for getMatchedSelectors
      and getApplied; required for an actor binding.
kb_refs:
  - kb/rdp/actors/styles.md
  - kb/rdp/from-our-codebase/open-gaps.md
first_call_sites:
  - primitive: ff_rdp_core::actors::page_style::PageStyleActor
    site: >-
      crates/ff-rdp-cli/src/commands/cascade.rs (issues getMatchedSelectors per
      element)
  - primitive: ff_rdp_core::css::specificity::compute
    site: >-
      crates/ff-rdp-cli/src/commands/cascade.rs (ranks the matched rules for the
      winner-explanation column)
dogfood_path: |
  ff-rdp launch --auto-consent --headless
  ff-rdp navigate https://example.com
  # Show why `display` on the first <dialog> resolves the way it does.
  ff-rdp cascade 'dialog' --prop display
  # Expected: a JSON array of matching rules in cascade order, each with
  # selector, specificity, origin, stylesheet:line, declaration value,
  # and a final `winner: true` on the rule whose declaration was used.
  pkill -f 'firefox.*ff-rdp-profile'
tags:
  - iteration
  - css
  - page-style
  - feature
---

Killer-feature ask from the 2026-05-25 dogfooding report.  The current
`ff-rdp computed` and `ff-rdp styles` commands answer "what is the value?"
and "what rules apply?", but not "*why* did this rule win?"  Cascade
debugging today means manually walking matched rules and computing
specificity in your head.

This iteration adds `ff-rdp cascade <selector> --prop <name>` which returns
the ordered list of matching rules with origin (UA / user / author / inline),
media query, stylesheet:line, selector specificity, declaration value, and
the `!important` flag — culminating in a `winner: true` marker on the rule
whose declaration is the computed value.

## Tasks

- [x] **Core actor binding.** Extended `AppliedRule` in
      `crates/ff-rdp-core/src/actors/page_style.rs` with `matched_selectors`
      and `media`, so `getApplied` (with `matchedSelectors: true`) carries
      the cascade-relevant fields.  No new method; `get_applied` already
      issues the wire request used here.
- [x] **Specificity calculator.** Added
      `crates/ff-rdp-core/src/css/specificity.rs` implementing CSS
      Selectors Level 4 specificity, including `:is/:not/:has/:where`
      and pseudo-element handling.  22 unit tests cover the W3C examples.
- [x] **Cascade command.** Added
      `crates/ff-rdp-cli/src/commands/cascade.rs` with
      `cascade <SEL> [--prop NAME | --all]`.  Output shape:
      ```json
      {
        "results": [{
          "ref": "e1", "selector": "dialog",
          "property": "display",
          "computed": "flex",
          "rules": [
            {"selector": "dialog", "specificity": [0,0,1],
             "origin": "author", "media": null,
             "stylesheet": "pico.css", "line": 88,
             "value": "block", "important": false, "winner": false},
            {"selector": "dialog#lightbox", "specificity": [1,0,1],
             "origin": "author", "media": null,
             "stylesheet": "site.css", "line": 142,
             "value": "flex", "important": false, "winner": true}
          ]
        }],
        "total": 1
      }
      ```
- [x] **Help text.** Added `cascade <SEL> [--prop NAME | --all]` to the
      `Inspect` group in the top-level help (`AFTER_LONG_HELP`) and to
      the command-list comment.
- [x] **Recorded fixture.** Added
      `live_page_style_get_applied_cascade` in
      `crates/ff-rdp-core/tests/live_record_fixtures.rs` — re-recording
      produces
      `crates/ff-rdp-cli/tests/fixtures/page_style_get_applied_cascade_response.json`
      via `FF_RDP_LIVE_TESTS_RECORD=1`.  Unit tests parse the same
      response shape directly without requiring the live capture, so the
      AC tests run in the standard `cargo test --workspace` flow.

## Acceptance Criteria [3/3]

- [x] `cascade_marks_winner_on_higher_specificity` (unit, in
      `crates/ff-rdp-cli/src/commands/cascade.rs`): given two rules where
      the higher-specificity rule sets `display: flex` and the lower sets
      `display: block`, `cascade` returns `winner: true` on the
      higher-specificity rule and the `computed` field matches.
- [x] `cascade_important_overrides_specificity` (unit, in
      `crates/ff-rdp-cli/src/commands/cascade.rs`): given a rule where a
      lower-specificity rule has `!important` and a higher-specificity
      rule does not, the `!important` rule is the winner.  Verifies the
      CSS cascade origin/importance step is respected, not just
      specificity.
- [x] `live_cascade_explains_pico_dialog` (live, in
      `crates/ff-rdp-cli/tests/live_cascade_explains_pico_dialog.rs`):
      on a real page that loads two stylesheets — a base rule on
      `dialog` and an author override on `dialog#lightbox` — `ff-rdp
      cascade 'dialog#lightbox' --prop display` returns at least two
      rules with distinct sources and the winner field matches the value
      `ff-rdp computed 'dialog#lightbox' --prop display` returns.  Gated
      `FF_RDP_LIVE_TESTS=1`.

## Out of scope

- Style mutation (`add-rule`, `edit-declaration`).  Read-only cascade
  view only.
- Pseudo-element cascades (`::before`, `::after`).  Element rules only
  in this iter; pseudos can be a follow-up.
- Animations / transitions origin in the cascade.  Static rules only.
- A human-readable explanation string ("Pico's `dialog` wins over your
  `dialog#lightbox` because…").  This iter returns the structured JSON;
  any prose layer is a follow-up.

## References

- CSS Selectors Level 4 — specificity algorithm.
- [[iteration-80-ff-rdp-ergonomics-bundle]] — owns the help-grouping
  refactor that `cascade` plugs into.
- Dogfooding report (2026-05-25, in-session chat) — #7 "killer-feature
  for CSS debugging".
