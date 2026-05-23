---
type: rdp-note
tags: [rdp, firefox-server, resource, debugger]
date: 2026-05-23
firefox_files:
  - devtools/server/actors/resources/sources.js
  - devtools/server/actors/source.js
---

# Resource: `source`

Frame-target resource. Spawns one entry per JS source seen by the SpiderMonkey Debugger API.

## Payload

```
{
  resourceType: "source",
  actor: <SourceActor id>,
  url, sourceMapURL, sourceMapBaseURL,
  introductionType, introductionUrl,
  isBlackBoxed, sourceLength,
  isPrettyPrinted, isInlineSource, isExtensionSource,
}
```

The SourceActor methods (`getBreakableLines`, `getBreakableOffsets`, `setBreakpoint`, …) live in `source.js`.

## Gotchas

- Inline `<script>` blocks each become separate sources (with `isInlineSource: true`).
- Source-mapped originals are loaded **on demand** — the raw source is always emitted; mapped originals appear when the client requests them via the source-map service.
- `introductionType`: `"scriptElement" | "eval" | "Function" | "javascriptURL" | "importedModule"` — useful for filtering eval'd code.
