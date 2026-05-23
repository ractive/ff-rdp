---
type: rdp-note
tags: [rdp, firefox-server, resource, network]
date: 2026-05-23
firefox_files:
  - devtools/server/actors/resources/network-events-decoded-body-size.js
---

# Resource: `network-event-decoded-body-size`

Per-target watcher. Emits a separate resource carrying the **decoded** (post-gzip/brotli) body size for a request — because the network observer learns this number well after `network-event` fires.

## Payload

```
{
  resourceType: "network-event-decoded-body-size",
  resourceId: <channelId>,
  decodedBodySize: number,
  transferredSize: number,
}
```

Pair it to the corresponding [[rdp/resources/network-event|network-event]] by `channelId`.

## Why split out

The total decoded size is unknown until the body has been fully decompressed; emitting it as an update to the original network-event would force consumers to deal with mutation. Separate resource = simpler client.
