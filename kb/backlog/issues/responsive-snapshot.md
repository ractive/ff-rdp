---
title: "Responsive testing: collect layout data at multiple viewport widths"
type: feature
status: open
priority: medium
discovered: 2026-04-07
tags: [responsive, layout, viewport, ai-agent]
---

# Responsive testing: collect layout data at multiple viewport widths

During frontend development, AI agents need to verify layouts work across breakpoints.
Currently this requires manually resizing, taking screenshots, and visually checking —
multiple round-trips and vision inference per breakpoint.

## Proposed command

```sh
ff-rdp responsive ".hero,.nav,.sidebar" --widths 320,768,1024,1440
```

## Output

```json
{
  "breakpoints": [
    {
      "width": 320,
      "elements": {
        ".hero": {"rect": {"x": 0, "y": 0, "width": 320, "height": 200}, "display": "block"},
        ".nav": {"rect": {"x": 0, "y": 0, "width": 320, "height": 60}, "display": "none"},
        ".sidebar": {"rect": {"x": 0, "y": 400, "width": 320, "height": 300}, "display": "block"}
      }
    },
    {
      "width": 1440,
      "elements": {
        ".hero": {"rect": {"x": 250, "y": 0, "width": 1190, "height": 400}, "display": "flex"},
        ".nav": {"rect": {"x": 0, "y": 0, "width": 250, "height": 1080}, "display": "block"},
        ".sidebar": {"rect": {"x": 1190, "y": 0, "width": 250, "height": 800}, "display": "block"}
      }
    }
  ]
}
```

## Implementation

Resize viewport via `resizeTo()` or eval, collect `getBoundingClientRect()` +
`getComputedStyle()` at each width, restore original viewport.
