---
title: "perf resource: transfer_size 0 for cached resources is confusing"
type: feature
status: resolved
priority: low
discovered: 2026-04-07
tags:
  - resource
  - ux
  - dogfooding
  - performance
---

# perf resource: transfer_size 0 for cached resources is confusing

Most resources on repeat visits show `transfer_size: 0` because they're served from
cache. This looks like broken data. Should expose `decoded_size` (actual content size)
and/or a `from_cache` boolean so users can distinguish cached vs genuinely tiny resources.

## Current

```json
{"url": "https://cdn.example.com/app.js", "transfer_size": 0, "decoded_size": 15377}
```

The `decoded_size` field is already present in the output but `transfer_size: 0` is
what users naturally look at. Adding `from_cache: true` when `transfer_size == 0 &&
decoded_size > 0` would clarify this.
