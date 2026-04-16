---
title: "Backlog: screenshot --annotate"
type: feature
status: backlog
date: 2026-04-16
tags: [backlog, screenshot, dx, visual-debugging]
---

# screenshot --annotate

Add a `--annotate <SELECTOR>` flag to the `screenshot` command that overlays
bounding-box highlights and label text onto the captured PNG.

Surfaced during [[dogfooding/dogfooding-session-nova-template-jsonforms-index]]:
the session took a screenshot to confirm form layout, then had to mentally map
JSON geometry output back onto the image. Annotated screenshots would make it
immediately obvious which elements are in frame and whether they overlap.

## Implementation sketch

- After capturing the PNG, evaluate `getBoundingClientRect` for each selector
  match to get pixel coordinates.
- Draw coloured rectangles and label strings onto the PNG bytes using a pure-Rust
  image library (e.g. `image` + `imageproc`).
- One annotation layer per `--annotate` invocation; multiple flags allowed.
- Return the annotated PNG via the existing `--output` / `--base64` paths.

This requires a new optional dependency (`image`, `imageproc`) so it should land
behind a Cargo feature flag (`screenshot-annotate`) to keep the default binary slim.

Out of scope for [[iterations/iteration-43-dx-fixes]].
