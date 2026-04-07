---
title: network command returns empty for already-loaded pages
type: bug
status: resolved
priority: low
discovered: 2026-04-07
tags:
  - network
  - watcher
  - dogfooding
resolved_by: iteration-26
---

# network command returns empty for already-loaded pages

Firefox 149+ only delivers `resources-available-array` events that occur *after*
`watchResources` subscription. Historical events from the already-loaded page are
not sent. This means `ff-rdp network` returns empty unless the page is actively loading.

## Repro

```sh
ff-rdp navigate https://example.com
# wait for page to finish loading
ff-rdp network
# returns empty []
```

## Possible fix

Fall back to Performance API `performance.getEntriesByType('resource')` via eval to
provide historical network data when the watcher returns nothing. This could be a
`--historical` flag or automatic fallback.
