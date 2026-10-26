---
title: "navigate --with-page: page.title is the og:title, not document.title"
type: bug
status: resolved
priority: low
discovered: 2026-10-22
reporter: comparis/lovable-snap-car
ff_rdp_version: ff-rdp 0.4.2 (fe5eb7256985 2026-10-18)
tags: [navigate]
resolved_by: 347
---

With `--with-page`, `results.page.title` comes from Readability, which prefers `og:title` over
`<title>`. Agents read it as the document title and draw wrong conclusions — e.g. when checking
that a route set its `<title>` correctly, the field shows the site-wide `og:title` instead.

## Repro

```html
<!doctype html><html lang="en"><head><meta charset="utf-8"><title>Document title</title>
<meta property="og:title" content="OG title"></head><body><main><h1>Repro</h1></main></body></html>
```

```sh
ff-rdp navigate http://127.0.0.1:<port>/repro.html --with-page | jq -r .results.page.title
# OG title
ff-rdp eval document.title | jq -r .results
# Document title
```

## Expected

`page.title` is `document.title` (`"Document title"`). If the Readability title is useful, expose
it under its own key (e.g. `page.reader_title`), with `page.source` saying where it came from.
