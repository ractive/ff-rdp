---
type: rdp-note
tags: [rdp, firefox-server, actor, network]
date: 2026-05-23
firefox_files:
  - devtools/server/actors/network-monitor/network-content.js
  - devtools/shared/specs/network-content.js
---

# NetworkContentActor (typeName `"networkContent"`)

Per-target content-process actor for **issuing** HTTP requests (as opposed to observing them).

- Source: `devtools/server/actors/network-monitor/network-content.js` (157 lines).
- Spec:   `devtools/shared/specs/network-content.js`.

## Methods

### `sendHTTPRequest(request: json) → number`

`request = { url, method, headers, body, cause, securityFlags }`.

- Builds an `nsIChannel` via `NetUtil.newChannel` with `loadingNode: targetActor.window.document`. This makes the request show up in the netmonitor as if the page fired it.
- Sets `LOAD_BYPASS_CACHE | INHIBIT_CACHING | LOAD_ANONYMOUS`.
- **CONNECT method is rejected**: `"The CONNECT method is restricted and cannot be sent by devtools"`.
- For `referer` header, uses `setNewReferrerInfo(value, UNSAFE_URL, true)` so the header sticks; other headers via `setRequestHeader`.
- Body via `nsIStringInputStream` + `explicitSetUploadStream`.
- Resolves with the channel ID **only after asyncFetch completes**, to maximise chance the redux store sees the request.
- Returns `{ channelId: number }` (spec returns `RetVal("number")` but impl returns dict — spec says number, impl says `{channelId}` — small inconsistency).

### `getStackTrace(resourceId: number) → json`

Looks up via `getResourceWatcher(targetActor, NETWORK_EVENT_STACKTRACE)`. Throws if `network-event-stacktrace` isn't being watched. Strips frames above the devtools eval boundary.

## Lifecycle

- Created per WindowGlobalTarget.
- No events.

## Gotchas for ff-rdp

- To replay/resend a request, this is the actor — paired with [[network-event]] on the observe side.
- `getStackTrace` requires having `watchResources(["network-event-stacktrace"])` first via [[watcher]].
- The fetched response is **anonymous** and **bypasses cache** — you cannot use this to read a normal cached resource. For that, use [[network-event]] `getResponseContent` after seeing the event.
