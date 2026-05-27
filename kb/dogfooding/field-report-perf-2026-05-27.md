---
title: "Field report: real user running `ff-rdp perf` (2026-05-27)"
type: field-report
date: 2026-05-27
status: open
source: external-user-session
commands_used: [launch, navigate, perf audit, daemon stop, --jq]
tags:
  - field-report
  - perf
  - daemon-lifecycle
  - lcp
  - render-blocking
  - jq
---

# Field report — `ff-rdp perf` session, 2026-05-27

Unprompted feedback from a user who ran a perf-investigation session
(image swap verification, wire inventory, CLS confirmation). Captured
verbatim to feed iter-86+ planning. Not collected via the formal
dogfooding skill — this is what fell out of real use.

## What worked (don't break)

- `launch` → `navigate` → `perf audit` is a tight, scriptable loop. JSON
  + `--jq` is "way better DX than scraping Lighthouse HTML reports".
- Resource breakdown (per-domain, per-type, slowest) was the actual
  signal — verified recompressions, caught a 72 KB `icon-512.png`,
  confirmed 8 requests / 571 KB total.
- `perf audit` bundling vitals + navigation + resources + dom-stats in
  one call (vs four separate ones) was specifically called out as
  "nice".
- Headless/non-headless from the same CLI with a temp profile that
  doesn't clobber a real Firefox install was "genuinely thoughtful".

## Friction / bugs (candidates for iter-86)

### 1. No LCP — most-asked perf metric is unavailable

Firefox doesn't implement the Chromium LCP `PerformanceObserver` entry.
ff-rdp falls back to "biggest visible image", which isn't the same
metric and the user noticed. Options:

- **Improve the fallback messaging**: be explicit that this is a
  Firefox limitation, not an ff-rdp gap. Suggest Lighthouse for the
  canonical LCP number.
- **Best-effort LCP via paint timing**: combine `largest-contentful-paint`
  (if/when Firefox ships it) with `paint-timing` `first-contentful-paint`
  + render-blocking analysis to estimate. Mark estimated values clearly.
- **Document the gap in `--help`**: a one-liner under `perf audit` so
  users don't get bitten.

### 2. `daemon stop` doesn't free port 6000 — bug

User had to `kill -9` the Firefox PID before relaunching non-headless.
`daemon stop` should:
- Terminate the Firefox process group, not just close the RDP socket
- Verify port 6000 is free before returning
- Or: add `launch --force` / `launch --replace` that handles a stuck
  prior instance transparently

### 3. `lcp_note` lies about headless state — bug

The note said "headless Firefox" even after the user relaunched
non-headless. They wasted a relaunch chasing a metric Firefox just
doesn't have. The note text is stale/hardcoded; it should reflect the
*current* launch's `headless` setting (read from the actor handshake
or the launch record), and ideally also state "this is a Firefox
limitation regardless of headless mode" so users stop chasing it.

### 4. "5 render-blocking resources" miscounts — bug

Includes `<link rel="icon">` entries, which don't render-block.
Filter should match the spec's render-blocking criteria:
`<link rel="stylesheet">` without `media` mismatch, `<script>`
without `async`/`defer`/`type=module`, etc. Favicons are not in
that list.

### 5. `--jq` inconsistent on missing paths — UX nit

User saw `{by_type:null,...}` when paths didn't exist. They wanted
either:
- **Loud error** ("path .foo.bar not found in input") with non-zero exit, OR
- **Silent omit** (drop the key entirely)

The current "emit `null` for missing" is the worst of both — looks
like a real value, breaks downstream `--jq` filters that test for
key presence.

## Verdict (user's words)

> For "did my image swap land, what's on the wire, is CLS still zero"
> — excellent, fast, scriptable. For "what's my LCP / final perf
> score" — go to Lighthouse. They're complementary, not substitutes.

## Suggested iter-86 themes (drafted, not committed)

1. `daemon stop` actually frees the port (bug #2) — kills process group,
   verifies port-free before return.
2. `lcp_note` reflects actual launch state + Firefox limitation
   (bug #3) — single-source-of-truth for headless flag.
3. Render-blocking-resources filter matches the spec (bug #4) —
   exclude favicons, async/defer scripts, media-mismatched stylesheets.
4. `--jq` missing-path policy: documented, consistent. Probably
   default to "silent omit" with `--jq-strict` opt-in for loud errors.
5. (Stretch) LCP best-effort via paint-timing + render-blocking
   analysis, clearly marked as estimated.

## References

- [[dogfooding-session-57]] — the formal session
- [[iteration-85-dogfood-57-carryovers-and-runnable-dogfood-path]] —
  in-flight; field report should feed iter-86 once 85 merges
