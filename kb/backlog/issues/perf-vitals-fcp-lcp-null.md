---
title: "perf vitals: FCP and LCP null in non-headless Firefox"
type: bug
status: open
priority: medium
discovered: 2026-04-07
tags: [perf, vitals, dogfooding]
---

# perf vitals: FCP and LCP null in non-headless Firefox

`perf vitals` returns `null` for `fcp_ms` and `lcp_ms` in non-headless Firefox.
`PerformanceObserver` with `buffered: true` for `paint` and `largest-contentful-paint`
entry types does not fire reliably without prior user interaction (scroll, click, etc.).

## Repro

```sh
ff-rdp launch
ff-rdp navigate https://www.comparis.ch
ff-rdp perf vitals
# fcp_ms: null, lcp_ms: null
```

## Possible fixes

- Fall back to `performance.getEntriesByType('paint')` for FCP when observer returns empty
- For LCP, consider a JS-injected observer that stays active during navigation
