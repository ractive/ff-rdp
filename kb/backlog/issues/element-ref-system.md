---
title: "Element reference system for cross-command element reuse"
type: feature
status: open
priority: low
discovered: 2026-04-07
tags: [dom, interaction, ai-agent, daemon, ux]
---

# Element reference system for cross-command element reuse

Chrome MCP's `read_page` assigns ref IDs (ref_1, ref_2, ...) to elements. These refs
can be reused in `form_input`, `click`, and `scroll_to` — no need to re-query selectors
or risk stale DOM references.

## Current problem

In ff-rdp, every command re-queries the DOM:
```sh
ff-rdp dom "input.search"    # finds element
ff-rdp type "input.search" "hello"  # re-queries, might find different element
ff-rdp click "button.submit"  # another query
```

This is fragile when pages are dynamic (React re-renders, DOM mutations).

## Proposed design

```sh
ff-rdp snapshot --interactive
# Returns: {"elements": [{"ref": "r1", "role": "textbox", "name": "Search"}, ...]}

ff-rdp type --ref r1 "hello"
ff-rdp click --ref r2
ff-rdp styles --ref r1
```

Refs are valid for the current page — invalidated on navigation.

## Implementation — daemon-side state

The daemon holds the persistent connection, so refs are managed daemon-side:

- `snapshot` assigns refs, daemon stores the ref→element mapping via a JS-side
  `WeakRef` registry (`window.__ff_rdp_refs`) on the page
- `click --ref r1` resolves through the daemon, which evals the ref lookup
- The daemon can subscribe to DOM mutation events via WatcherActor to detect
  and invalidate stale refs automatically
- SPA navigation or page reload clears all refs
- If a ref's target has been GC'd or mutated away, return a clear error:
  `"ref r1 is no longer valid (element removed from DOM)"`

This is more robust than Chrome MCP's approach, which doesn't do mutation-based
invalidation.

## Dependencies

- Daemon mode (existing, iteration 25 improves reliability)
- `snapshot` command (iteration 21)
- DOM mutation watching via WatcherActor (iteration 27, optional enhancement)
