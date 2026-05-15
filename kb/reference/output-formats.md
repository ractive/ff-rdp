---
title: "Output Formats Reference"
type: reference
date: 2026-05-15
tags:
  - output
  - json
  - text
  - html
  - jq
  - iter-60
---

# ff-rdp Output Formats

Introduced/revised in [[iteration-60-compact-responses-refs]].

## Default envelope shape (iter-60+)

All commands return:

```json
{"results": ..., "total": N}
```

When `meta` contains meaningful per-request fields (selector, depth, settle_method, etc.) it is included:

```json
{"results": ..., "total": N, "meta": {"selector": "button.submit", "settle_method": "idle"}}
```

**`meta` is omitted when empty** — no more boilerplate `"meta": {}` in the default output.

## --verbose flag

Adds `meta.connection` back to every response:

```json
{
  "results": ...,
  "total": N,
  "meta": {
    "selector": "...",
    "connection": {
      "host": "localhost",
      "port": 6000,
      "firefox_version": 139,
      "connected_pid": 12345,
      "connected_process": "firefox",
      "uptime_s": 42
    }
  }
}
```

Use `--verbose` for debugging, diagnostics, or when you need to confirm which Firefox instance was contacted.

## --format json (default)

Machine-readable JSON. This is the stable API contract. All downstream tooling (jq, scripts, agents) should consume this form.

## --format text

Human-readable tables and trees. Suitable for terminal use. Not a stable format — rendered output may change between versions.

```
url                           method  status  duration_ms
------------------------------------------------------------
https://example.com/app.js    GET     200     142
https://example.com/style.css GET     200     38
```

## --format html

Raw HTML passthrough. Available only for `dom`. Restores the pre-iter-60 shape where `results` contains raw HTML strings. (For `snapshot`, `--format html` is currently a no-op — see [[page-snapshot-format]].)

Use this when you need to:
- Diff raw HTML across page versions
- Feed HTML to an HTML parser
- Verify exact markup

Example:
```sh
ff-rdp --format html dom "h1"
# results: "<h1>Example Domain</h1>"
```

**This is the legacy escape hatch.** The default ARIA-tree JSON (see below) is better for most agent and inspection use cases.

## dom default: ARIA-tree JSON

Since iter-60, `dom <selector>` returns an ARIA-tree node per element:

```json
{
  "ref": "e1",
  "role": "heading",
  "name": "Welcome back",
  "level": 1,
  "tag": "h1",
  "state": {"expanded": false},
  "attrs": {
    "id": "page-title",
    "aria-label": "Welcome back"
  }
}
```

Fields:
- `ref` — stable ref ID (per-process; per-tab in daemon mode future)
- `role` — ARIA semantic role (from explicit `role=""` attribute or tag semantics)
- `name` — accessible name (aria-label, alt text, or trimmed text content)
- `level` — heading level (h1=1…h6=6; null for non-headings)
- `tag` — lowercase HTML tag name
- `state` — ARIA boolean states present on the element (expanded, disabled, selected, checked)
- `attrs` — actionable attributes only: id, name, type, href, aria-*, data-state, role, placeholder, value

## --jq filter

Applies a jq expression to the JSON envelope. Accesses any field: `.results`, `.total`, `.meta`.

```sh
ff-rdp tabs --jq '.results[0].url'
ff-rdp network --jq '[.results[] | select(.status >= 400)]'
ff-rdp perf vitals --jq '.results.lcp_ms'
```

`--jq` always suppresses hints (clean pipeline output).

## --jq + --format text combination (iter-60 D2)

Since iter-60, `--jq` and `--format text` can be combined. jq runs first on the JSON form, then text rendering applies to each output value.

```sh
ff-rdp network --detail --jq '.results[:5]' --format text
```

This is the "filter, then make terse" workflow: extract a subset with jq, display it as a human-readable table.

## page-snapshot-format

See [[page-snapshot-format]] for the `snapshot` command's ARIA-ish indented tree format.
