---
type: rdp-note
tags: [rdp, firefox-server, resource, performance]
date: 2026-05-23
firefox_files:
  - devtools/server/actors/resources/reflow.js
  - devtools/server/actors/reflow.js
---

# Resource: `reflow`

Frame-target resource. Emits one entry each time the layout engine performs a reflow.

## Payload

```
{
  resourceType: "reflow",
  interruptible: boolean,
  start: number,           // monotonic ms
  end: number,
  sourceURL?: string,      // approximate trigger JS
  sourceLine?: number,
  functionName?: string,
}
```

## Gotchas

- Only enabled for FRAME targets (not workers, not process targets).
- High-frequency: scrolling can produce reflows at frame rate. Always batch handling.
- The `sourceURL`/`functionName` fields are best-effort — Gecko's reflow does not always know who triggered it.
