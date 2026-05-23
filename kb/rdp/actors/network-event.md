---
type: rdp-note
tags: [rdp, firefox-server, actor, network]
date: 2026-05-23
firefox_files:
  - devtools/server/actors/network-monitor/network-event-actor.js
  - devtools/shared/specs/network-event.js
---

# NetworkEventActor (typeName `"netEvent"`)

**One actor per HTTP request.** Created by the `network-event` ResourceWatcher (see [[rdp/resources/network-event]]).

- Source: `devtools/server/actors/network-monitor/network-event-actor.js` (849 lines).
- Spec:   `devtools/shared/specs/network-event.js`.

Constructed by the network-event resource watcher with `(conn, sessionContext, {onNetworkEventUpdate, onNetworkEventDestroy}, networkEventOptions, channel)`.

## asResource()

Returns the inline payload used inside `resources-available-array`:

```
{ actor: <actorID>, …this._resource }
```

The `_resource` is built from the nsIChannel: url, method, isXHR, cause, requestHeaders count, …

## Pull methods (client must call these to fetch heavy data — NOT pushed)

| Method | Returns |
|---|---|
| `getRequestHeaders` | `{headers: [{name, value}], headersSize, rawHeaders: longstring}` |
| `getRequestCookies` | `{cookies: [{name, value}]}` |
| `getRequestPostData` | `{postData: {text: longstring}, postDataDiscarded}` |
| `getEarlyHintsResponseHeaders` | `{headers, headersSize, rawHeaders}` |
| `getResponseHeaders` | same shape |
| `getResponseCookies` | `{cookies}` |
| `getResponseCache` | `{content}` |
| `getResponseContent` | `{content: {text: longstring}, contentDiscarded}` |
| `getEventTimings` | `{timings: {blocked, dns, ssl, connect, send, wait, receive}, totalTime, offsets, serverTimings}` |
| `getSecurityInfo` | `{state, weaknessReasons, cipherSuite, kea/sig/protocolVersion, cert, hsts, hpkp, errorMessage, usedEch/DelegatedCredentials/Ocsp/PrivateDns}` |
| `getStackTrace` | json |
| `release` | `release: true` — destroys this actor. |

**Response bodies are longstrings** — for large responses you receive a LongStringActor reference and have to follow up with `substring(start, end)` to stream it out.

## Events (all re-emitted client-side as `networkEventUpdate`)

| Server event | Carries |
|---|---|
| `network-event-update:headers` | headers count, headersSize |
| `network-event-update:cookies` | cookies count |
| `network-event-update:post-data` | dataSize |
| `network-event-update:response-start` | `response: json` |
| `network-event-update:security-info` | state |
| `network-event-update:response-content` | mimeType, contentSize, encoding, transferredSize, blockedReason, extension |
| `network-event-update:event-timings` | totalTime |
| `network-event-update:response-cache` | (signal only) |

Each event has `updateType: <"headers"|"cookies"|…>` as Arg(0) so a single client handler can demultiplex.

**Why distinct events?** The spec comment says: *"We use individual event at protocol.js level to workaround performance issue with `Option` types. (See bug 1449162)"* — protocol.js's Option type is slow, so each `updateType` is its own event packet.

## Lifecycle

- Spawned by `NetworkEventWatcher` (`actors/resources/network-events.js`) when nsIHTTPChannel fires `http-on-modify-request`.
- Lives until the channel is fully complete (response body + timings delivered), then destroyed when the client calls `release` or the network watcher batches a `resources-destroyed-array`.
- If the channel is for a destroyed window (innerWindowId no longer valid), the watcher cleans it up early.

## Special-case channels

- `nsIDataChannel` / `nsIFileChannel`: skips header fetching, marks `_isNavigationRequest = false`, empty cookies/headers.
- Redirect channels: `_isRedirect = true`, only chained from original.

## Gotchas for ff-rdp

- The `available` event only tells you a request started — you must call `getResponseHeaders` / `getResponseContent` **after** seeing the matching `network-event-update:response-content` event, else the body may not be ready yet.
- `transferredSize` (post-decode bytes from the wire) vs `contentSize` (decoded size) vs `decodedBodySize` (separate resource type, see [[rdp/resources/network-event-decoded-body-size]]) are three different numbers.
- `serverTimings` is part of `getEventTimings`, not headers — parsed from `Server-Timing` response header.
- Storing all NetworkEventActors in memory will leak; either call `release` after extracting what you need or `Watcher.clearResources(["network-event"])`.
