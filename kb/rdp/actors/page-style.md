---
type: rdp-note
tags:
  - rdp
  - firefox-server
  - actor
  - inspector
  - css
date: 2026-05-23
firefox_files:
  - devtools/server/actors/page-style.js
  - devtools/shared/specs/page-style.js
title: PageStyleActor
---

# PageStyleActor (typeName `"pagestyle"`)

Computed styles, matched rules, font usage, layout boxes. Spawned by the InspectorActor.

- Source: `devtools/server/actors/page-style.js` (1712 lines).
- Spec:   `devtools/shared/specs/page-style.js`.

## Methods

| Method | Returns |
|---|---|
| `getComputed(node, {markMatched, onlyMatched, clearCache, filter, filterProperties})` | `{computed: json}` — full computed style map. |
| `getMatchedSelectors(node, property, {filter})` | `{rules, matched}` |
| `getApplied(node, {inherited, matchedSelectors, skipPseudo, filter})` | `appliedStylesReturn` — full cascade with inherited rules. |
| `getRule(ruleId)` | `domstylerule` |
| `getLayout(node, …)` | box-model values (margin/border/padding/content + position). |
| `isPositionEditable(node)` | bool |
| `getAllUsedFontFaces({includePreviews, includeVariations, previewText, previewFontSize, previewFillStyle})` | `array:fontface` |
| `getUsedFontFaces(node, …)` | per-node fonts |

## Events

- `stylesheet-updated` — emitted when a stylesheet changes (re-applied by user, hot reload, …).

## Lifecycle

- One per target; created lazily by InspectorActor's `getPageStyle()`.
- Caches computed-style results until `clearCache: true` is passed.

## Gotchas

- `getApplied` is **the** call for "give me the cascade for this element" — heavier than `getComputed` because it includes rule sources.
- `filter` values are `"all" | "ua" | "user"` (filter out user-agent stylesheets, etc).
- Returns `domstylerule` types defined in `devtools/shared/specs/style/style-types.js` — these reference the underlying `StyleSheetActor` (see also the [[rdp/resources/stylesheet]] resource) which you can mutate to live-edit CSS.
