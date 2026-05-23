---
type: rdp-note
tags:
  - rdp
  - firefox-server
  - resource
  - css
date: 2026-05-23
firefox_files:
  - devtools/server/actors/resources/stylesheets.js
  - devtools/server/actors/style-sheets.js
  - devtools/server/actors/stylesheets/
title: "Resource: stylesheet"
---

# Resource: `stylesheet`

Frame-target resource. One per `<style>` / `<link rel=stylesheet>` / constructable stylesheet attached to the document.

Watcher provides all three callbacks: `onAvailable`, `onUpdated`, `onDestroyed`. (Most resource types only use `onAvailable`.)

## Payload

```
{
  resourceType: "stylesheet",
  actor: <StyleSheetActor id>,
  href, title, disabled, system,
  ruleCount, sourceMapURL, sourceMapBaseURL, mediaRules,
  styleSheetIndex, fileName, atRules, isNew, ...
}
```

## Updates

The watcher tracks applicable stylesheet add/remove via `StyleSheetsManager` and emits updates when stylesheets are toggled disabled/enabled or their rule counts change after `setRuleText` (live edit).

## Gotchas

- Constructable stylesheets (`new CSSStyleSheet().replace(...)`) appear here too.
- "system" stylesheets (UA stylesheets, chrome) are filtered out by default — only "author" sheets for normal sessions.
- Updates may arrive shortly **after** the resource is "available" — UI should re-render on `onUpdated`.
