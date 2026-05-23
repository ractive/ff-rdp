---
type: rdp-note
tags: [rdp, firefox-server, resource, css]
date: 2026-05-23
firefox_files:
  - devtools/server/actors/resources/css-changes.js
  - devtools/server/actors/changes.js
---

# Resource: `css-change`

Frame-target resource. Emitted whenever DevTools (the inspector / rules pane) **edits** a CSS property on a style rule. Used by the "Changes" panel.

## Payload (a "change" entry)

```
{
  resourceType: "css-change",
  source: { type, href, index, sourceId, ... },
  ruleId, selector, type: "set" | "remove" | "add",
  property, value, previousValue, oldValue,
  ancestors: array,    // parent rules (for @media etc.)
}
```

## Gotchas

- Only fires for **devtools-driven** edits via [[../actors/page-style]] / style-rule actors, NOT for page-script `element.style.x = …` or `sheet.insertRule(...)`.
- The data is also queryable via `ChangesActor` (`actors/changes.js`) which tracks the full session diff and can produce CSS snippets for copy/export.
