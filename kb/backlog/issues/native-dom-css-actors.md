---
title: "Native DOM/CSS inspection via Inspector, Walker, and PageStyle actors"
type: feature
status: open
priority: high
discovered: 2026-04-07
tags: [dom, css, protocol, inspector, layout]
---

# Native DOM/CSS inspection via Inspector, Walker, and PageStyle actors

Essential for debugging layout issues during development — understanding why elements
overlap, which CSS rules win, what the actual box model dimensions are.

## What eval can't do well

- **CSS cascade**: which rule from which stylesheet applies, selector specificity, !important
- **Box model breakdown**: separate margin/border/padding/content values per side
- **Applied rules**: all CSS rules targeting an element, in priority order
- **Inherited styles**: which properties come from parent elements
- **DOM tree structure**: efficient parent/child/sibling traversal without serializing entire subtrees

## Actors involved

- **InspectorActor**: entry point, provides walker and pagestyle references
- **WalkerActor**: DOM tree traversal — `querySelector`, `children`, `parents`, `nextSibling`
- **NodeActor**: per-node operations — attributes, innerHTML, highlight, scrollIntoView
- **PageStyleActor**: `getComputed(node)`, `getApplied(node)`, `getLayout(node)` (box model)

## Proposed commands

```sh
ff-rdp styles ".my-element"              # computed styles
ff-rdp styles ".my-element" --applied    # applied rules with source locations
ff-rdp styles ".my-element" --layout     # box model (margin/border/padding/content)
ff-rdp dom tree ".my-element" --depth 3  # structured DOM subtree
```
