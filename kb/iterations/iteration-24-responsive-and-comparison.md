---
title: "Iteration 24: Responsive Testing & Page Comparison"
type: iteration
status: completed
date: 2026-04-07
tags:
  - iteration
  - responsive
  - comparison
  - viewport
branch: iter-24/responsive-and-comparison
---

# Iteration 24: Responsive Testing & Page Comparison

Automated cross-viewport testing and multi-page performance comparison.

## Tasks

- [x] `ff-rdp responsive <selectors> --widths 320,768,1024,1440` — resize viewport,
  collect geometry + key computed styles at each breakpoint, restore original size
  → [[responsive-snapshot]]
- [x] `ff-rdp perf compare <url1> <url2> [...]` — navigate each URL sequentially,
  collect timing/vitals/resource stats, return comparison table
  → [[perf-compare-pages]]
- [x] `--format text` output mode with human-readable tables for all commands
- [x] `--label` flag for perf compare to name each page in output

## Test Fixtures

All e2e test fixtures must be recorded from a real Firefox instance — never hand-craft them.
Run with `FF_RDP_LIVE_TESTS_RECORD=1 cargo test -p ff-rdp-core --test live_record_fixtures -- --ignored` to record fixtures.
