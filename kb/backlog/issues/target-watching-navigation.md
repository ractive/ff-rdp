---
title: Target watching for seamless navigation handling
type: feature
status: resolved
priority: medium
discovered: 2026-04-07
tags:
  - watcher
  - target
  - protocol
  - navigate
---

# Target watching for seamless navigation handling

Currently ff-rdp reconnects to the target after navigation. The WatcherActor supports
`watchTargets("frame")` which sends `target-available-form` / `target-destroyed-form`
events, enabling seamless handling of page transitions and iframe creation.

## Benefits

- No need to re-resolve target actor after navigation
- Automatic tracking of new documents (SPAs, iframes)
- Foundation for `--follow` streaming mode

## Protocol

```json
{"to": "<watcherActor>", "type": "watchTargets", "targetType": "frame"}
// Events: target-available-form, target-destroyed-form
```
