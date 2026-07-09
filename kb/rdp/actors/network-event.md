---
type: rdp-note
tags:
  - rdp
  - firefox-server
  - actor
  - network
date: 2026-05-23
firefox_files:
  - devtools/server/actors/network-monitor/network-event-actor.js
  - devtools/shared/specs/network-event.js
title: NetworkEventActor
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

## `getSecurityInfo` (iter-104 — `network --security`)

`getSecurityInfo` returns the request's cached `_securityInfo` (see
`network-event-actor.js:340-360`). The payload is:

```
{ state, protocolVersion, cipherSuite, weaknessReasons, hsts, hpkp,
  cert: { subject.commonName, issuer.commonName, validity.{start,end},
          fingerprint.sha256 }, kea, sig, errorMessage, usedEch/… }
```

ff-rdp's `NetworkEventActor::get_security_info` projects the audit-relevant
subset into `SecurityInfo` (`protocolVersion`, `cipherSuite`, `hsts`,
`weaknessReasons`, curated `cert` summary) and returns `None` when Firefox
attaches no security info.

**Population constraint (important):** `_securityInfo` is populated **only when
the response was observed** by the watcher (`network-event-actor.js:690-710`).
Consequences for ff-rdp:

- **Plain-HTTP requests** have no TLS handshake → `securityInfo` is `null` →
  `get_security_info` returns `None`. The `network --security` CLI reports these
  as `security: null` and counts them under a top-level `insecure_requests`.
- **A request the watcher never observed** (e.g. it loaded before the daemon
  subscribed, or the one-shot `--no-daemon` window opened after the response)
  also returns `None`. Security info therefore exists only for requests captured
  in a daemon-buffered or `--with-network` observation window — the same
  requests that carry a NetworkEventActor id in the first place. The
  performance-api fallback has no NetworkEventActor ids at all, so
  `network --security` emits a per-entry note there instead of security data.

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
