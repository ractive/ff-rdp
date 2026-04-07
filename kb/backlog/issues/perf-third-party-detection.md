---
title: "perf resource: add third-party detection"
type: feature
status: open
priority: medium
discovered: 2026-04-07
tags: [perf, resource, third-party, dogfooding]
---

# perf resource: add third-party detection

During dogfooding, 88% of resources on comparis.ch were third-party. Determining this
required a custom jq filter comparing each URL's domain against the page origin. A
`third_party: true` field on each resource entry (or a `--third-party-summary` flag)
would make this automatic.

## Detection logic

Compare resource URL domain against the navigation document's domain. If different,
mark as third-party. Could also group by registrable domain (eTLD+1) for accuracy,
but simple hostname comparison covers most cases.

## Desired output

Per-resource: `"third_party": true`

Summary (in `perf audit`):
```json
{
  "third_party_requests": 75,
  "third_party_pct": 88.1,
  "third_party_domains": ["nfahomefinder.b-cdn.net", "troubadix.data.comparis.ch", ...]
}
```
