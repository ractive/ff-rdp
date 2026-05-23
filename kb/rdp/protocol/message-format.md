---
type: rdp-note
tags: [rdp, official-docs, protocol, messages]
date: 2026-05-23
title: Message format — packet shape and pairing
sources:
  - https://firefox-source-docs.mozilla.org/devtools/backend/protocol.html
  - https://firefox-source-docs.mozilla.org/devtools/backend/protocol.js.html
---

# Message format — packet shape and pairing

Once you've decoded a JSON packet off the wire ([[transport]]), this
page tells you what's *inside* it.

## Direction is encoded by the addressing field

From [protocol][proto]:

> - *"Client packets: `{ "to":actor, "type":type, ... }`"*
> - *"Server packets: `{ "from":actor, ... }`"*

So:

- **Client → server** packets carry `"to"` (which actor receives this)
  and `"type"` (which request).
- **Server → client** packets carry `"from"` (which actor is speaking).
  They do *not* carry `"to"`.

There is no separate request ID. The pairing is positional — see
"request/reply pairing" below.

[proto]: https://firefox-source-docs.mozilla.org/devtools/backend/protocol.html

## Request packet

```
{
  "to":   "<actorID>",
  "type": "<methodName>",
  ...arbitrary additional fields per request type...
}
```

Examples:

```json
{"to":"root","type":"listTabs"}
{"to":"server1.conn0.watcher5","type":"watchResources",
 "resourceTypes":["console-message","source"]}
{"to":"server1.conn0.consoleActor7","type":"evaluateJSAsync",
 "text":"1+1"}
```

`protocol.js` *"will add the 'type' and 'to' request properties"* for
you when you're inside the framework ([protocol.js][pjs]); a raw client
writes them by hand.

[pjs]: https://firefox-source-docs.mozilla.org/devtools/backend/protocol.js.html

## Reply packet

```
{
  "from": "<actorID>",
  ...zero or more result fields...
}
```

There is **no `type` field on replies** and no echoed request id.
A successful reply is anything from the right `from` that isn't an
error and isn't a known event for that actor.

## Request / reply pairing

From [protocol][proto]:

> *"Each client request receives exactly one server reply, processed
> in order."* and *"Clients may pipeline multiple requests without
> waiting for individual replies, though implementations should
> maintain bounded pending request counts."*

The rule is **per-actor FIFO**:

- For a given actor, the server processes requests in arrival order
  and emits exactly one reply per request, in the same order.
- Different actors are independent: replies from actor A and actor B
  can interleave freely.
- Therefore a client's matching algorithm is: keep a FIFO queue of
  pending requests **per actor ID**, and pop the head of that queue
  when a packet arrives with that `"from"` (and is not an event).

## Notifications (events)

The protocol allows *"Request/Reply/Notify … spontaneous notifications
from the server."* ([protocol][proto]).

A notification looks like an extra server packet:

```
{ "from": "<actorID>", "type": "<eventName>", ...payload... }
```

It is **not** a reply and must not be paired with a pending request.
Heuristic: if the packet has a `"type"` and it matches a known event
for that actor, treat as event; otherwise treat as the next reply for
that actor. (The actor's spec — see [[../overview/actor-model]] —
defines its event names.)

Examples of common events:

- `target-available-form`, `target-destroyed-form` from a WatcherActor
- `resources-available-array`, `resources-updated-array`,
  `resources-destroyed-array` from a WatcherActor — see [[resources]]
- `paused`, `resumed`, `newSource` from a ThreadActor
- `tabNavigated`, `frameUpdate` from target actors

## Field naming conventions

- Actor handles inside payloads are referred to as **"forms"** — a
  small JSON dict including at minimum an `actor` field with the ID,
  plus type-specific metadata. Cache the whole form; the protocol
  occasionally adds fields.
- Arrays of forms tend to be named after the contents (`tabs`,
  `workers`, `addons`, `processes`).
- Booleans gating new features go under `"traits"` — see
  backward-compatibility in [[../overview/architecture]].

## Putting it together — one round-trip

```
client → 28:{"to":"root","type":"listTabs"}
server → 142:{"from":"root","tabs":[
   {"actor":"server1.conn0.tabDescriptor3","browserId":42,
    "title":"example.com","url":"https://example.com/","selected":true}
]}
```

That's the entire wire-level model. Errors are a tiny variant of the
reply shape — see [[error-handling]].
