---
title: "Iteration 114: live-suite debt zero — Firefox-152 color drift, self-launching legacy tests, self-hosted fixture server"
type: iteration
date: 2026-07-10
status: planned
branch: iter-114/live-suite-debt-zero
depends_on:
  - kb/iterations/iteration-110-post-batch-live-sweep.md
  - kb/iterations/iteration-113-live-launch-hardening.md
firefox_refs: []
kb_refs:
  - kb/iterations/iteration-106-live-test-masking-cascade.md
first_call_sites: []
dogfood_path: |
  # Representative fixed test from each category, then the sweep:
  FF_RDP_LIVE_TESTS=1 cargo test -p ff-rdp-cli --test live live_cascade_returns_matched_rules -- --include-ignored
  FF_RDP_LIVE_TESTS=1 cargo test -p ff-rdp-cli --test live live_index_local_fixture -- --include-ignored
  FF_RDP_LIVE_TESTS=1 FF_RDP_LIVE_NETWORK_TESTS=1 cargo test-live
  # expected: 0 unexplained failures (every remaining red carries an explicit
  # justification in this plan's Results)
tags:
  - iteration
  - tests
  - live-suite
---

# Iteration 114: live-suite debt zero

The [[iteration-110-post-batch-live-sweep]] Results inventory leaves exactly
22 pre-existing live reds, in three root-cause categories. This iteration
retires all three, so that a full sweep is either green or every red carries
an explicit, reviewed justification — restoring the live suite as a trustable
regression net instead of a "known noise" pile.

## Execution policies (standing, per James)

Live tests ARE this iteration's deliverable: run the specific tests being
fixed (filtered) while iterating; run the full live sweep exactly ONCE at the
end — it is the primary AC evidence (this is a single-iteration batch, so the
end-of-iteration sweep IS the batch sweep). Scoped testing: affected tests
only during development; one full `cargo test --workspace -q` in the final
pre-PR gates; the review agent does not re-run the full workspace suite.
CI-wait: merge on required lanes; `test (windows-latest)` is expected GREEN
— any windows failure is real and blocks. `live-tests` (CI lane) stays
advisory but should improve visibly with this diff.

## Theme A — Firefox-152 computed-color drift (4 tests)

Firefox 152 serializes computed colors as keywords (`red`) where older
versions returned `rgb(255, 0, 0)`; the four cascade tests assert the old
form. Add a canonical-color comparison helper to the live-test `common`
module (parse keyword/hex/rgb() to one canonical form; unit-test the helper's
equivalences) and convert the four tests to it — assertions survive future
serialization drift in either direction:
`live_cascade::live_cascade_returns_matched_rules`,
`live_cascade::live_cascade_returns_matched_rules_external_css`,
`live_95_cascade_computed_agreement::live_cascade_inherited_or_default_note_fires_on_h1_color`,
`live_95_cascade_computed_agreement::pre_fix_repro_cascade_prop_populates_computed_when_standalone_computed_does`.

## Theme B — legacy tests must self-launch (16 tests)

These predate the self-launch harness: they assume an already-running Firefox
on port 6000 (fail: nothing listens) or navigate to real external sites
(flake). For each: port it to the modern `common::LiveFirefox` self-launch
harness with `data:` URLs / local content where feasible (preferred), or —
where its coverage is demonstrably duplicated by a modern test — retire it
with a one-line justification in this plan's Results mapping old coverage to
the surviving test. Zero silent deletions. The list:
`live_98_media_query_truthfulness::pre_fix_repro_cascade_winner_ignores_media_context`,
`live_a11y_contrast_wai_bad`, `live_cascade_explains_pico_dialog`,
`live_cascade_real_site::live_cascade_real_site_cli`,
`live_cascade_real_site::live_cascade_real_site_returns_rules_for_a_element`,
`live_console_printf::live_console_printf_e2e`,
`live_cookies_set_cookie_header`, `live_dom_stats_perf_audit_parity`,
`live_navigate_default_fast::live_navigate_default_fast_no_budget_exhaustion`,
`live_navigate_default_fast::live_navigate_global_timeout_flag_accepted`,
`live_screenshot_ff151::live_screenshot_ff151_cli`,
`live_screenshot_ff151::live_screenshot_ff151_produces_valid_png`,
`live_screenshot_bulk_fallback::live_screenshot_bulk_fallback_then_eval`,
`live_stale_tab_race::live_stale_tab_race_no_such_actor_after_navigate`,
`live_styles_applied_dedupe::live_styles_applied_dedupe_no_duplicate_actor_ids`,
`live_wait_timeout_ms_canonical::live_wait_timeout_ms_canonical_flag`.
Real-site coverage that is genuinely valuable (e.g. one cascade-on-real-site
smoke) may keep a single representative network-gated test behind
`FF_RDP_LIVE_NETWORK_TESTS=1`; the rest move to local content.

## Theme C — self-hosted fixture server (2 tests)

`live_62_page_map_index::{live_index_local_fixture,live_runner_page_map_resolution}`
self-skip because they expect an uncommitted server at `http://localhost:18080`.
Give the live `common` module a minimal std-only static HTTP server
(`TcpListener` on an **ephemeral** port — never a fixed one; parallel-safe;
serves an in-source fixture map) that the tests start themselves, and inject
its URL. No new dependencies, no polyglot tooling, all Rust.

## Out of scope

- The CI `live-tests` lane's advisory status (unchanged).
- New product features; this is test-infrastructure debt only.
- If Theme B proves larger than one iteration, its remainder is droppable to
  a filed carry-over plan BEFORE merge (iter-104 precedent) — Themes A and C
  are not droppable.

## Acceptance criteria

- [ ] color_drift_normalized: the four Theme A tests pass live on Firefox 152
      via the canonical-color helper; helper unit tests cover
      keyword/hex/rgb() equivalence both directions.
- [ ] fixture_server_selfhosted: both live_62 tests pass live with the
      self-started ephemeral-port fixture server (no localhost:18080
      dependency, no self-skip path taken).
- [ ] legacy_tests_selflaunching: every Theme B test either passes live under
      the self-launch harness or is retired with a coverage-mapping
      justification line in Results; zero remaining port-6000 assumptions
      (grep evidence: no `6000` literal in tests/live/ outside comments).
- [ ] sweep_zero_unexplained: final full sweep
      (`FF_RDP_LIVE_TESTS=1 FF_RDP_LIVE_NETWORK_TESTS=1 cargo test-live`)
      recorded in Results with 0 failures, or every failure carrying an
      explicit justification + cross-reference.

## Results

(to be filled by the implementing iteration)
