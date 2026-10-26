---
title: "screenshot: no full-page capture at a true mobile width; the conflict error is plain text"
type: feature
status: resolved
priority: medium
discovered: 2026-10-22
reporter: comparis/lovable-snap-car
ff_rdp_version: ff-rdp 0.4.2 (fe5eb7256985 2026-10-18)
tags: [screenshot]
resolved_by: 347
---

Checking a long article on a phone means looking at the whole page at 390px. `--window-size` is the
only true sub-500px path, but it cannot be combined with `--full-page`, so the capture stops at
the first screen; `launch --window-size 390x…` is clamped to the ~500px live-viewport floor, so
`--full-page` there is not a real mobile layout either. Result: no way to see the bottom of a page
(tables, card rows, FAQ) at phone width.

Second, smaller problem: the rejection is clap's plain-text usage error on stderr, exit 2 — not the
JSON envelope every other failure uses. A script that pipes ff-rdp into `jq` gets a parse error
instead of `{"error": …}`.

## Repro

```sh
ff-rdp navigate https://example.com
ff-rdp screenshot --window-size 390x844 --full-page --output /tmp/m.png
# error: the argument '--window-size <WxH>' cannot be used with '--full-page'
# (plain text, exit 2)
```

## Expected

1. A full-page capture at a true mobile width, e.g. `--window-size 390 --full-page` (width only).
2. Argument/usage errors in the JSON error envelope (`{"error": {"kind": "usage", "message": …}}`),
   or at least when stdout is not a TTY.

## Proposed fix

Firefox's own batch mode already does (1): `firefox --headless --screenshot --window-size=390`
with only a width captures the full document height (worth re-checking on the current Firefox).
Map `--window-size W --full-page` to that call. For (2), catch clap's `Error` in `main`, and when
JSON output is active, render it through the existing error envelope.

## Resolution (iter-296)

Part 1 is fixed: `--window-size WxH --full-page` passes only `--window-size=W` to the batch
capture, so the PNG is W wide and spans the whole document (390x3372 on a 3372px page). When
Firefox writes no PNG (a page too tall to allocate the surface), the error says so. Part 2 (clap
usage errors as a JSON envelope) is a `kb/backlog.md` line; it needs a decision-log entry.
