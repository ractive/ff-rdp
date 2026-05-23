---
type: rdp-note
tags: [rdp, firefox-server, resource, network]
date: 2026-05-23
firefox_files:
  - devtools/server/actors/resources/network-events-stacktraces.js
---

# Resource: `network-event-stacktrace`

Per-target watcher. Captures the JS stack at the moment a request is initiated, keyed by `channelId`.

## Payload

```
{
  resourceType: "network-event-stacktrace",
  resourceId: <channelId>,
  stacktrace: [{filename, lineNumber, columnNumber, functionName, asyncCause, sourceId}, …] | true,
  timeStamp,
  cause,
}
```

If only "stacktrace available" is known (e.g. for chunked tracking), `stacktrace` may be `true`. Fetch the full trace via [[rdp/actors/network-content]] `getStackTrace(resourceId)`.

## Gotchas

- Separate from [[rdp/resources/network-event|network-event]] so that the stacktrace overhead is opt-in.
- Required if you want the netmonitor "Initiator" column.
- Capturing stacks has measurable perf cost on JS-heavy pages.
