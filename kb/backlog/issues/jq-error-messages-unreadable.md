---
title: "jq error messages dump internal Rust debug format"
type: bug
status: open
priority: high
discovered: 2026-04-07
tags: [jq, ux, dogfooding]
---

# jq error messages dump internal Rust debug format

When a `--jq` filter fails, the error message dumps the internal Rust representation
(`TStr(b"decoded_size"): Num(Int(0))`) instead of readable JSON or a clean error.

## Repro

```sh
ff-rdp perf --type resource --jq '.nonexistent_field | length'
# internal error: jq runtime error: Error(Str([Str("cannot index "), Val(Arr([Obj({TStr(b"decoded_size"): ...
```

## Expected

A short, readable error like:
```
jq error: cannot index array with string "nonexistent_field"
```

Or at minimum, show the JSON shape of the input so the user can fix their filter.
