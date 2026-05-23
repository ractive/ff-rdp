---
type: rdp-note
tags: [rdp, firefox-server, resource, network]
date: 2026-05-23
firefox_files:
  - devtools/server/actors/resources/server-sent-events.js
  - devtools/server/actors/resources/websockets.js
  - devtools/server/actors/resources/webtransport.js
---

# Resources: `server-sent-event`, `websocket`, `webtransport`

Per-target sub-protocol stream watchers. Each emits frames/events that the netmonitor's per-channel sub-pane displays.

## server-sent-event

Observes `EventSource` connections. Payload entries:

```
{ resourceType: "server-sent-event", channelId, data, eventName, lastEventId, retry, timestamp }
```

## websocket

Observes WebSocket frames. Payload:

```
{ resourceType: "websocket", channelId, type: "frameSent"|"frameReceived"|"opened"|"closed", payload, fin, maskBit, opCode, timestamp }
```

## webtransport

Observes HTTP/3 WebTransport sessions. Payload similar, with stream/datagram framing.

## Gotchas

- These are **per-frame** payloads (potentially many per second) — heavy. Don't subscribe unless you need them.
- Tied to the parent [[rdp/resources/network-event|network-event]] resource by `channelId` — use that to associate the upgrade request with the stream.
