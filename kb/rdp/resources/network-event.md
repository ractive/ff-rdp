---
type: rdp-note
tags: [rdp, firefox-server, resource, network]
date: 2026-05-23
firefox_files:
  - devtools/server/actors/resources/network-events.js
  - devtools/server/actors/resources/network-events-content.js
---

# Resource: `network-event`

The big one. **Watched at the WATCHER (parent-process) level**, not per-target, so a single subscription captures cross-origin / cross-process / cross-tab traffic in scope.

- Watcher: `devtools/server/actors/resources/network-events.js` — observes nsIHTTPChannel via `network-monitor/NetworkObserver.sys.mjs`.
- Content-side companion: `network-events-content.js` (for some content-process derived data).

## Payload at availability time

The resource is the `asResource()` of a [[rdp/actors/network-event]] actor — see that file for fields. Key inline fields:

```
{
  actor: <NetworkEventActor id>,
  resourceType: "network-event",
  resourceId, channelId, startedDateTime,
  request: { url, method, headers count, cause, isXHR, isThirdParty, ... },
  cause: { type, loadingDocumentUri, stacktraceAvailable },
  ...
}
```

## Lifecycle within the stream

1. `resources-available-array` with the initial resource (request just fired).
2. Multiple `resources-updated-array` deltas (or per-event-actor `network-event-update:*` events) as headers/cookies/post-data/response-start/response-content/timings arrive.
3. Eventually steady state. No "destroy" unless `clearResources(["network-event"])` is called or `release` is invoked on the per-request actor.

## Configuration knobs

Controlled via [[rdp/actors/network-parent]] (obtained from watcher.getNetworkParentActor):

- `setSaveRequestAndResponseBodies(true)` — required for `getResponseContent()` to return data.
- `setBlockedUrls`, `blockRequest`, `setNetworkThrottling`, `setPersist`, `override`.

## Gotchas for ff-rdp

- **Body capture is opt-in.** Without `setSaveRequestAndResponseBodies(true)`, all bodies report `contentDiscarded: true`.
- The per-request data is **NOT included** in the initial payload — headers, body, timings each require a separate request to the spawned NetworkEventActor.
- Resource arrives BEFORE the response — initial event has only request side. Watch for `network-event-update:response-content` before fetching body.
- For navigation requests, the actor's `innerWindowId` is the **new** window-global once known; before that it's null. Don't use innerWindowId to correlate navigation requests with their tab.
- See [[network-event-decoded-body-size]] and [[network-event-stacktrace]] — split so they can be requested separately and updated lazily.
