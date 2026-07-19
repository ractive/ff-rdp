---
branch: iter-133/viewport-emulation
date: 2026-07-19
depends_on:
  - kb/iterations/iteration-131-measurement-honesty.md
dogfood_path: |
  ff-rdp launch --headless --window-size 1200x900
  ff-rdp eval 'innerWidth + "x" + innerHeight'   # → "1200x900"
  ff-rdp emulate --viewport 390x844 --dppx 3 --touch on
  ff-rdp eval 'matchMedia("(max-width: 400px)").matches'   # → true (real media-query flip)
  ff-rdp screenshot -o /tmp/mobile.png   # → PNG width 390 × DPR
first_call_sites: []
status: planned
---

# Iteration 133: viewport emulation — mobile screenshots, launch window-size, emulate --viewport

Driven by agent-seat feedback (2026-07-19): an agent trying to take mobile-mode
screenshots searched four natural homes for viewport control and found none. Each stop
in the trail is a finding in itself:

1. `launch --window-size WxH` — Firefox honors `-width`/`-height` CLI args headless, so
   launch only needs to forward them. Cheap 90% solution; the ~500 px window floor
   belongs in that flag's help text.
2. `screenshot --viewport-width` — `--viewport-height` exists, so the missing width
   reads as an oversight, not a design choice. Discoverability trap: it's the first
   thing an agent tries.
3. `emulate --viewport 390x844` — the actually-wanted feature. `emulate` already
   patches the target-configuration actor (touch, dppx, UA); the feedback claims
   Firefox RDM does true viewport emulation through the same family — below the window
   floor, with real media-query evaluation. **Unverified** — the kb/decision-log
   position to date has been "no RDP viewport-sizing actor" (hence `responsive`'s CSS
   width-constraint technique). Theme A settles this against the Firefox source.
4. `responsive` is honest about the limitation but, on utility-CSS apps (Tailwind
   `tablet:`/`desktop:` classes), the media-query flip IS the layout — geometry at a
   CSS-constrained width answers the wrong half of the question. With an emulated
   viewport, `matches:true` becomes the normal case.

Also recorded: headless `window.resizeTo()` is silently ignored — cost the agent a
probe; worth a doc note where agents will look (eval/screenshot help).

## Themes

- **A — research: how RDM sizes the viewport, and whether ff-rdp can reach it.**
  Read the Firefox source (RDM + target-configuration actor + browsing-context
  actor family) and determine the mechanism for true viewport emulation over RDP:
  which actor/fields, window-floor behavior, dppx interaction, media-query
  re-evaluation. Outcome → `kb/research/viewport-emulation.md` with `firefox_refs`;
  update the decision log (supersede the "no viewport actor" position if disproven).
  Any drift from the published spec dict gets `// allow-spec-drift` per convention.
- **B — `launch --window-size WxH`.** Forward `-width`/`-height`; help text documents
  the ~500 px floor and that this is a window size, not device emulation.
- **C — `emulate --viewport WxH`** (mechanism per Theme A). Composes with the existing
  `--dppx`/`--touch`/UA fields into honest device profiles; `emulate` envelope reports
  the applied viewport and whether it is true emulation or a window resize.
  `--device <preset>` sugar (e.g. `iphone-15` bundling viewport+dppx+touch+UA) only if
  it falls out cheaply; otherwise note it as follow-up.
- **D — screenshot + responsive ride on the viewport.** `screenshot --viewport-width`
  for symmetry with the existing height flag; `responsive` uses the emulated viewport
  when available so `media_queries_applied:true` is the normal case (its
  iter-131 `simulation` field then reports the technique actually used), and its
  warning points at `emulate --viewport` when emulation is unavailable.
- **E — dead-end doc notes.** eval/screenshot help mention that headless
  `window.resizeTo()` is silently ignored and point at the viewport features.

## Tasks

- [ ] A: research spike + `kb/research/viewport-emulation.md` + decision-log update.
- [ ] B: `launch --window-size` forwarding + floor documentation + validation
      (reject e.g. `0x0`, non-`WxH` input).
- [ ] C: `emulate --viewport` via the Theme A mechanism; envelope reports
      `viewport: {width, height, emulated: bool}`.
- [ ] D: `screenshot --viewport-width`; `responsive` integration + warning update.
- [ ] E: help-text notes (resizeTo, window floor, emulate pointer).

## Acceptance Criteria [0/5]

<!-- Each AC names a live test + asserted post-condition, per CLAUDE.md convention. -->

- [ ] live_133_launch_window_size: `launch --headless --window-size 1200x900` →
      `eval innerWidth/innerHeight` reports 1200×900 (above the window floor).
- [ ] live_133_emulate_viewport_media_query: after `emulate --viewport 390x844`,
      `matchMedia("(max-width: 400px)").matches` is true and `innerWidth` is 390 —
      i.e. below the window floor, proving true emulation (Theme A mechanism).
- [ ] live_133_screenshot_mobile_width: with the 390-wide viewport active,
      `screenshot` produces a PNG of width 390 × DPR.
- [ ] live_133_responsive_matches_true: `responsive body --widths 390` with viewport
      emulation available reports `media_queries_applied:true` and
      `simulation:"viewport-emulation"`.
- [ ] e2e_help_resizeto_note: eval/screenshot `--help` mention the headless
      `resizeTo()` no-op and point at `--window-size` / `emulate --viewport`.

## Notes

**Scope hinge:** if Theme A concludes true sub-floor viewport emulation is NOT
reachable over current RDP, this iteration lands B + the screenshot width flag + E
with honest `emulated:false` reporting, and the C/D emulation ACs are annotated
`[deferred — new plan: …]` with the follow-up filed before merge, per discipline.
Design-heavy (new protocol surface) — opus implement model, like
[[iteration-129-consent-and-cross-origin-frames]].
Depends on [[iteration-131-measurement-honesty]] because both touch `responsive`'s
simulation/honesty fields — serialize to avoid conflicts.
