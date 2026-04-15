---
title: "page-snapshot: combined structural dump for AI agent page understanding"
type: feature
status: resolved
priority: high
discovered: 2026-04-07
tags:
  - dom
  - accessibility
  - ai-agent
  - snapshot
---

# page-snapshot: combined structural dump for AI agent page understanding

Chrome MCP's `read_page` is the #1 tool AI agents use to understand a page. ff-rdp
needs an equivalent — a single command that returns everything an agent needs to
"read" a page and decide what to do next.

## Proposed command

```sh
ff-rdp snapshot                           # full page snapshot
ff-rdp snapshot --depth 5                 # limit tree depth
ff-rdp snapshot --selector ".main"        # subtree only
ff-rdp snapshot --interactive             # only actionable elements
```

## Output combines

1. **DOM structure** — tag hierarchy with key attributes (id, class, role, href, src)
2. **Accessibility roles** — semantic meaning (heading, button, link, textbox)
3. **Interactive elements** — forms, buttons, links with their current state
4. **Text content** — visible text (truncated for large pages)
5. **Key metrics** — viewport size, document dimensions, scroll position

This is NOT a raw HTML dump. It's a structured, pruned representation optimized
for LLM consumption — similar to how Chrome MCP's `read_page` returns an
accessibility tree, not innerHTML.

## Design considerations

- Output should be token-efficient (LLMs pay per token)
- Truncate deep/wide subtrees with "[... N more children]"
- Include element counts so the agent knows what it's missing
- Consider a `--format compact` for even more concise output
