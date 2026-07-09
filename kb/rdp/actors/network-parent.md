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

## ff-rdp wiring (iter-109)

`NetworkParentFront` (`crates/ff-rdp-core/src/fronts/network_parent.rs`) is the
typed handle. It is obtained from `WatcherFront::get_network_parent_actor`
(nested `{network: {actor}}` reply — the key is `network`, NOT `networkParent`;
iter-109 guessed `networkParent` and shipped a decode bug caught by iter-110's
live sweep and fixed after capturing the real Firefox 152 reply
`{"network":{"actor":"…networkParent12"},"from":…}` — see [[watcher]]) and
drives the `throttle` CLI command:

| Front method | Wire call | CLI surface |
|---|---|---|
| `set_network_throttling(profile)` | `setNetworkThrottling({latency, downloadThroughput, uploadThroughput})` | `throttle slow-3g` / `throttle fast-3g` |
| `clear_network_throttling()` | `setNetworkThrottling(null)` | `throttle off` |
| `set_blocked_urls(urls)` | `setBlockedUrls({urls})` | `throttle --block <pat>…` / `throttle --unblock` |

`ThrottleProfile` presets (bytes/sec, ms latency): **slow-3g** = 50 000 / 50 000
/ 400 ms; **fast-3g** = 200 000 / 93 750 / 150 ms (canonical DevTools tiers).

### Protocol quirk — response-less but NOT oneway

`setNetworkThrottling` and `setBlockedUrls` declare **no response block** in
`devtools/shared/specs/network-parent.js`, but they are **not** marked
`oneway`. Firefox still sends an empty ACK the client must read — the same shape
as `walker.releaseNode`. `NetworkParentFront` therefore uses `actor_request`
(reads the reply), **not** `actor_send`. Do not add `ONEWAY = true` for these.

## Gotchas

- **Body capture is opt-in via `setSaveRequestAndResponseBodies(true)`.** Without it, `getResponseContent` on NetworkEventActor will return `contentDiscarded: true`.
- Throttling units: latency in ms, throughput in **bytes per second** (despite the field name).
- `override(url, path)` requires a parent-process file path — won't work for content sandboxed paths.
- **`throttle`/`--block` require `watchResources(["network-event"])` first** — every network-parent method throws `"Not listening for network events"` otherwise. The `throttle` command subscribes before configuring.
- **Lifetime = the RDP connection.** Throttling/blocking die when the connection that set them closes. Under `--no-daemon` the one-shot process disconnects immediately (setting discarded → envelope carries `lifetime_warning`); use the daemon to keep it active across commands.
