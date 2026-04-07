---
title: "Iteration 24: Responsive Testing & Page Comparison"
type: iteration
status: planned
date: 2026-04-07
tags: [iteration, responsive, comparison, viewport]
branch: iter-24/responsive-and-comparison
---

# Iteration 24: Responsive Testing & Page Comparison

Automated cross-viewport testing and multi-page performance comparison.

## Tasks

- [ ] `ff-rdp responsive <selectors> --widths 320,768,1024,1440` — resize viewport,
  collect geometry + key computed styles at each breakpoint, restore original size
  → [[responsive-snapshot]]
- [ ] `ff-rdp perf compare <url1> <url2> [...]` — navigate each URL sequentially,
  collect timing/vitals/resource stats, return comparison table
  → [[perf-compare-pages]]
- [ ] `--format text` output mode with human-readable tables for all commands
- [ ] `--label` flag for perf compare to name each page in output
