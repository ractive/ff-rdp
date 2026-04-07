---
title: "Accessibility tree command via AccessibilityActor"
type: feature
status: open
priority: high
discovered: 2026-04-07
tags: [accessibility, a11y, protocol, ai-agent]
---

# Accessibility tree command via AccessibilityActor

Chrome MCP's `read_page` returns a structured accessibility tree — semantic roles,
names, states, ref IDs — and this is the primary way AI agents "read" and understand
a page without taking screenshots. ff-rdp has no equivalent.

## Why this matters for AI agents

An AI agent needs to understand page structure to:
- Navigate and interact ("click the submit button")
- Verify UI state ("is the dropdown open?")
- Check accessibility compliance
- Understand form structure (labels, required fields, error states)

Currently ff-rdp can only `eval document.querySelectorAll(...)` which returns raw
HTML/text, not semantic structure.

## Proposed command

```sh
ff-rdp a11y                              # full accessibility tree
ff-rdp a11y --depth 3                    # limit depth
ff-rdp a11y --selector ".main-content"   # subtree only
ff-rdp a11y --interactive                # only interactive elements (buttons, links, inputs)
```

## Output structure

```json
{
  "role": "document",
  "name": "Page Title",
  "children": [
    {
      "role": "navigation",
      "name": "Main menu",
      "children": [
        {"role": "link", "name": "Home", "url": "/"},
        {"role": "link", "name": "About", "url": "/about"}
      ]
    },
    {
      "role": "main",
      "children": [
        {"role": "heading", "name": "Welcome", "level": 1},
        {"role": "form", "name": "Search", "children": [
          {"role": "textbox", "name": "Query", "value": "", "required": true},
          {"role": "button", "name": "Search"}
        ]}
      ]
    }
  ]
}
```

## Protocol

Firefox's AccessibilityActor provides:
- `getWalker` → AccessibleWalkerActor
- Walker: `children`, `getAccessibleFor(domNode)`
- Each accessible: role, name, value, description, states, attributes
