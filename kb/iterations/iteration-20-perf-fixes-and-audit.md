---
title: "Iteration 20: Perf Fixes & Audit Command"
type: iteration
status: completed
date: 2026-04-07
tags:
  - iteration
  - perf
  - jq
  - bugfix
  - audit
branch: iter-20/perf-fixes-and-audit
---

# Iteration 20: Perf Fixes & Audit Command

Fix broken perf/jq output, enrich resource data, then build the `perf audit`
command on top. Natural progression: fix the foundation, then build the summary.

## Part A: Bug Fixes

- [x] Fix jq error messages: catch jaq runtime errors and format as readable
  messages instead of dumping internal Rust debug representation
  → [[jq-error-messages-unreadable]]
- [x] Fix `perf --type resource --jq` envelope: apply jq filter to the full
  `{meta, results, total}` envelope, consistent with other perf subcommands
  → [[perf-jq-envelope-inconsistency]]
- [x] Fix `perf vitals` FCP/LCP null: fall back to `performance.getEntriesByType('paint')`
  when PerformanceObserver returns empty for paint entries
  → [[perf-vitals-fcp-lcp-null]]
- [x] Add `from_cache` boolean to `perf --type resource` entries when
  `transfer_size == 0 && decoded_size > 0`
  → [[perf-resource-cache-indicator]]

## Part B: Resource Enrichment

- [x] Add `resource_type` field to `perf --type resource` entries, derived from
  URL extension with content-type fallback (js, css, image, font, document, xhr, other)
  → [[perf-resource-type-classification]]
- [x] Add `third_party` boolean to `perf --type resource` entries by comparing
  resource domain against the navigation document's domain
  → [[perf-third-party-detection]]

## Part C: Audit Command

- [x] Add `ff-rdp dom stats` command: node count, document size, inline script
  count, render-blocking resources, images without lazy loading
  → [[dom-stats-command]]
- [x] Implement `ff-rdp perf audit` combining: navigation timing, web vitals,
  resource breakdown by type/domain, third-party weight, top-N slowest resources,
  DOM stats — single structured JSON output
- [x] Add cookbook/recipes section to `ff-rdp --help` or `ff-rdp recipes`:
  curated `--jq` one-liners for common tasks

## Test Fixtures

All e2e test fixtures must be recorded from a real Firefox instance — never hand-craft them.
Run with `FF_RDP_LIVE_TESTS_RECORD=1 cargo test -p ff-rdp-core --test live_record_fixtures -- --ignored` to record fixtures.
