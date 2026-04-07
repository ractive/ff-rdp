---
title: "perf --type resource --jq applies filter to array, not envelope"
type: bug
status: open
priority: medium
discovered: 2026-04-07
tags: [perf, jq, dogfooding]
---

# perf --type resource --jq applies filter to array, not envelope

`perf --type resource --jq '.total'` fails because the jq filter is applied to the
`results` array directly, not the full `{meta, results, total}` envelope.

This is inconsistent with `perf --type navigation` and `perf vitals`, which apply jq
to the full envelope.

## Repro

```sh
ff-rdp perf --type resource --jq '.total'
# Error: cannot index array with string "total"

ff-rdp perf --type navigation --jq '.total'
# Works: 1
```

## Expected

All `perf` subcommands should apply `--jq` to the same envelope shape.
