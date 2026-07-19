---
title: "Iteration 127: a11y contrast --fail-only reports the sampled count as total, not the failure count"
type: iteration
date: 2026-07-19
status: in-progress
branch: iter-127/a11y-contrast-fail-only-total
depends_on: []
firefox_refs: []
kb_refs:
- kb/dogfooding/dogfooding-session-61.md
first_call_sites: []
dogfood_path: |
  # --fail-only's top-level total must count returned failures, not sampled elements:
  ff-rdp --port <p> navigate https://news.ycombinator.com
  ff-rdp --port <p> a11y contrast --fail-only --all \
    --jq '{total, shown: (.results|length), sampled}'
  # expected: total == (.results|length)  (0 when nothing fails AA),
  #           sampled == number of checked elements (e.g. 4)
  # was: {"total": 4, "shown": 0}  — total lied about 4 "failures" that were the sample;
  #      on a failing page: total 500 vs 447 actual failures
tags:
- iteration
- a11y
- contrast
- output-contract
- firefox-152
- dogfood-61
---

# Iteration 127: a11y contrast --fail-only total counts the sample, not the failures

Discovered in [[dogfooding-session-61]] (ff-rdp v0.3.0 / Firefox 152), reproduced clean on a
single instance: `a11y contrast --fail-only` on news.ycombinator.com returned `total: 4` with
`results: []` — zero failures, yet `total` claims 4. On a genuinely failing page the same
skew appeared as `total: 500` against 447 actual failing entries. Any consumer asserting on
`total` ("page has N contrast failures") gets a lie; the contrast *detection* itself is
accurate — only the count field is wrong.

Root cause in `crates/ff-rdp-cli/src/commands/a11y_contrast.rs`:

1. The in-page JS returns `summary: {total: checks.length, aa_pass, aa_fail, capped}`
   (`a11y_contrast.rs:235`) — `summary.total` is the **sampled** check count.
2. `run` applies the `--fail-only` filter to the checks (`a11y_contrast.rs:37-51`), then
   reads `total_count` from that JS `summary.total` (`a11y_contrast.rs:53-58`) — the
   pre-filter sample size.
3. The envelope is built with `total_count.max(total)` (`a11y_contrast.rs:80-86`, where
   `total` is the post-filter/pre-limit count from `apply_limit`,
   `a11y_contrast.rs:76`) — so the top-level `total` reports the sampled count whenever it
   exceeds the failure count, i.e. always under `--fail-only` on a mostly-passing page.
   The help text (`crates/ff-rdp-cli/src/cli/args.rs:660`) documents `"total": N` with no
   hint that it can exceed `results`.

The `.max()` was presumably meant to keep `total` honest when the output limit truncates
`results`; combined with `--fail-only` it instead conflates two different populations
(sampled elements vs returned failures).

## Themes

- **A — `total` counts what the command returns.** Under `--fail-only`, `total` is the
  number of failing checks (post-filter, pre-limit); without the flag, the number of checks.
  Drop the `total_count.max(total)` conflation.
- **B — Expose the sample size under its own name.** Surface the JS `summary.total` as a
  distinct `sampled` field so the "how many elements were examined" signal (and the `capped`
  interaction) is not lost — it just stops masquerading as `total`.

## Tasks

### A. Honest total

- [x] In `run`, pass the post-filter count to the envelope: replaced `total_count.max(total)`
      with the `total` returned by `apply_limit` (now `total_count` is renamed `sampled` and no
      longer feeds the envelope `total`), so truncation via `--limit` still reports the full
      failure count while `--fail-only` no longer inflates it. `apply_fail_only_filter` extracted
      as a pure, testable helper.
- [x] Added unit tests around the filter + envelope assembly: `fail_only_all_passing_reports_zero_total_and_sample_size`
      (sampled=4, failures=0 → `total == 0`, `results == []`, `sampled == 4`) and
      `fail_only_reports_failure_count_not_sample_size` (sampled=500, failures=447 → `total == 447`).

### B. Distinct sampled field

- [x] Emit `sampled` (from JS `summary.total`) at the top level of the envelope next to `total`,
      keeping the existing `meta.summary` untouched for aa_pass/aa_fail/capped detail
      (`a11y_contrast::run` inserts `sampled` after `envelope_with_truncation`).
- [x] Updated the help text (`args.rs` A11y `long_about`, the new `Contrast` `long_about`, and the
      `--fail-only` usage/cookbook lines) to document `total` = returned results (failures when
      `--fail-only`) and `sampled` = elements checked, with a backward-compat note that `total`
      previously reported the sample size.

## Acceptance Criteria [4/4]

<!-- Each AC names a live test + asserted post-condition, per CLAUDE.md convention. -->

- [x] live_a11y_contrast_fail_only_total_zero: on a page whose sampled text all passes AA
      (local all-passing `data:` fixture `FIXTURE_HTML_ALL_PASS`), `a11y contrast --fail-only`
      yields `total == 0`, `results == []`, and `sampled >= 1`. PASSED against live Firefox.
- [x] live_a11y_contrast_fail_only_total_counts_failures: on the known-AA-failure fixture
      (`live_a11y_contrast_wai_bad::FIXTURE_HTML`), `--fail-only --all` yields
      `total == (.results | length)` and `sampled >= total`. PASSED against live Firefox.
- [x] live_a11y_contrast_limit_keeps_total: `--fail-only --limit 1` on the failing page still
      reports the full failure count in `total` (with `truncated == true`), not 1 — limit
      truncates `results`, never `total`. PASSED against live Firefox.
- [x] `cargo fmt && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace -q` clean.

## Design notes

- The JS template (`a11y_contrast.rs:99-235`) needs no change: it already reports the sample
  size correctly under `summary.total`; the bug is purely the Rust-side envelope mapping
  (`a11y_contrast.rs:53-86`). Keep the fix on the Rust side.
- `total` semantics after the fix match the rest of the CLI: `envelope_with_truncation`
  (`crates/ff-rdp-cli/src/output.rs:28`) everywhere else receives the post-filter population
  count, with `shown` covering the limited slice — a11y contrast becomes consistent instead
  of special.
- iter-126 precedent (output-contract fixes): keep the parity discipline from
  `network_and_navigate_summary_fields_agree_field_for_field` — write one unit test that pins
  the exact field values (not just presence/type) for both the zero-failure and
  known-failure-count cases, and record the live dogfood transcript in the same
  `{t, n}`-style compact `--jq` projection used by `dogfood_path` above so a shape regression
  is caught by the pre-PR gate, not just by hand-inspection.
- Without `--fail-only`, `total == sampled` by construction; emitting both keeps the shape
  stable across flag combinations (no key that appears only with the flag).

## Out of scope

- The 1000-element sampling cap in the JS (`capped`, `a11y_contrast.rs:235`) — sampling
  policy is unchanged; this iteration only stops the sample size from posing as the result
  count.
- Other `a11y` subcommands (tree, labels) — their totals are not built from a pre-filter JS
  summary.
- AAA-level filtering — `--fail-only` stays AA-based (`aa_large`/`aa_normal`,
  `a11y_contrast.rs:41-47`).

## References

- [[dogfooding-session-61]]
- [[iteration-125-perf-audit-lcp-unavailable]]
- [[iteration-126-network-json-shape-consistency]]
