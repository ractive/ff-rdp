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

## Cascade inspector (iter-81)

`ff-rdp cascade <SEL> [--prop NAME]` issues `getApplied` and uses the rule's
`matchedSelectors` array (the subset of `selectors` that actually matched the
node) to compute specificity via `ff_rdp_core::css::specificity::compute`.

The cascade order applied by the CLI matches CSS Cascade Level 4:

1. `!important` declarations beat normal ones (origin order reversed).
2. Within an importance group: origin precedence (Author > User > UA for
   normal; flipped for `!important`).  Origins are classified heuristically
   from the stylesheet href: `resource://`/`chrome://` → `ua`, empty → `inline`,
   anything else → `author`.  Distinguishing user-stylesheet origin needs a
   separate stylesheet query; tracked as future work.
3. Specificity tuple `(a, b, c)` comparison.
4. Document order (`source_order`) — later wins.

The `AppliedRule` struct exposes `matched_selectors` and `media` fields from
the response; both are populated when `getApplied` sends `matchedSelectors:
true` and the rule sits inside an `@media` block respectively.
