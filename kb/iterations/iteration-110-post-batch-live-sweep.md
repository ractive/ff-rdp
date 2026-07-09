---
title: "Iteration 110: post-batch full live-suite sweep — run everything against real Firefox once, fix all fallout"
type: iteration
date: 2026-07-09
status: planned
branch: iter-110/post-batch-live-sweep
depends_on:
  - kb/iterations/iteration-109-network-throttle-block.md
  - kb/iterations/iteration-106-live-test-masking-cascade.md
firefox_refs: []
kb_refs:
  - kb/iterations/iteration-100b-live-test-consolidation.md
first_call_sites: []
dogfood_path: |
  # The sweep itself IS the dogfood: full gated live suite against headless Firefox.
  FF_RDP_LIVE_TESTS=1 FF_RDP_LIVE_NETWORK_TESTS=1 cargo test-live
  # Post-condition: zero failures that are attributable to iterations 101-105.
tags:
  - iteration
  - tests
---

# Iteration 110 — post-batch full live-suite sweep

## Execution policies (2026-07-09, per James)

The full live-suite run IS this iteration's core job — no live-test
restriction applies here. Scoped testing still applies to the fix work: while
iterating on a fix, run only the tests it touches; one full
`cargo test --workspace -q` in the final pre-PR gates, and the review agent
does not re-run the full workspace suite. CI-wait: required lanes only; if
[[iteration-108-windows-ci-preexisting-reds]] merged earlier in this batch,
`test (windows-latest)` should be green and any windows failure is real.

## Motivation

Per James's 2026-07-09 decision, iterations 102–105 and 106–109 do NOT run the full live
Firefox suite per-iteration (it dominated wall-clock: 20–40 min per run, often
run twice per iteration by implement + review agents). Each of those
iterations still runs its own dogfood script and the specific live tests named
in its ACs — only the *full-suite* pass is deferred to here, once, after
iteration 105 merges.

## Theme A — one full sweep

Run `FF_RDP_LIVE_TESTS=1 FF_RDP_LIVE_NETWORK_TESTS=1 cargo test-live` (the
consolidated `live` target from [[iteration-100b-live-test-consolidation]])
against current main. Record the complete pass/fail inventory in this plan's
Results section.

## Theme B — fix the fallout

For every failure:
- If caused by an iteration in the 101–109 range: fix it in this iteration.
- If pre-existing environmental (the 19 reds catalogued during iter-100b,
  tracked in [[iteration-106-live-test-masking-cascade]]): leave to iter-106,
  but cross-reference it in the inventory.
- New live tests introduced by 101–109 whose full-suite interaction was never
  exercised (port contention, daemon-registry sharing, buffer state leaking
  between modules in the consolidated binary) are in scope here.

## Acceptance criteria

- [ ] full_sweep_recorded: complete `cargo test-live` inventory (pass/fail per
      test) attached to Results, run on post-109 main.
- [ ] no_101_105_regressions: every failure attributable to iterations
      101–105 is fixed and its test passes in a re-run; fixes carry their own
      unit/live tests where behaviour changed.
- [ ] preexisting_reds_crossref: remaining failures are each cross-referenced
      to iter-106 (or a filed follow-up), none left untracked.

## Results

### 2026-07-09 — full sweep inventory recorded during iter-106 Theme C

iter-106's masked-surface audit ran the entire consolidated live target once
(`FF_RDP_LIVE_TESTS=1 FF_RDP_LIVE_NETWORK_TESTS=1 cargo test -p ff-rdp-cli
--test live --no-fail-fast -- --ignored --test-threads=1`, clean sequential
run, no concurrent cargo). **68 passed, 31 failed** (of 99 ignored live tests;
10 non-ignored filtered out). iter-106 fixed the failures in ITS scope (the
four themed gates + `live_cookie_longstring_value`); the remaining **29** are
pre-existing masked failures handed to this iteration (Theme B). Root-cause
categories:

**(a) `data:` URL security gate — navigate to a `data:` fixture without
`--allow-unsafe-urls`** (dominant; the gate landed in iter-63). These tests
never ran in CI (masked since iter-61t) so nobody noticed they lack the flag.
Fix: add `--allow-unsafe-urls` to the fixture navigate in each. Files:
`live_cascade` (2), `live_cascade_explains_pico_dialog`,
`live_95_cascade_computed_agreement` (2), `live_98_media_query_truthfulness`
(pre_fix_repro), `live_a11y_critical`, `live_dom_include_style`,
`live_snapshot_max_depth`, `live_styles_applied`,
`live_navigate_real_site::live_navigate_default_completes_within_timeout`.
Symptom: `navigate failed` with empty stderr (the error
`URL scheme 'data:' is not allowed by default` is emitted as JSON on stdout).

**(b) Stale test-assertion shapes** (same class iter-106 fixed for the network
tests). `live_network_headers` (checks a `response_headers` field that is now
`headers.response`); `live_cross_actor::live_cross_actor_packet_not_lost`
(compares the whole eval JSON to `2` instead of `.results`). Fix: update the
assertions to the current output contract. **These likely share the fix
pattern from iter-106's `live_61q`/`live_network_default_watcher` corrections.**

**(c) Real-site network flakiness / timing** — tests that navigate to
example.com / MDN / pico under the default fast-navigate budget:
`live_navigate_default_fast` (2), `live_navigate_real_site` (dom-complete),
`live_cascade_real_site` (2), `live_cascade_explains_pico_dialog`,
`live_dom_stats_perf_audit_parity`, `live_a11y_contrast_wai_bad`,
`live_screenshot_ff151` (2), `live_screenshot_bulk_fallback`,
`live_wait_timeout_ms_canonical`, `live_stale_tab_race`,
`live_target_destroyed`, `live_styles_applied_dedupe`, `live_console_printf`
(console-message content mismatch), `live_cookies_set_cookie_header`. Triage
each: real product bug vs. runner network/timing. Some may overlap category (a)
or (b) — re-run in isolation to classify (the full-suite run itself can flake
under load; iter-106 confirmed several category-(a) failures reproduce in
isolation on fresh Firefox, so they are real, not load artifacts).

**(d) Fixture-server-not-committed (unchanged, expected)** —
`live_62_page_map_index::live_index_local_fixture` /
`live_runner_page_map_resolution` self-skip when `http://localhost:18080` is
unreachable; not a real failure. Left as-is (iter-106 Theme D design note).

Complete raw failure list (31):
`live_62_page_map_index::{live_index_local_fixture,live_runner_page_map_resolution}`,
`live_95_cascade_computed_agreement::{live_cascade_inherited_or_default_note_fires_on_h1_color,pre_fix_repro_cascade_prop_populates_computed_when_standalone_computed_does}`,
`live_98_media_query_truthfulness::pre_fix_repro_cascade_winner_ignores_media_context`,
`live_a11y_contrast_wai_bad::live_a11y_contrast_wai_bad_detects_failures`,
`live_a11y_critical::a11y_critical_filters_to_violations`,
`live_cascade::{live_cascade_returns_matched_rules,live_cascade_returns_matched_rules_external_css}`,
`live_cascade_explains_pico_dialog::live_cascade_explains_pico_dialog`,
`live_cascade_real_site::{live_cascade_real_site_cli,live_cascade_real_site_returns_rules_for_a_element}`,
`live_console_printf::live_console_printf_e2e`,
`live_cookies_set_cookie_header::live_cookies_set_cookie_header_visible_after_navigate`,
`live_cross_actor::live_cross_actor_packet_not_lost`,
`live_dom_include_style::dom_include_style_attaches_computed_values`,
`live_dom_stats_perf_audit_parity::live_dom_stats_perf_audit_parity_images_without_lazy`,
`live_navigate_default_fast::{live_navigate_default_fast_no_budget_exhaustion,live_navigate_global_timeout_flag_accepted}`,
`live_navigate_real_site::{live_navigate_default_completes_within_timeout,live_navigate_dom_complete_within_default_timeout}`,
`live_network_headers::live_network_headers`,
`live_screenshot_bulk_fallback::live_screenshot_bulk_fallback_then_eval`,
`live_screenshot_ff151::{live_screenshot_ff151_cli,live_screenshot_ff151_produces_valid_png}`,
`live_snapshot_max_depth::live_snapshot_max_depth_truncates_tree`,
`live_stale_tab_race::live_stale_tab_race_no_such_actor_after_navigate`,
`live_styles_applied::live_styles_applied_returns_real_rules`,
`live_styles_applied_dedupe::live_styles_applied_dedupe_no_duplicate_actor_ids`,
`live_target_destroyed::live_target_destroyed_invalidates_registry`,
`live_wait_timeout_ms_canonical::live_wait_timeout_ms_canonical_flag`.
