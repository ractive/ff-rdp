---
title: "Page Snapshot Format"
type: reference
date: 2026-05-15
tags:
  - snapshot
  - dom
  - aria
  - output
  - iter-60
---

# Page Snapshot Format

The `ff-rdp snapshot` command produces a compact DOM tree representation for LLM consumption.

## JSON form (default)

```json
{
  "tag": "html",
  "children": [
    {
      "tag": "main",
      "role": "main",
      "children": [
        {
          "tag": "h1",
          "children": ["Welcome back, James"]
        },
        {
          "tag": "a",
          "interactive": true,
          "attrs": {"href": "/users"},
          "children": ["Users"]
        }
      ]
    }
  ]
}
```

Each node:
- `tag` — lowercase HTML tag name
- `role` — semantic role (navigation, main, banner, etc.) or explicit `role=""` attribute
- `interactive` — true for A, BUTTON, INPUT, SELECT, TEXTAREA, DETAILS, SUMMARY
- `attrs` — key attributes: id, class, href, src, alt, type, name, value, placeholder, aria-label, aria-expanded, aria-hidden, data-testid
- `children` — nested nodes; strings are text nodes
- `truncated` — string describing children omitted at max depth

## Text form (--format text)

Indented tree with 2-space indent:

```
<html>
  <body>
    <main role=main>
      <h1>
        "Welcome back, James"
      <a [interactive] href="https://example.com/users">
        "Users"
      <table>
        <tr [interactive]>
          "James Admin Test 1 ..."
```

## Snapshot vs dom

| Feature | `snapshot` | `dom` (default) |
|---------|------------|-----------------|
| Output shape | Recursive DOM tree (walk whole page) | Flat list of matched elements |
| ARIA info | tag + role + key attrs | ref + role + name + level + state + actionable attrs |
| Best for | Page overview / LLM context | Element inspection / interaction target selection |
| Legacy shape | `--format html` | `--format html` |

## Options

- `--depth N` — maximum tree depth (default: 6)
- `--max-chars N` — maximum total text characters (default: 50000)
- `--format html` — reserved for future use; currently no-op (snapshot doesn't return raw HTML)

## Notes for agents

- Prefer `snapshot` for initial page orientation (understand structure)
- Use `dom <selector>` to get ARIA-tree records for elements you want to interact with
- The `ref` field in ARIA-tree output is a stable handle across calls within a session

See also: [[output-formats]]
