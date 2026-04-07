---
title: "screenshot: return inline base64 data for AI agent consumption"
type: feature
status: open
priority: high
discovered: 2026-04-07
tags: [screenshot, ai-agent, ux]
---

# screenshot: return inline base64 data for AI agent consumption

Currently `ff-rdp screenshot` saves to a file path. AI agents need the image data
inline (base64) so they can "see" the page through their vision capabilities without
a separate file read step.

Chrome MCP's `computer screenshot` returns the image directly, which is why it's
the primary way agents do visual verification.

## Proposed change

```sh
ff-rdp screenshot                        # current: saves to file
ff-rdp screenshot --base64               # new: returns base64 in JSON output
ff-rdp screenshot --data-uri             # new: returns data:image/png;base64,...
```

## Output

```json
{
  "meta": {"host": "localhost", "port": 6000},
  "results": {
    "format": "png",
    "width": 1920,
    "height": 1080,
    "data": "iVBORw0KGgoAAAANSUhEUg..."
  }
}
```

## MCP server integration

When ff-rdp runs as an MCP server, screenshot should return the image as an MCP
image content type, which MCP clients can render inline.
