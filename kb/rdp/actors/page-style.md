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
| `getApplied`-raw (ff-rdp `get_applied_raw`, iter-82) | Same `getApplied` request as above, but returns the *uninterpreted* reply as `serde_json::Value`. Backs the `ff-rdp cascade --debug-raw` escape hatch added in iter-82 so protocol-shape drift can be diagnosed without a rebuild. |
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

## iter-84 fixes: rule type field and actor ID deduplication

### Theme A — absent `type` field on external-stylesheet rules

Firefox sometimes omits the `type` field on CSS rules from external
stylesheets (observed on css.gg and similar icon libraries).

**Before**: `parse_applied_entry` used `?` on `rule.get("type")` which
returned `None` (skipped the rule) when the field was absent.

**After**: a `match` on `rule.get("type").and_then(Value::as_u64)` treats
`None` (absent) identically to `Some(1)` (stylesheet rule) — both are kept.
`Some(0)` (inline style) and all other values are still rejected.

### Theme E — rule deduplication via `rule_actor_id`

When the same CSS rule matches via multiple inheritance paths (e.g. a `*`
selector applied to every ancestor), `getApplied` returns the rule once per
matched element, producing duplicates in the applied-styles output.

**Change**: `AppliedRule` now carries a `rule_actor_id: Option<ActorId>`
field populated from `rule.actor` in the `getApplied` response. The CLI's
`styles applied` command deduplicates by this field before building the
JSON output — rules with `None` (Firefox omitted the field) pass through
unchanged because there is no safe key to merge on.

`rule_actor_id` is serialized only when `Some`
(`#[serde(default, skip_serializing_if = "Option::is_none")]`).
