---
title: RDP Protocol — Index
type: index
tags: [rdp, index, protocol]
date: 2026-05-23
---

# RDP Protocol (wire level)

The bytes-and-JSON layer. Everything here is observable on the socket.

- [[rdp/protocol/transport|transport]] — TCP framing: `length:JSON` packets and `bulk actor type length:data` packets. Handshake, WebSocket variant.
- [[message-format]] — request/reply/event JSON shape: `from`, `to`, `type`, payload. Per-actor FIFO serialization. How a reply is distinguished from an event.
- [[error-handling]] — `{from, error, message}` shape; the small set of documented error names + the larger set actually emitted in practice.
- [[resources]] — the WatcherActor's streaming model: target events, resource events, ordering, throttling.

## Related

- For the spec/Front layer above the wire (Firefox client-side framework), see [[spec-and-front]].
- For per-actor request lists, see [[rdp/actors/README|actors/]].
- For resource-type-specific payloads, see [[rdp/resources/README|resources/]] (yes — confusingly named: `protocol/resources.md` is *how* resources stream; `resources/*.md` is *what* each resource type looks like).
