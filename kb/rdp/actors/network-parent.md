---
type: rdp-note
tags:
  - rdp
  - firefox-server
  - actor
  - network
date: 2026-05-23
firefox_files:
  - devtools/server/actors/network-monitor/network-parent.js
  - devtools/shared/specs/network-parent.js
title: NetworkParentActor
---

# NetworkParentActor (typeName `"networkParent"`)

Parent-process network configuration actor. Created by [[watcher]] via `getNetworkParentActor()`.

- Source: `devtools/server/actors/network-monitor/network-parent.js` (176 lines).
- Spec:   `devtools/shared/specs/network-parent.js`.

## Methods (all delegate to the underlying NetworkEventWatcher resource)

| Method | Effect |
|---|---|
| `setNetworkThrottling({latency, downloadThroughput, uploadThroughput} \| null)` | Apply throttling. Internally expands to `latencyMean/Max`, `downloadBPSMean/Max`, `uploadBPSMean/Max`. |
| `getNetworkThrottling()` | Returns current `{downloadThroughput, uploadThroughput, latency}` or null. |
| `clearNetworkThrottling()` | Restores `defaultThrottleData` captured on first set. |
| `setSaveRequestAndResponseBodies(save: bool)` | Enable/disable request/response body capture (else they're discarded). |
| `setBlockedUrls(urls: string[])` | Replace the block-list. |
| `getBlockedUrls() → string[]` | |
| `blockRequest(filters)` | Add a block filter. |
| `unblockRequest(filters)` | Remove. |
| `setPersist(enabled)` | When true, network events survive navigation (the netmonitor "Persist Logs" toggle). |
| `override(url, path)` | Local file override for a given URL (Firefox's response override). |
| `removeOverride(url)` | |

**Every method throws `"Not listening for network events"` unless `watchResources(["network-event"])` was issued first** (except `setPersist`, which silently no-ops).

## Lifecycle

- Owned by the [[watcher]]. One per debugging session.
- No events.

## Gotchas

- **Body capture is opt-in via `setSaveRequestAndResponseBodies(true)`.** Without it, `getResponseContent` on NetworkEventActor will return `contentDiscarded: true`.
- Throttling units: latency in ms, throughput in **bytes per second** (despite the field name).
- `override(url, path)` requires a parent-process file path — won't work for content sandboxed paths.
