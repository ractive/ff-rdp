---
title: "screenshot: --output-root without --output rejects its own default path"
type: bug
status: resolved
priority: medium
discovered: 2026-10-22
reporter: comparis/lovable-snap-car
ff_rdp_version: ff-rdp 0.4.2 (fe5eb7256985 2026-10-18)
tags: [screenshot, cli]
resolved_by: 347
---

`--output-root DIR` alone (no `-o`) should put the auto-named file inside DIR — that's the obvious
reading of "restrict output to this directory", and the natural way to say "drop screenshots
here". Instead the default name is resolved against the current working directory and then
rejected as escaping the root, so the flag only works when `-o` is also given.

## Repro

```sh
cd ~/some/project
ff-rdp screenshot --full-page --output-root /tmp/shots
# {"error":"screenshot: output path escapes --output-root: output path
#  '/Users/…/some/project/screenshot-1792698674338.png' escapes --output-root '/tmp/shots'",
#  "error_type":"User"}
```

## Expected

With `--output-root` and no `--output`, write `DIR/screenshot-<ts>.png` (the same default name,
joined to the root). Keep rejecting an explicit `-o` that resolves outside the root.
