---
title: "Tree Output Controls Design"
type: design
status: draft
date: 2026-04-07
tags: [design, output, tree, ux]
---

# Tree Output Controls Design

Design document for `--depth N` and `--max-chars N` tree output controls, to be implemented in iterations 22-24.

## Applies to

- `snapshot` command (full DOM tree)
- `a11y` command (accessibility tree)
- `dom tree` command (DOM subtree)

## Flags

### `--depth N`

Limit the tree traversal depth. Nodes beyond depth N are truncated with a marker:

```json
{"tag": "div", "children": "[... 42 more children]"}
```

Default depths (per command):
- `snapshot`: 3
- `a11y`: 3
- `dom tree`: 5

### `--max-chars N`

Limit the total output size in characters. Once the serialized output exceeds N characters, remaining branches are replaced with truncation markers.

Default: no limit (use `--depth` for size control).

## Truncation markers

All tree commands use the same truncation format:

- **Children truncated**: `"[... N more children]"` — the array of children is cut short
- **Subtree truncated**: `"[... subtree truncated at depth N]"` — the entire subtree is elided
- **Text truncated**: `"[... N more chars]"` — long text content is cut

## Output envelope

Tree commands follow the same envelope pattern as list commands:

```json
{
  "results": { "tag": "html", "children": [...] },
  "total": 1,
  "truncated": true,
  "depth": 3,
  "meta": { "host": "localhost", "port": 6000 }
}
```

When truncated, the envelope includes:
- `"truncated": true`
- `"depth": N` (the effective depth used)
- `"hint": "tree truncated at depth 3, use --depth N for deeper traversal"`

## Interaction with `--jq`

When `--jq` is set, tree output controls still apply first (depth/max-chars limiting happens before jq filtering). This keeps memory bounded regardless of the jq expression.

## Consistency principle

All tree-producing commands MUST follow this pattern. New tree commands should reuse the same truncation logic (to be provided as a shared utility in a future iteration).
