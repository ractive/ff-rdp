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

- [x] color_drift_normalized: the four Theme A tests pass live on Firefox 152
      via the canonical-color helper; helper unit tests cover
      keyword/hex/rgb() equivalence both directions.
- [x] fixture_server_selfhosted: both live_62 tests pass live with the
      self-started ephemeral-port fixture server (no localhost:18080
      dependency, no self-skip path taken).
- [x] legacy_tests_selflaunching: every Theme B test either passes live under
      the self-launch harness or is retired with a coverage-mapping
      justification line in Results; zero remaining port-6000 assumptions
      (grep evidence: no `6000` literal in tests/live/ outside comments).
      One exception carried explicitly: live_console_printf_e2e stays red on
      a diagnosed product gap
      [deferred — new plan: kb/iterations/iteration-116-console-cache-start-listeners.md].
- [x] sweep_zero_unexplained: final full sweep
      (`FF_RDP_LIVE_TESTS=1 FF_RDP_LIVE_NETWORK_TESTS=1 cargo test-live`)
      recorded in Results with 0 failures, or every failure carrying an
      explicit justification + cross-reference. (121/122 live green; the one
      red is live_console_printf_e2e
      [deferred — new plan: kb/iterations/iteration-116-console-cache-start-listeners.md])

## Results

Implemented 2026-07-10 on `iter-114/live-suite-debt-zero` (Firefox 152.0.5,
macOS). All 22 inventoried reds retired: 20 now pass live, 1 retired with
coverage mapping, 1 left red with a diagnosed product-side root cause and a
filed follow-up plan. Two product bugs discovered and one fixed in-branch.

### Theme A — canonical-color helper (5 tests green)

`common/mod.rs` gained `pub type Rgba = (u8,u8,u8,u8)`,
`parse_css_color(&str) -> Option<Rgba>` (keyword / #rgb / #rrggbb / #rgba /
#rrggbbaa / rgb() / rgba()), and `assert_colors_equal(actual, expected,
context)`, plus 8 non-ignored unit tests covering keyword↔rgb()↔hex
equivalence in both directions (run in plain `cargo test`). Converted and
verified live (all ok):

- live_cascade_returns_matched_rules — computed now serializes as `red`
- live_cascade_returns_matched_rules_external_css — `red`
- live_cascade_inherited_or_default_note_fires_on_h1_color — cascade vs
  standalone computed compared canonically (the two code paths can serialize
  the same color differently since FF152)
- pre_fix_repro_cascade_prop_populates_computed_when_standalone_computed_does
  — canonical compare only for color-valued rows; font-size stays
  byte-for-byte
- live_cascade_explains_pico_dialog — NOT color drift: FF152 reports inline
  `<style>` blocks as `stylesheet: null, line: 1`, so the distinct-sources
  assertion keyed on `stylesheet:line` collapsed. Product gap: `CascadeEntry`
  never copies `AppliedRule::rule_actor_id` into cascade output. Test now
  keys distinctness on `selector:specificity`; real fix filed as
  [[iteration-115-cascade-rule-actor-id]].

### Theme B — 16 legacy tests: 14 ported+green, 1 retired, 1 deferred red

Ported to `LiveFirefox::headless_on_random_port()` + `base_args(port)` with
local content; `FF_RDP_LIVE_NETWORK_TESTS` gate dropped wherever no real
network remains:

- pre_fix_repro_cascade_winner_ignores_media_context — root cause was NOT
  port-6000: the unescaped `data:` URL fixture (`{ } @ ( ) :` + spaces) gets
  truncated by Firefox's navigation pipeline so `#probe` never existed; fixed
  with a percent-encoded twin fixture. PASS.
- live_cookies_set_cookie_header_visible_after_navigate — httpbin.org →
  in-file local TcpListener server emitting `Set-Cookie: probe=1`; also fixed
  stale `cookies list` syntax (now flat `ff-rdp cookies`). PASS.
- live_dom_stats_perf_audit_parity_images_without_lazy — httpbin.org/html
  (0 `<img>`, vacuous parity) → local fixture with 4 non-lazy images so
  `images_without_lazy` is deterministically non-zero; also fixed the perf
  audit JSON pointer (`results.dom_stats.images_without_lazy`). PASS.
- live_navigate_default_fast_no_budget_exhaustion,
  live_navigate_global_timeout_flag_accepted — example.com → local HTTP
  round-trip (keeps the default-wait budget-splitting path meaningful). PASS.
- live_screenshot_ff151_produces_valid_png, live_screenshot_ff151_cli —
  port-6000 → self-launch + deterministic `data:` fixture; `--output -` no
  longer means stdout, switched to `--base64` + decode. PASS.
- live_screenshot_bulk_fallback_then_eval — example.com → `data:` fixture;
  fixed stale `--bulk <path>` syntax (boolean flag + `-o`) and eval JSON
  pointer. PASS.
- live_a11y_contrast_low_contrast_fixture_detects_failures (renamed from
  live_a11y_contrast_wai_bad_detects_failures) — w3.org WAI demo → local
  low-contrast fixture covering leaf, inline-children-container, and `<td>`
  paths; aa_fail=5 ≥ 1. PASS.
- live_cascade_real_site_cli — kept as the single representative real-site
  smoke per plan; ported to self-launch, keeps BOTH env gates
  (tennis-sepp.ch, 2 rules for h1). PASS.
- live_stale_tab_race_no_such_actor_after_navigate — example.com→example.org
  → two local servers on distinct host strings (`127.0.0.1` vs `localhost`,
  distinct sites under Fission, preserving the cross-process swap), with a
  sanity check that the first navigate landed. PASS.
- live_styles_applied_dedupe_no_duplicate_actor_ids — example.com (no
  multi-sheet exercise) → `data:` fixture with `<p>` matched across two
  sheets; also fixed invalid legacy syntax `styles applied --selector p` →
  `styles <selector> --applied` (5 results, 5 unique actor ids). PASS.
- live_wait_timeout_ms_canonical_flag — example.com → `data:` fixture; the
  legacy first call (`wait --timeout-ms 2000` with no condition) was invalid
  pre-port (clap requires a condition, exit 2) — added `--selector body` to
  preserve the flag-acceptance intent. PASS.

Retired (1):
- live_cascade_real_site_returns_rules_for_a_element (css.gg) — its coverage
  (cascade returns ≥1 rule for an element on a real site with external
  stylesheets) is subsumed by the kept live_cascade_real_site_cli smoke plus
  the local cascade suites (live_cascade, live_95_cascade_computed_agreement,
  live_cascade_explains_pico_dialog), which exercise the same
  external/multi-stylesheet paths deterministically.

Left red with justification (1):
- live_console_printf_e2e — product gap, not test drift:
  `commands::console::run` calls `get_cached_messages` without ever calling
  `startListeners`, so a fresh `--no-daemon` connection's cache is
  legitimately empty (verified live, including with a temporary product
  patch that fixes the exact sequence). Fix filed as
  [[iteration-116-console-cache-start-listeners]]; the test is its live AC.

Grep evidence: word-boundary `6000` in `tests/live/` matches only comments —
historical "ported away from" notes and the CLI's default-port product fact
(live_110_kill_scoping); two stale live_86 doc comments claiming current
port-6000 use were corrected to describe the random-port self-launch reality.

### Theme C — self-hosted fixture server + index.rs FF152 fix

`common/mod.rs` gained `FixtureServer::start(routes) -> Option<Self>` /
`.base_url()` / `.port()` (std-only `TcpListener` on an ephemeral port,
`FixtureRoute { content_type, body }`, clean shutdown on Drop). Both live_62
tests now start their own server, the `localhost:18080` self-skip path is
deleted, and the network gate is dropped:

- live_index_local_fixture ... ok (16.42s)
- live_runner_page_map_resolution ... ok (11.40s)

**Product bug found and fixed in-branch** (`fix(index)` commit): `crawl_page`
hardcoded `WaitStrategy::Events`, which never completes on Firefox 152 —
`ff-rdp index` was effectively broken for real users, masked since iter-62
because the fixture server was never committed and both tests always
soft-skipped. Isolated outside the test suite:

```
# Events-only: deterministic timeout (5/5 same session, 3/3 fresh sessions)
ff-rdp --host 127.0.0.1 --port <p> --no-daemon navigate --allow-unsafe-urls \
  --wait-strategy events --timeout 3000 "data:text/html,<h1>x</h1>"
# => {"error":"navigate: page did not fire dom-complete within the timeout …"}

# Both (CLI default): succeeds every time
ff-rdp --host 127.0.0.1 --port <p> --no-daemon navigate --allow-unsafe-urls \
  --wait-strategy both --timeout 3000 "data:text/html,<h1>x</h1>"
# => ready_state "complete"
```

Fix: `crawl_page` now uses `WaitStrategy::Both` (readystate fallback,
matching the CLI navigate default). The two live_62 tests are its live
coverage.

### Sweep methodology finding — `cargo test-live` was not the baseline

The first full sweep ran `cargo test-live` as then defined (default libtest
parallelism) and produced 14 live-binary reds. 13 of them are OUTSIDE this
iteration's inventory and all pass in a serial re-run
(`--test-threads=1 … 14 passed; 0 failed`, 175s): cross-test interference
(daemon stop/kill and profile-prune tests colliding with concurrently
launched Firefox instances), made newly visible because this diff turned ~20
instant connection-refused reds into tests that actually launch Firefox. The
CI live lane and the iter-110 sweep baseline already used `--test-threads=1`;
the alias did not. Fixed in-branch: `test-live` in `.cargo/config.toml` now
includes `--test-threads=1`, so the plan's dogfood command matches the
CI/baseline methodology.

The serial sweep then exposed one more piece of suite debt:
`live_bulk_cap::live_bulk_frame_oversize_rejected` sets the process-global
transport cap via `set_max_frame_bytes(1024)` and never restored it, so every
later in-process transport user in the same binary inherited a 1 KiB frame
cap — observed as `live_console_no_double_delivery` failing with
`FrameTooLarge { declared: 1489, max: 1024 }` (it passes in any run order
that excludes bulk_cap). Fixed in-branch with a panic-safe RAII guard that
restores the previous cap on exit; verified by running the pair in-order
serially (both ok, 4.76s).

### Final full sweep (serial, post-alias-fix)

Two-part evidence, both under `FF_RDP_LIVE_TESTS=1 FF_RDP_LIVE_NETWORK_TESTS=1`:

1. Full workspace sweep (`cargo test-live`, serial): every non-live-binary
   target green — 689 + 274 + 9 + 10 + 9 + 5 + 1 + 1 passed, 0 failed.
2. Live binary re-run in full after the two in-branch suite fixes (alias +
   frame-cap guard): `cargo test -p ff-rdp-cli --test live -- --include-ignored
   --test-threads=1` → **121 passed; 1 failed; finished in 1075.67s**.

The single failure is `live_console_printf::live_console_printf_e2e` — the
explicitly justified red (product gap: console cache never primed via
`startListeners`; see Theme B above), cross-referenced to
[[iteration-116-console-cache-start-listeners]] which is filed on this branch
and carries the fix plus this test as its live AC. Zero unexplained failures.
