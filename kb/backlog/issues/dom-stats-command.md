---
title: "Add DOM stats: node count, document size, render-blocking resources"
type: feature
status: resolved
priority: low
discovered: 2026-04-07
tags:
  - dom
  - dogfooding
  - performance
---

# Add DOM stats: node count, document size, render-blocking resources

During the dogfooding audit there was no way to get DOM complexity metrics without
writing custom eval scripts. A `ff-rdp dom stats` command (or inclusion in `perf audit`)
would provide:

- Total DOM node count (`document.querySelectorAll('*').length`)
- Document size (`document.documentElement.outerHTML.length`)
- Inline script count and total size
- Render-blocking resources (scripts without `async`/`defer` in `<head>`)
- Image count and how many lack `loading="lazy"`

These are standard Lighthouse-style checks useful for performance audits.
