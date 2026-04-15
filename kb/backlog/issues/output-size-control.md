---
title: "Output size control: --limit, --sort, --fields, --summary for LLM-friendly responses"
type: feature
status: resolved
priority: high
discovered: 2026-04-07
tags:
  - output
  - ux
  - ai-agent
  - architecture
---

# Output size control: --limit, --sort, --fields, --summary for LLM-friendly responses

List-returning commands (`network`, `perf --type resource`, `dom`, `console`) can
return hundreds of entries, blowing up an LLM agent's context window. Commands should
return concise, meaningful output by default.

## Design principles

1. **Summary by default, detail on opt-in** — list commands return an aggregated
   summary. `--detail` returns individual entries.
2. **Sensible default limits** — `--limit N` on every list command, with a
   meaningful default (not unlimited). `--all` to override.
3. **Meaningful default sort** — `--sort <field>` with `--asc`/`--desc`. Default
   sort is the most useful per command (slowest first for network/resources,
   most recent for console, document order for DOM).
4. **Field selection** — `--fields url,status,duration_ms` to return only needed
   columns, reducing per-entry size.
5. **Tree depth + char budget** — for tree-shaped output (snapshot, a11y, dom tree):
   `--depth N` and `--max-chars N` with truncation markers like
   `"[... 42 more children]"`.
6. **Never dump raw data in errors** — jq and other errors show input shape
   (field names + types), not the full payload.

## Per-command defaults

| Command | Default mode | Default sort | Default limit |
|---------|-------------|--------------|---------------|
| `network` | summary (by type + top N slowest) | duration desc | 20 |
| `perf --type resource` | summary (by type/domain + top N) | duration desc | 20 |
| `dom` | count + first N matches | document order | 20 |
| `console` | recent messages | time desc | 50 |
| `snapshot` | depth-limited tree | document order | depth 5 |
| `a11y` | depth-limited tree | document order | depth 5 |

## Examples

```sh
ff-rdp network                                # summary: counts by type, top 20 slowest
ff-rdp network --detail                       # individual entries, default limit 20
ff-rdp network --detail --limit 10            # top 10 slowest
ff-rdp network --detail --sort size --desc    # top 20 largest
ff-rdp network --detail --all                 # every entry, no limit
ff-rdp network --detail --fields url,status,duration_ms  # only these fields

ff-rdp perf --type resource                   # summary: by type/domain, top 20 slowest
ff-rdp perf --type resource --detail --sort transfer_size --desc --limit 5

ff-rdp snapshot --depth 3 --max-chars 15000   # pruned tree within char budget
ff-rdp a11y --interactive --depth 4           # only interactive elements, 4 levels
```
