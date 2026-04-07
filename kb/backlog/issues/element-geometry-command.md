---
title: "Element geometry: bounding rects, overlap detection, positions"
type: feature
status: open
priority: high
discovered: 2026-04-07
tags: [dom, layout, geometry, ai-agent]
---

# Element geometry: bounding rects, overlap detection, positions

AI agents debugging layout issues need exact element positions and dimensions.
Currently the only option is taking a screenshot and visually estimating — slow,
imprecise, and expensive (vision model inference).

## Proposed command

```sh
ff-rdp geometry ".sidebar"                   # bounding rect for one element
ff-rdp geometry ".sidebar" ".main-content"   # multiple elements + overlap check
ff-rdp geometry ".card" --all                # all matching elements
```

## Output

```json
{
  "elements": [
    {
      "selector": ".sidebar",
      "tag": "aside",
      "rect": {"x": 0, "y": 80, "width": 250, "height": 920},
      "visible": true,
      "z_index": 1,
      "overflow": "hidden",
      "position": "fixed"
    },
    {
      "selector": ".main-content",
      "tag": "main",
      "rect": {"x": 248, "y": 80, "width": 1172, "height": 2400},
      "visible": true,
      "z_index": 0,
      "overflow": "visible",
      "position": "relative"
    }
  ],
  "overlaps": [
    {
      "a": ".sidebar",
      "b": ".main-content",
      "overlap_rect": {"x": 248, "y": 80, "width": 2, "height": 920},
      "overlap_px": 1840
    }
  ]
}
```

## Implementation

Can be done via eval using `getBoundingClientRect()`, `getComputedStyle()` for
z-index/position/overflow. No native actor needed — this is a good eval-based command.
