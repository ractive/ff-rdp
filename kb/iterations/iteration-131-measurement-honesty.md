---
branch: iter-131/measurement-honesty
date: 2026-07-19
depends_on: []
dogfood_path: |
  ff-rdp launch --headless --auto-consent
  ff-rdp navigate https://www.theguardian.com >/dev/null
  ff-rdp perf summary --jq '{total: .results.total_transfer_size, opaque: .results.transfer_size_opaque}'
  # → opaque:true when per-resource sizes are 0 (cross-origin, no Timing-Allow-Origin)
  ff-rdp responsive main --widths 320 --jq '.results.breakpoints[0].media_queries_applied'
  # → false, promoted to a per-breakpoint field (not only a buried warning)
  ff-rdp throttle status --jq '.results.profile'
  # → "slow-3g" after `throttle slow-3g`
first_call_sites: []
status: planned
---

# Iteration 131: measurement honesty — perf transfer sizes, responsive simulation, snapshot bounds, throttle state

The tool's numbers must not look authoritative when the platform can't back them.
Bundles four honesty findings from [[dogfooding-session-61]]/[[dogfooding-session-62]].

## Findings driving this iteration

1. **`perf summary` aggregates zeros into a fake total** (dogfood-62 #3, MODERATE): 93
   resources, every `transfer_size:0` (cross-origin without Timing-Allow-Origin), yet
   `total_transfer_size:6386` is presented as page weight. "6.4 KB total" on a Guardian
   page is obviously wrong, and nothing marks it as opaque.
2. **`responsive` reports width-matching rects while media queries never fire**
   (dogfood-62 #5, MODERATE): `rect.width` equals the requested 320/768/1024 while
   `inner_width` stays 1366 and `matchMedia` never matches — layout looks responsive
   while mobile nav/hidden/reflowed elements are never exercised. The honest signal
   (iter-98's warning) exists but is easy to miss next to authoritative-looking rects.
   Platform constraint: no RDP viewport-sizing actor (see memory/decision log) — this
   iteration is about presentation, not simulation.
3. **`snapshot --max-chars` is a near-no-op** (s61 #9, MINOR): 100 vs 5000 vs default
   → 1741/1742/1743 bytes; the flag bounds only leaf text, not the serialized tree.
4. **`throttle` state is write-only** (dogfood-62 #8, LOW): set succeeds but nothing
   reports the active profile, so an agent cannot verify or even recall what throttling
   is in effect; the observed ~50 ms reload under slow-3g (cache? measurement?) was
   unverifiable. (The flaky `live_109_throttle` 2× timing assertion is the test-side
   face of the same verifiability gap.)

## Themes

- **A — opaque transfer sizes are labelled, not summed silently.** Per-resource
  `transfer_size: null` (not 0) when resource timing is opaque; the aggregate carries
  `transfer_size_opaque: true` + an excluded-count note when any/most resources are
  opaque. Text renderer says "n/a (cross-origin)" instead of 0.
- **B — responsive output leads with what was actually simulated.** Per-breakpoint
  `media_queries_applied: bool` promoted next to `rect`, and a `simulation:
  "css-width-constraint"` field naming the technique, so the rect can't be mistaken
  for a real viewport resize.
- **C — `snapshot --max-chars` bounds the whole output.** Truncate the serialized
  tree (breadth-first, deepest-first pruning or equivalent) with a `truncated: true`
  marker; leaf-text bounding stays as-is beneath it.
- **D — throttle state is readable.** `throttle status` reports the active profile
  (or none); `throttle <profile>`'s envelope echoes what was applied. Document the
  cache caveat (throttling does not bypass the HTTP cache) in help; if the
  target-configuration actor's cache-disable is cheap to expose alongside (core
  support exists — `live_cache_disable_via_target_config`), add
  `throttle <profile> --disable-cache`.

## Tasks

- [ ] A: opaque-detection in the resource-timing mapper; null per-resource sizes,
      aggregate flag + note; text renderer alignment.
- [ ] B: promote `media_queries_applied` + `simulation` per breakpoint in
      `responsive`; keep the iter-98 warning strings.
- [ ] C: whole-output bounding for `snapshot --max-chars` + `truncated` marker.
- [ ] D: `throttle status`, applied-profile echo, cache caveat in help; optional
      `--disable-cache` if the core plumbing is a small lift (else defer with a filed
      plan, per discipline).
- [ ] Help/cookbook updates for all four surfaces.

## Acceptance Criteria [0/5]

<!-- Each AC names a live test + asserted post-condition, per CLAUDE.md convention. -->

- [ ] live_131_perf_opaque_transfer (network-gated): on a page whose cross-origin
      resources all report 0 transfer size, `perf summary` yields per-resource
      `transfer_size: null` and top-level `transfer_size_opaque: true`.
- [ ] live_131_perf_transparent_transfer: on a same-origin fixture (sizes real),
      `transfer_size_opaque` is false and the aggregate equals the sum of per-resource
      sizes.
- [ ] live_131_responsive_simulation_fields: `responsive body --widths 320` reports
      `media_queries_applied:false` and `simulation:"css-width-constraint"` per
      breakpoint on a desktop-viewport Firefox.
- [ ] live_131_snapshot_max_chars_bounds: `snapshot --max-chars 500` output payload is
      ≤ 500 chars + envelope overhead and carries `truncated:true` on a page whose
      full snapshot exceeds it.
- [ ] live_131_throttle_status: after `throttle slow-3g`, `throttle status` reports
      `profile:"slow-3g"`; after `throttle off` it reports none.

## Notes

Sibling plans from the same findings batch: [[iteration-128-network-hint-always-present]],
[[iteration-129-consent-and-cross-origin-frames]], [[iteration-130-navigation-truthfulness]],
[[iteration-132-cli-polish]].
[[iteration-133-viewport-emulation]] builds on this iteration's responsive
`simulation`/`media_queries_applied` fields — land this one first.
