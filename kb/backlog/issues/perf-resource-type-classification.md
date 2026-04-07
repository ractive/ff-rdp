---
title: "perf resource: add resource_type field derived from content-type"
type: feature
status: open
priority: medium
discovered: 2026-04-07
tags: [perf, resource, ux, dogfooding]
---

# perf resource: add resource_type field derived from content-type

`perf --type resource` only exposes `initiator_type` (script, fetch, img, link, etc.)
which reflects *what initiated* the request, not *what the resource is*. A `resource_type`
field derived from the URL extension or content-type (js, css, image, font, document, xhr)
would make grouping and analysis much more natural.

## Current

```sh
ff-rdp perf --type resource | jq '[.results[] | .initiator_type] | group_by(.) | ...'
# Groups by initiator, not by actual resource type
# A font loaded by a <link> shows as "link", not "font"
```

## Desired

Each resource entry should include:
```json
{
  "initiator_type": "link",
  "resource_type": "font",
  "url": "https://cdn.example.com/roboto.woff2"
}
```

Classification can be done by URL extension (`.js` → script, `.woff2` → font, `.css` → stylesheet,
`.png/.jpg/.webp/.svg` → image) with content-type as fallback.
