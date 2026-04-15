---
title: "perf compare: side-by-side page performance comparison"
type: feature
status: resolved
priority: low
discovered: 2026-04-07
tags:
  - comparison
  - dogfooding
  - performance
---

# perf compare: side-by-side page performance comparison

During dogfooding, comparing performance across a multi-page user flow (homepage →
search landing → search results → listing detail) required running commands manually
and assembling a table by hand.

A `ff-rdp perf compare <url1> <url2> [<url3>...]` command could navigate to each URL
sequentially and return a comparison table:

```json
{
  "pages": [
    {"url": "https://example.com/", "ttfb_ms": 176, "dom_complete_ms": 649, ...},
    {"url": "https://example.com/search", "ttfb_ms": 1422, "dom_complete_ms": 4035, ...}
  ]
}
```

Could also support labeling: `--label "Homepage" --label "Search Results"`.
