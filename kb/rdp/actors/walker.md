---
type: rdp-note
tags:
  - rdp
  - firefox-server
  - actor
  - inspector
  - dom
date: 2026-05-23
firefox_files:
  - devtools/server/actors/inspector/walker.js
  - devtools/server/actors/inspector/inspector.js
  - devtools/shared/specs/walker.js
title: WalkerActor (DOM inspector)
---

# WalkerActor (typeName `"domwalker"`)

The DOM tree access actor. Reached via `inspector.getWalker()`.

- Source: `devtools/server/actors/inspector/walker.js` (2906 lines — by far the largest actor).
- Spec:   `devtools/shared/specs/walker.js`.

## Selected methods (full set is huge — see spec)

- `document(nodeFront?)` / `getRootNode()` — entry node.
- `querySelector(baseNode, selector)` — `nullable:domnode`.
- `querySelectorAll(node, selector)` — returns a `domnodelist`.
- `multiFrameQuerySelectorAll(selector)` — crosses iframe boundaries.
- `getNodeFromActor(actorID, path)` — given an actor and a property path, return the underlying DOMNode.
- `children(node, {maxNodes, center, start})`, `nextSibling`, `previousSibling`, `parents`.
- `removeNode`, `insertBefore`, `editTagName`, `setOuterHTML`, `setInnerHTML`, `getOuterHTML`, `getInnerHTML`.
- `setAttribute(node, name, value)`, `removeAttribute`.
- `getNodeActorFromContentDomReference(domReference)` — resolves cross-frame refs.
- `getOffsetParent`, `getClosestBackgroundColor`, `getEmbedderElement`.
- `getMutations(opts)`, `clearPseudoClassLocks`, `addPseudoClassLock`, `removePseudoClassLock`.
- `getEventListenerInfo(node)` — via `event-collector.js`.
- `search(query, opts)` — `searchresult` `{list, metadata}`.
- `pickerNodePicked` / `cancelPick` / `pick(doFocus)` — the node-picker (alt+click in devtools).

## Events

- `new-mutations` → batched via `getMutations()`. Walker accumulates and signals; client polls.
- `root-available`, `root-destroyed` — top-document changes (navigation).
- `picker-node-picked`, `picker-node-previewed`, `picker-node-hovered`, `picker-node-canceled`.
- `display-change`, `scrollable-change`, `overflow-change`, `container-type-change`, `anchor-name-change` — layout/scroll observer notifications.
- `resize` — window resize.

## Lifecycle

- Created on demand by the InspectorActor (`inspector.js`).
- One per target. Lives until target destroyed.
- Holds a `DocumentWalker` (anonymous-content–aware) and a `CustomElementWatcher`.

## Gotchas

- **Mutation events are throttled and pull-based**: `new-mutations` is just a signal; you must call `getMutations` to drain.
- Node identity uses `NodeActor` actorIDs — refer to nodes by their actorID across requests.
- For shadow DOM access, walker is shadow-aware (anonymous content), but selector queries don't pierce shadow boundaries by default.
- Cross-frame: `multiFrameQuerySelectorAll` exists, but normal `querySelector` is single-document.
- Avoid `getOuterHTML` on huge documents — it returns a LongStringActor; you'll have to substring it.
- `setOuterHTML` can invalidate every NodeActor in the subtree; expect a flood of `root-available`/`new-mutations` after.
- **`nodeValue` and attribute *values* are `longstring` slots** (`devtools/shared/specs/node.js`): a text node above ~10 KB, or a large attribute value (e.g. an inline `src` data URI), arrives as a `{type:"longString", …}` grip, not an inline string. Since iter-102, `parse_dom_node` (`actors/dom_walker.rs`) resolves both through `specs::types::resolve_long_string_slot`, so large `nodeValue`/attribute values are returned in full instead of dropped to empty. Unit tests: `parse_dom_node_resolves_longstring_node_value`, `parse_dom_node_resolves_longstring_attr_value`. Live AC: `live_dom_text_longstring_roundtrip` (`live_102_longstring_and_reload.rs`). See [[lessons-learned#longstring-grips-everywhere]].
