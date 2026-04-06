---
title: "Iteration 5: DOM + Page Text"
type: iteration
date: 2026-04-06
tags:
  - iteration
  - dom
  - inspector
  - page-text
status: completed
branch: iter-5/dom-page-text
---

# Iteration 5: DOM + Page Text

DOM inspection via the Inspector/Walker actors and visible text extraction.

## Tasks

- [x] Implement `ff-rdp-core/src/actors/inspector.rs` — `InspectorActor` with `get_walker()`
- [x] Implement `ff-rdp-core/src/actors/inspector.rs` — `WalkerActor` with `document()`, `querySelector(node, selector)`, `querySelectorAll(node, selector)`
- [x] Implement DOM node serialization: tag name, attributes, text content, children (with configurable depth)
- [x] Implement `ff-rdp-cli/src/commands/dom.rs` — `ff-rdp dom <selector> [--tab ...] [--outer-html|--inner-html|--text|--attrs]`
- [x] Implement `ff-rdp-cli/src/commands/page_text.rs` — `ff-rdp page-text [--tab ...]` using eval `document.body.innerText` as efficient fallback
- [x] Consider: should `dom` use native Inspector actors or eval-based approach? Eval is simpler and covers most cases.

## Design Decision

For the initial implementation, `dom` and `page-text` can both use `eval` internally:

```javascript
// dom --selector "#content" --outer-html
document.querySelector("#content").outerHTML

// dom --selector "#content" --text
document.querySelector("#content").textContent

// page-text
document.body.innerText
```

Native Inspector/Walker actors provide more structured data (node trees, computed styles) but are significantly more complex. Use eval first, add native inspector support later if needed.

**Note from iteration 4:** Eval results exceeding ~1000 chars are returned as `longString` grips. The `LongStringActor::full_string()` method (added in iter 4) must be used to fetch the complete content. Both `page-text` and `dom --outer-html` will hit this on real pages.

## Acceptance Criteria

- `ff-rdp dom "h1"` returns the first h1 element's outerHTML
- `ff-rdp dom "h1" --text` returns just the text content
- `ff-rdp dom "ul li" --text` returns text of all matching elements as array
- `ff-rdp page-text` returns all visible text from the page
- `ff-rdp page-text --jq '.results | length'` counts characters (rough page size indicator)
