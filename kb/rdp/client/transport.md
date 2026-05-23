---
type: rdp-note
tags:
  - rdp
  - firefox-client
  - transport
date: 2026-05-23
firefox_files:
  - devtools/shared/transport/transport.js
  - devtools/shared/transport/packets.js
  - devtools/shared/transport/stream-utils.js
  - devtools/shared/transport/websocket-transport.js
  - devtools/shared/transport/local-transport.js
title: Transport (client-side framing)
---

# RDP Transport Layer (Firefox client)

The transport is wire-level only. It moves opaque packets between client and
server; it knows nothing about actors or methods. Two packet kinds, three
transport variants.

## Framing — length-prefixed JSON

Defined in `devtools/shared/transport/packets.js` (see also [[rdp/protocol/transport|protocol/transport]]). Each JSON packet
is serialized as UTF-8 and prefixed with its byte length and a colon:

```
<decimal-length>:<json-bytes>
```

Example: `61:{"to":"root","type":"listTabs"}` (length = 30 here is illustrative;
real packets count post-UTF-8 bytes — see `JSONPacket` setter at
`packets.js:152-157` which converts via `nsIScriptableUnicodeConverter` before
measuring length).

The header regex `JSONPacket.HEADER_PATTERN = /^(\d+):$/` (`packets.js:141`)
matches and the parser switches to "reading N bytes of body" mode. After body is
fully read, the packet is parsed as JSON and dispatched via
`transport.hooks.onPacket(packet)`.

Header parsing has a guard: `PACKET_HEADER_MAX = 200` in
`transport.js:28` — if no `:` is seen in 200 bytes, the connection is killed.
Body length cap is `PACKET_LENGTH_MAX = 2^40` (1 TiB) in `packets.js:47`.

## Bulk packets (binary)

Header pattern `BulkPacket.HEADER_PATTERN = /^bulk ([^: ]+) ([^: ]+) (\d+):$/`
matches lines like:

```
bulk <actor> <type> <length>:<length bytes of raw binary>
```

After the colon, the next N bytes are raw bytes (not JSON). Used for things
like heap snapshots, large source contents, perf profiles. Client must use
`copyTo` / `copyToBuffer` helpers — see `transport.js:51-84` doc comment.
ff-rdp doesn't currently use bulk packets — the screenshot path returns a
base64 data: URL inside a JSON packet instead.

## Connection handshake

The server speaks first. On TCP accept, the server sends an unsolicited
JSON packet from actor `"root"` describing itself:

```json
{"from":"root","applicationType":"browser","testConnectionPrefix":"server1.","traits":{...}}
```

The client side ([`devtools-client.js:94-120`](devtools-client.md))
sets up a one-shot listener via `this.expectReply("root", ...)` *before* calling
`this._transport.ready()`. When the greeting arrives, the client:

1. Builds a `RootFront` via `createRootFront(this, packet)`.
2. Sends `{"to":"root","type":"connect","frontendVersion":"..."}` (Fx 133+) to
   negotiate.
3. Emits the `"connected"` event so `client.connect()` can resolve.

Only after this handshake should consumers send `listTabs`, etc.

## Transport variants

- **`DebuggerTransport`** (`transport.js`) — TCP via `nsIAsyncInputStream` /
  `nsIAsyncOutputStream`. The variant Firefox itself uses, and the one ff-rdp
  speaks against (`firefox --start-debugger-server 6000`).
- **`LocalDebuggerTransport`** (`local-transport.js`) — In-process pipe between
  client and server in the same process (e.g. DevTools on a local tab). Skips
  framing entirely — just hands `packet.object` directly across.
- **`WebSocketDebuggerTransport`** (`websocket-transport.js`) — Wraps a
  WebSocket; each frame *is* one JSON packet, no length prefix.
  **Bulk send not supported** (`websocket-transport.js:35-37`). Used by some
  remote-debug-over-WSS setups.
- **`ChildDebuggerTransport`** / **`JsWindowActorTransport`** /
  **`WorkerDebuggerTransport`** — IPC variants used inside content/worker
  processes.

## Reconnection

There is **no** built-in reconnect. `transport.close()` calls
`hooks.onTransportClosed(reason)`; `DevToolsClient` rejects all pending
requests and emits `"closed"`. Clients (including DevTools) start a fresh
connection from scratch when needed.

See also: [[spec-and-front]], [[devtools-client]],
[[rdp/flows/connect-and-list-tabs]].
