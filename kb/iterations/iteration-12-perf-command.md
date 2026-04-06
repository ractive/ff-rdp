---
title: "Iteration 12: Performance API & Core Web Vitals"
type: iteration
date: 2026-04-06
tags:
  - iteration
  - performance
  - core-web-vitals
status: completed
branch: iter-12/perf-command
---

# Iteration 12: Performance API & Core Web Vitals

Extract `network --cached` into a standalone `perf` command and extend it to cover all Core Web Vitals metrics — making `ff-rdp` a scriptable tool for CWV collection.

## Background

Iteration 4 added `network --cached` using `performance.getEntriesByType("resource")`. Iteration 8 identified that this should be its own command since it's eval-based browser introspection, not RDP protocol. This iteration implements that extraction and extends it to cover all CWV metrics.

See [[research/performance-api-core-web-vitals]] for API details and computation methods.

## Design: Command Structure

```
ff-rdp perf                              # all resource entries (default)
ff-rdp perf --type navigation            # page load waterfall (DNS, TLS, TTFB, DOM)
ff-rdp perf --type paint                 # FP, FCP timestamps
ff-rdp perf --type lcp                   # Largest Contentful Paint
ff-rdp perf --type cls                   # Cumulative Layout Shift entries
ff-rdp perf --type longtask              # tasks >50ms (TBT proxy)
ff-rdp perf --type resource              # explicit resource timing (same as default)

ff-rdp perf vitals                       # Core Web Vitals summary with ratings (LCP, CLS, TBT, FCP, TTFB)

ff-rdp perf --filter "api/"              # URL substring filter (resource/navigation types)
```

### Subcommand: `perf vitals`

Returns a single JSON object with all Core Web Vitals computed from raw entries:

```json
{
  "results": {
    "lcp_ms": 1850,
    "lcp_rating": "good",
    "cls": 0.05,
    "cls_rating": "good",
    "tbt_ms": 120,
    "tbt_rating": "good",
    "fcp_ms": 980,
    "fcp_rating": "good",
    "ttfb_ms": 340,
    "ttfb_rating": "good"
  }
}
```

**CWV Computation** (matches web-vitals library):
- **TTFB**: `navigationEntry.responseStart` (or `responseStart - activationStart` if prerendered)
- **FCP**: paint entry where `name === "first-contentful-paint"`, `startTime`
- **LCP**: last `largest-contentful-paint` entry `startTime` (before page hidden)
- **CLS**: session window algorithm — group layout shifts (excluding `hadRecentInput`) into 1s-gap/5s-max windows, take max window sum
- **TBT**: sum of `(longTask.duration - 50)` for all longtask entries between FCP and TTI (lab proxy for INP since INP needs real user interactions)
- **INP**: not measurable in headless/lab — omitted

**Rating Thresholds** (Google's published thresholds):
| Metric | Good | Needs Improvement | Poor |
|--------|------|-------------------|------|
| LCP    | ≤2500ms | ≤4000ms | >4000ms |
| CLS    | ≤0.1 | ≤0.25 | >0.25 |
| TBT    | ≤200ms | ≤600ms | >600ms |
| FCP    | ≤1800ms | ≤3000ms | >3000ms |
| TTFB   | ≤800ms | ≤1800ms | >1800ms |

### JavaScript Strategy

Use `PerformanceObserver` with `buffered: true` for reliable collection of entries that may have been emitted before our script runs:

```javascript
new Promise(resolve => {
  const entries = {};
  const types = ['largest-contentful-paint', 'layout-shift', 'longtask', 'paint'];
  let pending = types.length;
  types.forEach(type => {
    try {
      new PerformanceObserver(list => {
        entries[type] = (entries[type] || []).concat(list.getEntries().map(e => e.toJSON()));
        // Observer fires once with buffered entries
      }).observe({ type, buffered: true });
    } catch(e) { /* type not supported */ }
    pending--;
  });
  // Also get navigation and resource via getEntriesByType (always available)
  entries.navigation = performance.getEntriesByType('navigation').map(e => e.toJSON());
  entries.resource = performance.getEntriesByType('resource').map(e => e.toJSON());
  // Give observers time to fire
  setTimeout(() => resolve(JSON.stringify(entries)), 100);
});
```

## Tasks

### Part A: Basic `perf` command (extract from network --cached)

- [x] Create `crates/ff-rdp-cli/src/commands/perf.rs` — extract from `network::run_cached()`
- [x] Support `--type` flag: `resource` (default), `navigation`, `paint`, `largest-contentful-paint`, `layout-shift`, `longtask`
- [x] Map CLI `--type` short aliases: `lcp` → `largest-contentful-paint`, `cls` → `layout-shift` (accept both forms)
- [x] Add `--filter` for URL substring filtering (resource/navigation types)
- [x] Handle LongString results (reuse `LongStringActor::full_string`)
- [x] Remove `--cached` flag from `network` command
- [x] Add CLI args and dispatch routing
- [x] Add live fixture recording tests for each `--type`
- [x] Add e2e tests with mock server
- [x] Update README

### Part B: `perf vitals` subcommand

- [x] Implement `perf vitals` subcommand in args.rs (clap subcommand within perf)
- [x] Write JS snippet using PerformanceObserver with buffered:true to collect all entry types in one eval
- [x] Implement CWV computation in Rust:
  - [x] TTFB from navigation entry
  - [x] FCP from paint entries
  - [x] LCP from largest-contentful-paint entries
  - [x] CLS session window algorithm
  - [x] TBT from longtask entries (sum of duration - 50ms)
- [x] Always include `_rating` field (good/needs-improvement/poor) alongside each metric value
- [x] Unit tests for CWV computation logic (pure Rust, no fixtures needed)
- [x] Live fixture recording for vitals snapshot
- [x] E2e tests with mock server

## Acceptance Criteria

- `ff-rdp perf` returns all resource timing entries (replaces `network --cached`)
- `ff-rdp perf --type navigation` returns page load waterfall with DNS/TLS/TTFB/DOM timings
- `ff-rdp perf --type paint` returns FP and FCP timestamps
- `ff-rdp perf --type lcp` returns LCP element and render time
- `ff-rdp perf --type cls` returns layout shift entries
- `ff-rdp perf --type longtask` returns long tasks
- `ff-rdp perf --filter "api/"` filters resource entries by URL
- `ff-rdp perf vitals` returns computed LCP, CLS, TBT, FCP, TTFB in one call
- `ff-rdp perf vitals` always includes `_rating` (good/needs-improvement/poor) alongside each metric
- `ff-rdp perf vitals --jq '.results.lcp_ms'` extracts single metric
- `network --cached` is removed (replaced by `perf`)
- Works on any page without prior watcher subscription

## Design Notes

- `perf` is purely eval-based — no RDP-specific actors beyond the console eval path
- The `vitals` subcommand collects all entry types in a single eval round-trip for efficiency
- CWV computation is in Rust for testability and correctness — the JS only collects raw entries
- INP is explicitly excluded (requires real user interaction, not measurable in lab/headless)
- TBT is used as the lab proxy for INP (INP requires real user interaction)
- PerformanceObserver with `buffered: true` is more reliable than `getEntriesByType` for observer-only types (LCP, CLS, longtask)
