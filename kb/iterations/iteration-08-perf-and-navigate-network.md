---
title: "Iteration 8: Performance API + Navigate with Network"
type: iteration
date: 2026-04-06
tags:
  - iteration
  - performance
  - network
  - navigate
  - resource-timing
status: obsolete
branch: iter-8/perf-navigate-network
---

# Iteration 8: Performance API + Navigate with Network

Two related but distinct improvements to network/performance observability.

## Background

Iteration 4 added `network` (WatcherActor-based, real-time) and a `--cached` flag using the Performance Resource Timing API. During review we identified two design issues:

1. **Concept mixing**: `network --cached` conflates RDP protocol monitoring with a browser API query. The Performance API is eval-based browser introspection (like `page-text`), not RDP network watcher protocol. It should be its own command.
2. **Connection scoping**: The WatcherActor is connection-scoped — you can only capture events that happen *during* the subscription. `navigate` and `network` are separate CLI invocations on separate TCP connections, so you can't capture a navigation's network traffic without a combined command.

## Part A: `ff-rdp perf` command

Extract `network --cached` into a standalone `perf` command backed by the W3C Performance API. This is a family of eval-based queries, not just resource timing:

```
ff-rdp perf                    # resource entries (default)
ff-rdp perf --type navigation  # page load waterfall (DNS, TLS, TTFB, DOM)
ff-rdp perf --type paint       # First Paint, First Contentful Paint
ff-rdp perf --type lcp         # Largest Contentful Paint
ff-rdp perf --type layout-shift # Cumulative Layout Shift entries
ff-rdp perf --type longtask    # Main thread tasks >50ms
```

Each `--type` maps to a `performance.getEntriesByType()` call.

### Tasks

- [ ] Create `ff-rdp-cli/src/commands/perf.rs` — extract and generalize from `network::run_cached()`
- [ ] Support `--type` flag with values: `resource` (default), `navigation`, `paint`, `largest-contentful-paint`, `layout-shift`, `longtask`
- [ ] Add `--filter` for URL substring filtering (resource/navigation types)
- [ ] Handle LongString results (reuse `LongStringActor::full_string`)
- [ ] Remove `--cached` flag from `network` command
- [ ] Add CLI args and dispatch routing
- [ ] E2e tests with mock server (evaluateJSAsync pattern)
- [ ] Update README with `perf` command and examples

### Acceptance Criteria

- `ff-rdp perf` returns all resource timing entries retrospectively
- `ff-rdp perf --type navigation` returns page load waterfall
- `ff-rdp perf --type paint` returns paint milestones
- `ff-rdp perf --filter "api/"` filters resource entries by URL
- `ff-rdp perf --jq '[.results[] | .duration_ms] | add'` sums durations
- Works on any page without needing prior watcher subscription

## Part B: `navigate --with-network`

Add a `--with-network` flag to `navigate` that keeps the same TCP connection open, subscribes to the watcher *before* navigating, then drains network events *after*.

Flow: connect → getWatcher → watchResources → navigate → drain events → unwatchResources → output

### Tasks

- [ ] Add `--with-network` flag to `Navigate` command variant
- [ ] Implement combined flow in `commands/navigate.rs` (or a new orchestrator)
- [ ] Output shape: `{ "navigated": "...", "network": [...] }` or separate sections
- [ ] Reuse existing network event parsing from `ff-rdp-core` (parse_network_resources, parse_network_resource_updates, merge logic)
- [ ] E2e tests with mock server (navigate + followup resource events)
- [ ] Update README

### Acceptance Criteria

- `ff-rdp navigate https://example.com --with-network` returns navigation result + all network requests triggered by the navigation
- `ff-rdp navigate https://example.com --with-network --jq '.network[] | select(.status >= 400)'` finds failed requests during navigation
- Network data includes status, timing, size (from resource-updated-array merging)

## Design Notes

- `perf` is purely eval-based — no RDP-specific actors beyond the console eval path
- `navigate --with-network` is purely RDP-based — single connection, watcher subscription
- Both complement `network` (standalone real-time watcher) without overlapping
- The existing `network --cached` implementation can be refactored into `perf` with minimal changes
- `LongStringActor` (added in iter 4) is a protocol-level actor used by both `perf` and `eval` — correctly lives in ff-rdp-core

## Performance API Reference

| `--type` value | `getEntriesByType()` argument | What it returns |
|---|---|---|
| `resource` (default) | `"resource"` | Every fetched resource: URL, timing, size, protocol, initiator |
| `navigation` | `"navigation"` | Page load waterfall: DNS, TLS, TTFB, DOM interactive/complete |
| `paint` | `"paint"` | First Paint, First Contentful Paint timestamps |
| `lcp` | `"largest-contentful-paint"` | LCP element, render time, size |
| `layout-shift` | `"layout-shift"` | CLS entries with value and source elements |
| `longtask` | `"longtask"` | Tasks blocking main thread >50ms |
