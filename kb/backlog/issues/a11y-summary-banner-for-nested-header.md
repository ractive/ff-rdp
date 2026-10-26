---
title: "a11y summary: every <header> is reported as a banner landmark"
type: bug
status: resolved
priority: medium
discovered: 2026-10-22
reporter: comparis/lovable-snap-car
ff_rdp_version: ff-rdp 0.4.2 (fe5eb7256985 2026-10-18)
tags: [a11y]
resolved_by: 347
---

`a11y summary` lists one `banner` landmark per `<header>` element, including headers nested in
`nav`, `section` and `article`. Per HTML-AAM a `<header>` only maps to `banner` when it is scoped
to `<body>`; inside sectioning content or a landmark it has no landmark role (Firefox's own
accessibility tree agrees). On a real page this inflates the landmark list: the comparis
PageShell (mega menu + cards) produced **42** `banner` entries where the page has exactly one.

## Repro

Serve this file from any static server and open it:

```html
<!doctype html><html lang="en"><head><meta charset="utf-8"><title>Document title</title></head><body>
<header>Site header</header>
<nav><header>Menu header</header><a href="#a">A</a></nav>
<main><h1>Repro</h1>
  <section><header><h2>Section header</h2></header></section>
  <article><header><h2>Article header</h2></header><p>Text</p></article>
</main></body></html>
```

```sh
ff-rdp navigate http://127.0.0.1:<port>/repro.html
ff-rdp a11y summary | jq '[.results.landmarks[] | select(.role=="banner")] | length'
# 4
```

DOM check on the same page: 4 `<header>` elements, 1 of them not inside
`article,aside,main,nav,section` (and no explicit `role="banner"`).

## Expected

`1` — only the body-scoped `<header>` is a `banner`. Nested headers are either omitted or reported
without a landmark role (`generic`/`sectionhead`, whatever Firefox's AccessibilityActor says).

## Proposed fix

If the summary is built by the JS fallback (`meta.source: "js-fallback"`), apply the HTML-AAM
scoping rule: `header`/`footer` map to `banner`/`contentinfo` only when
`!el.parentElement.closest("article,aside,main,nav,section,[role=article],[role=complementary],[role=main],[role=navigation],[role=region]")`.
Better: take the role from the AccessibilityActor when available.
