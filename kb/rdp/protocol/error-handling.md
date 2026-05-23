---
type: rdp-note
tags: [rdp, official-docs, protocol, errors]
date: 2026-05-23
title: Error handling
sources:
  - https://firefox-source-docs.mozilla.org/devtools/backend/protocol.html
  - https://firefox-source-docs.mozilla.org/devtools/backend/backward-compatibility.html
---

# Error handling

A failed request comes back as a normal reply packet with an `error`
field. From [protocol][proto]:

> *"Standard error replies follow this structure:
> `{ "from":actor, "error":name, "message":message }`"*

[proto]: https://firefox-source-docs.mozilla.org/devtools/backend/protocol.html

## Shape

```json
{
  "from":    "<actorID>",
  "error":   "<machineReadableName>",
  "message": "<human-readable explanation>"
}
```

- `error` is a stable, machine-readable identifier (camelCase, no
  spaces).
- `message` is freeform and may change between Firefox versions —
  **do not match on it**.
- The error packet **counts as the one reply** for the request that
  caused it (per [[message-format]] §pairing). Pop the pending request
  off the per-actor FIFO.

## Common error names

Documented in [protocol][proto]:

| `error` name              | When                                                  |
| ------------------------- | ------------------------------------------------------ |
| `noSuchActor`             | Actor ID is not (or no longer) known to the server     |
| `unrecognizedPacketType`  | Actor exists but doesn't understand this `type`        |
| `missingParameter`        | Required field absent from the request                 |
| `badParameterType`        | Field present but wrong type / value                   |

Other names appear in the wild (and in [searchfox][sf] grepping
`error:`):

| `error` name              | Typical meaning                                        |
| ------------------------- | ------------------------------------------------------ |
| `wrongState`              | Actor is in a state where this request isn't valid     |
| `wrongOrder`              | Sequenced requests arrived out of order                |
| `notImplemented`          | Stub method, or unsupported on this build              |
| `unknownActor`            | Same flavour as `noSuchActor`; older naming            |
| `protocolError`           | Generic framing or schema violation                    |

[sf]: https://searchfox.org/mozilla-central/source/devtools/

Names not in the official table are not guaranteed stable — treat
unrecognised `error` strings as "permanent failure of this request".

## Feature-detection vs error-catching

Per [backward-compatibility][bc]:

> *"Clients can synchronously check for actor support using
> `toolbox.target.hasActor("actorTypeName")` … When an older server
> lacks a trait, it defaults to false, allowing graceful feature
> degradation."*

Preferred order for a client:

1. **Check traits** on the root greeting / actor form first.
2. **Check `hasActor()`** on the form (equivalent: look for the actor
   in the descriptor/target form).
3. Only then send the request and *also* be ready for
   `unrecognizedPacketType` or `noSuchActor` if you guessed wrong.

[bc]: https://firefox-source-docs.mozilla.org/devtools/backend/backward-compatibility.html

## Lifecycle-driven errors

Because *"once a parent is removed from the pool, its children are
removed as well"* ([actor-hierarchy][ah]), in-flight requests to a
soon-to-die actor will commonly return `noSuchActor` rather than a
useful reply. The robust pattern:

- Subscribe to the relevant `target-destroyed-form` /
  `resources-destroyed-array` events ([[resources]]).
- Cancel pending requests for actors below a destroyed parent yourself
  — don't wait for individual errors to clean up state.

[ah]: https://firefox-source-docs.mozilla.org/devtools/backend/actor-hierarchy.html

## Transport-level failure modes

The transport spec ([[transport]]) doesn't define a "protocol error
frame" — if framing breaks (bad length, non-UTF-8 inside JSON,
unparseable JSON), the server typically just closes the socket.
Clients should treat sudden close as fatal: there is no resume.

## What about exceptions in the page?

Errors that happen *inside the debuggee* (e.g. a thrown exception from
`evaluateJSAsync`) are **not** RDP errors. They are normal successful
replies whose payload encodes the exception (typically an
`exception`/`exceptionMessage`/`exceptionStack` field). The request
succeeded — it just brought back bad news.
