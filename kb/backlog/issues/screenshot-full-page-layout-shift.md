---
title: "screenshot --full-page: layout differs from the live viewport (sticky header over content, nav shifted)"
type: bug
status: resolved
priority: medium
discovered: 2026-10-22
reporter: comparis/lovable-snap-car
ff_rdp_version: ff-rdp 0.4.2 (fe5eb7256985 2026-10-18)
tags: [screenshot, layout]
resolved_by: 347
---

On a page with a sticky site header (comparis NEON shell, TanStack Start app on localhost),
`--full-page` captures a different layout than the viewport capture of the same page in the same
tab, seconds apart:

| | viewport `screenshot` | `screenshot --full-page` |
|---|---|---|
| hero band top | y = 72 (below the 72px header) | y ≈ 0 — the header is drawn over the hero's top |
| first nav item x | 332 | 291 |
| Login button x | 1304–1424 | 1223–1343 (≈ 80px left) |

The full-page image is therefore misleading for layout review: it shows an overlap that a visitor
never sees, and a narrower header. Likely the capture re-lays out the document (viewport resized
to the full height, or scrollbar/scroll position change) and the sticky header / a measured
spacer settle differently.

## Repro

```sh
ff-rdp launch --headless --debug-port 6011 --window-size 1440x1000 --url <page with sticky header>
FF_RDP_PORT=6011 ff-rdp screenshot -o /tmp/vp.png
FF_RDP_PORT=6011 ff-rdp screenshot --full-page -o /tmp/full.png
# compare the top 120px of both images
```

## Expected

The first viewport-height of a `--full-page` capture matches the viewport capture (same element
positions). If a re-layout is unavoidable, say so in the envelope (e.g. a `warnings[]` entry with
the viewport size used) so the reader doesn't trust the overlap.
