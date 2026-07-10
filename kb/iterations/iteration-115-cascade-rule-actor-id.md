---
title: "Iteration 115: cascade entries carry rule_actor_id — distinguish same-line inline stylesheets"
type: iteration
date: 2026-07-10
status: done
branch: iter-115/cascade-rule-actor-id
depends_on:
  - kb/iterations/iteration-114-live-suite-debt-zero.md
firefox_refs: []
kb_refs: []
first_call_sites:
  - primitive: CascadeEntry.rule_actor_id (new serialized field in cascade JSON)
    site: >-
      crates/ff-rdp-cli/tests/live/live_cascade_explains_pico_dialog.rs
      (distinct-source assertion keys on the new field)
dogfood_path: |
  # Two inline <style> blocks styling the same h1; entries must be tellable apart:
  ff-rdp --port <p> navigate 'data:text/html,<style>h1{color:red}</style><style>h1{color:blue}</style><h1>x</h1>'
  ff-rdp --port <p> cascade h1 --prop color --jq '.rules[].rule_actor_id'
  # expected: two distinct non-empty actor ids
tags:
  - iteration
  - cascade
---

# Iteration 115: cascade entries carry rule_actor_id

Discovered during [[iteration-114-live-suite-debt-zero]] (Theme A diagnosis of
`live_cascade_explains_pico_dialog`): since Firefox 152, inline `<style>`
blocks report `stylesheet: null, line: 1`, so two distinct inline sheets are
indistinguishable in `cascade` output. `AppliedRule::rule_actor_id`
(crates/ff-rdp-core/src/actors/page_style.rs) is already fetched from the RDP
`rule.actor` field and already used by `styles --applied` for dedupe — but
`CascadeEntry` (crates/ff-rdp-cli/src/commands/cascade.rs) never copies it
into the cascade JSON. iter-114 worked around this in the test by keying
distinctness on `selector:specificity`; the real fix is to expose the actor id.

## Scope

- Add `rule_actor_id` to `CascadeEntry`, populated from the existing
  `AppliedRule::rule_actor_id` (no new RDP traffic, no spec drift — the field
  is already read by `styles --applied`).
- Restore `live_cascade_explains_pico_dialog`'s distinct-source assertion to
  key on `rule_actor_id` (keeping the selector:specificity check is fine as a
  secondary assertion).

## Out of scope

- Any RDP protocol/actor changes (the data is already on the wire).
- Upstream Firefox work on inline-stylesheet line reporting.

## Acceptance criteria

- [x] live_cascade_explains_pico_dialog: the two inline-`<style>` cascade
      entries carry distinct non-empty `rule_actor_id` values.
- [x] live_cascade_returns_matched_rules: cascade JSON still passes (field
      addition is backward-compatible; no other assertion drifts).

## Results

Implemented on `iter-115/cascade-rule-actor-id`.

- Added `rule_actor_id: Option<ActorId>` to `CascadeEntry`
  (`crates/ff-rdp-cli/src/commands/cascade.rs`), populated in
  `build_cascade_for_property` from the already-fetched
  `AppliedRule::rule_actor_id` and emitted by `CascadeEntry::to_json` as the
  `rule_actor_id` JSON field (always present; `null` when Firefox omits the
  `actor`). No new RDP traffic, no spec drift — the field is already read from
  the `getApplied` reply by `styles --applied`.
- `live_cascade_explains_pico_dialog` now keys its distinct-source assertion on
  `rule_actor_id` (each entry must carry a non-empty id; ≥ 2 distinct ids),
  keeping the `selector:specificity` check as a secondary assertion.
  `live_cascade_returns_matched_rules` is unchanged and still passes — the field
  addition is backward-compatible (existing assertions do not reference it).
- Non-live coverage: `unit_cascade_entries_carry_distinct_rule_actor_id` proves
  two rules that collide on selector/specificity/`stylesheet:null`/`line:1` are
  still distinguished by distinct emitted `rule_actor_id` values;
  `unit_cascade_null_rule_actor_id_serializes_as_null` proves the key is always
  present (as `null`) for a stable `--jq` shape.

Gates: `cargo fmt`, `cargo clippy --workspace --all-targets -- -D warnings`, and
`cargo test --workspace -q` all green.
