---
title: "a11y summary: headings are grouped by level instead of document order"
type: bug
status: resolved
priority: medium
discovered: 2026-10-22
reporter: comparis/lovable-snap-car
ff_rdp_version: ff-rdp 0.4.2 (fe5eb7256985 2026-10-18)
tags: [a11y, navigate]
resolved_by: 347
---

`a11y summary` (and the identical `results.page.headings` of `--with-page`) returns all `h1`,
then all `h2`, then all `h3` — the page outline is lost. An agent checking an article's heading
structure ("does every section start with an h2 followed by its h3s?") cannot tell which `h3`
belongs to which `h2`, and a skipped level inside a section is invisible.

## Repro

```html
<!doctype html><html lang="en"><head><meta charset="utf-8"><title>Outline</title></head><body><main>
<h1>Title</h1><h2>A</h2><h3>A.1</h3><h3>A.2</h3><h2>B</h2><h3>B.1</h3>
</main></body></html>
```

```sh
ff-rdp navigate http://127.0.0.1:<port>/outline.html
ff-rdp a11y summary | jq -c '[.results.headings[].text]'
# ["Title","A","B","A.1","A.2","B.1"]
```

## Expected

Document order: `["Title","A","A.1","A.2","B","B.1"]` — one `querySelectorAll("h1,h2,h3,h4,h5,h6,[role=heading]")`
pass instead of one pass per level. `--sort level` stays available for the grouped view.
