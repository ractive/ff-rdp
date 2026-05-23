---
type: rdp-note
tags: [rdp, official-docs, protocol, transport]
date: 2026-05-23
title: Transport — framing on the wire
sources:
  - https://firefox-source-docs.mozilla.org/devtools/backend/protocol.html
---

# Transport — framing on the wire

The RDP runs over any reliable, ordered byte stream — *"TCP/IP or
pipes"* ([protocol][proto]). Mozilla calls this layer the **Stream
Transport**.

[proto]: https://firefox-source-docs.mozilla.org/devtools/backend/protocol.html

## Two packet kinds

Every byte on the stream belongs to one of two packet shapes:

1. **JSON packet** — length-prefixed UTF-8 JSON. The default.
2. **Bulk packet** — length-prefixed raw bytes, identified by a
   leading `bulk` keyword. Used for shipping large binary blobs (heap
   snapshots, screenshots) without forcing them through JSON encoding.

There is **no newline framing, no HTTP, no WebSocket, no
length-in-header binary framing** — just ASCII length prefixes.

## JSON packet format

From [protocol][proto]:

> *"Format: `length:JSON` … Where the length represents byte count of
> the UTF-8 encoded JSON that follows."*

So the wire literally looks like:

```
83:{"from":"root","applicationType":"browser","traits":{"networkMonitor":true}}
```

Rules:

- `length` is **decimal ASCII**, no leading zeros required.
- The separator is a single ASCII colon `:`.
- The `length` counts bytes (not characters) of the UTF-8 JSON body.
- The JSON body is a single top-level object.
- No padding, no terminator — the next packet starts immediately after
  the JSON object's closing brace.

A correct reader is therefore: read ASCII digits until `:`, parse the
integer N, read exactly N bytes, parse as JSON. Repeat.

## Bulk packet format

From [protocol][proto]:

> *"`bulk actor type length:data`"* — *"ASCII-encoded 'bulk' keyword,
> actor name (no spaces/colons), type identifier, decimal length, and
> raw binary data."*

Concretely:

```
bulk <actor> <type> <length>:<length bytes of raw data>
```

The header (`bulk`, actor, type, length) is ASCII separated by single
spaces. Right after the `:` comes exactly `<length>` bytes of opaque
binary payload. Then the next packet (JSON or bulk) starts.

Bulk packets are bidirectional but rare; many simple clients never
need to *send* one. They **must**, however, be able to *skip* an
inbound bulk packet they don't recognise (read header, consume N
bytes, move on) to stay framed.

## Why this design

The docs note this scheme is chosen so streaming callbacks can read or
write large datasets *"directly without creating temporary copies"*
([protocol][proto]). For typical clients it just means: don't load the
whole packet into a String — read length first, then exact bytes.

## What clients usually get wrong

- **Counting characters instead of bytes** for the JSON length. Any
  non-ASCII content (a console message, a URL with `é`) will desync.
- **Reading line-by-line.** There are no newlines in the framing.
- **Not handling the greeting** — the very first thing the server
  sends is an unsolicited JSON packet from `"root"` (see
  [[rdp/overview/connection-lifecycle]]).
- **Choking on bulk packets** even when you don't use them. At minimum
  parse the header and skip.

## Relationship to higher layers

The packet body is plain JSON; see [[message-format]] for its shape
(`from`/`to`/`type`/...), [[error-handling]] for failure replies, and
[[resources]] for the Watcher resource events.
