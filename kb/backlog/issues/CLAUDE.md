# ff-rdp issue intake

One file per issue, `<command>-<short-slug>.md`. Agents from other projects write here; they never
edit `kb/backlog.md`.

## Reporting

```yaml
---
title: "<command>: <what goes wrong>"
type: bug | feature
status: open
priority: low | medium | high
discovered: YYYY-MM-DD
reporter: <project>
ff_rdp_version: <`ff-rdp --version` output>
tags: [<command>]
---
```

Sections: **Repro** (exact commands and the JSON output or error envelope), **Expected**,
**Proposed fix** (optional). The repro must run for someone else: a public URL or a
self-contained HTML snippet, never `localhost`.

## Triage (ff-rdp sessions)

`hyalo find --property status=open --glob 'backlog/issues/*.md' --format text` lists the queue.
Reproduce it, note the root cause if it is quick to find, then add a `kb/backlog.md` line linking
`[[<file>]]` — or set `status: resolved` with a reason. Land it through a docs PR.
The fix commit sets `status: resolved` and `resolved_by: <PR>` and ticks the backlog line.
