---
branch: iter-133/mobile-viewports
date: 2026-07-19
depends_on:
  - kb/iterations/iteration-131-measurement-honesty.md
dogfood_path: |
  ff-rdp launch --headless --window-size 600x800
  ff-rdp eval 'innerWidth + "x" + innerHeight'   # → "600x800" (true value, above the floor)
  ff-rdp navigate https://example.com >/dev/null
  ff-rdp screenshot --window-size 390x844 -o /tmp/mobile.png
  # → PNG exactly 390 wide (batch capture path, no floor)
first_call_sites: []
status: planned
---

# Iteration 133: mobile viewports — launch window-size, batch mobile screenshots, dppx composition

Driven by agent-seat feedback (2026-07-19): an agent taking mobile-mode screenshots
found no viewport control at any of the four natural homes (`launch`, `screenshot`,
`emulate`, `responsive`). Design was settled up-front by the research spike
[[viewport-emulation]] (2026-07-20, Firefox 152.0.6, empirical + RDM-internals-traced):

- **True viewport emulation over RDP is impossible.** RDM sizes the viewport by
  resizing a chrome-owned frame element via CSS custom properties in the parent
  process; no RDP actor/method carries a size. `customViewport` sent to the
  target-configuration actor is silently stripped (echo `{}`; `innerWidth`
  unchanged). This confirms the standing kb position and refutes the feedback's
  target-configuration claim.
- **Headless `--screenshot` batch mode honors `--window-size` exactly** — PNG 390×844
  or even 320×568, no floor. This is the only true sub-500px mobile raster path.
- **A debugger-server headless instance clamps `-width` to a ~500px floor** (390 →
  innerWidth 500) and **ignores `--window-size` entirely** (stays 1366).
- **`overrideDPPX` works and is orthogonal to width** (DPR 3 with width unchanged) —
  density composes with any width mechanism.
- **ff-rdp-core ships a dead primitive**: `TargetConfigurationFront::
  set_custom_viewport_size` (`fronts/target_configuration.rs:193`) sends the
  server-ignored `customViewport` field; its only consumer is its own unit test.

## Themes

- **A — `launch --window-size WxH`** forwards `-width`/`-height` (NOT the
  headless-shell `--window-size` arg, which debugger-server instances ignore).
  True viewport for widths ≥ the ~500px floor: real `innerWidth`, real media
  queries, real layout. The launch envelope reports requested vs effective size and
  warns when the request is below the floor. Help documents the floor and that this
  is a window size.
- **B — true sub-500px mobile screenshots via batch capture.** `screenshot
  --window-size WxH` runs a one-shot `firefox --headless --screenshot
  --window-size=W,H <current-tab-url>` with a scratch profile (separate from the
  RDP session; proven exact PNG dimensions). Envelope reports
  `capture: "batch-window-size"` so the mode is self-evident. Document the
  limitation that batch capture re-navigates (fresh session: no cookies/state
  from the live tab). **Correction during implementation (2026-08-09):** the
  planned `--dppx` density knob for this path was dropped — direct testing
  against Firefox 153.0.3 showed `layout.css.devPixelsPerPx` has zero effect
  on the `--screenshot` raster (with or without e10s); the plan's dppx
  assumption only held for the unrelated live-RDP `emulate --dppx` mechanism.
  See the addendum in [[viewport-emulation]].
- **C — honest docs + pointers.** eval/screenshot help note headless
  `window.resizeTo()` is a silent no-op; `responsive`'s warning and help point at
  `launch --window-size` (≥500) and `screenshot --window-size` (below), and clearly
  distinguish its CSS-constraint technique (layout-only) from a real window size.
- **D — remove the dead primitive.** Delete `set_custom_viewport_size` (+ its
  mirror in the actor kb doc if listed) so nobody wires a UX flag to a field the
  server strips; cite the research doc in the removal commit.

## Tasks

- [x] A: `--window-size WxH` parsing + validation (reject `0x0`, non-`WxH`) in
      launch; forward `-width`/`-height` in the Firefox command builder; requested
      vs effective in the envelope + below-floor warning; help text.
- [x] B: batch capture path in screenshot (scratch profile, `--screenshot`,
      `--window-size=W,H`); current-tab URL resolution; `capture` field in the
      envelope; help text incl. fresh-session caveat. `--dppx` dropped — see
      the Themes section correction above.
- [x] C: resizeTo note in eval/screenshot help; responsive warning + help pointers.
- [x] D: remove `set_custom_viewport_size` and its unit test; update
      `kb/rdp/actors/` target-configuration doc (check-actor-kb-sync will require
      the kb edit anyway).

## Acceptance Criteria [5/5]

<!-- Each AC names a live test + asserted post-condition, per CLAUDE.md convention. -->

- [x] live_133_launch_window_size_above_floor: `launch --headless --window-size
      600x800` → `eval innerWidth` == 600 and a `(max-width: 700px)` media query
      matches (true emulation at ≥500px widths). Verified live (Firefox
      153.0.3): innerWidth=600, media query matched.
- [x] live_133_launch_window_size_floor_warning: `launch --headless --window-size
      390x844` → `eval innerWidth` ∈ [390, 500] (platform floor clamp) AND the
      launch envelope reports the requested 390×844 alongside a below-floor
      warning. Verified live: innerWidth clamped to 500, envelope carried
      `window_size.below_floor: true` + a `warnings` entry.
- [x] live_133_screenshot_batch_mobile: with the live tab on example.com,
      `screenshot --window-size 390x844` → PNG exactly 390 px wide; envelope
      `capture == "batch-window-size"`. Verified live: PNG IHDR 390×844.
- [x] unit_launch_window_size_validation: `0x0`, `x`, `390`, `390x` all rejected
      with a user error naming the expected `WxH` form. See
      `crates/ff-rdp-cli/src/util/window_size.rs` unit tests.
- [x] e2e_help_viewport_pointers: `eval_help_mentions_resize_to_no_op`,
      `screenshot_help_mentions_window_size_and_resize_to`, and
      `responsive_help_points_at_window_size_commands` (tests/e2e/{eval,
      screenshot,responsive}.rs) assert eval/screenshot help mention the
      headless resizeTo() no-op, and responsive help points at
      `--window-size`.

`live_133_screenshot_batch_dppx` [dropped — see [[viewport-emulation]]
addendum]: `layout.css.devPixelsPerPx` was confirmed to have zero effect on
the `--screenshot` batch-capture raster (Firefox 153.0.3, tested with and
without e10s), so no `--dppx` flag was added to `screenshot` — shipping one
would have been a silent no-op. `screenshot_has_no_dppx_flag`
(tests/e2e/screenshot.rs) asserts the flag does not exist.

## Notes

Scope was settled by [[viewport-emulation]] — no research theme remains, no scope
hinge. **Sonnet-implementable** (mechanical: arg pass-through, a subprocess capture
path, docs, a deletion); use `model-implement sonnet` when running via new-ralph-loop.
A future `--device iphone-15` preset (window-size + dppx + touch + UA bundle) is
deliberately out of scope — file separately if wanted.
Depends on [[iteration-131-measurement-honesty]] (both touch responsive's
warning/simulation surfaces — serialize to avoid conflicts).

**iter-132 post-merge note (2026-08-09):** no scope changes — iter-132 was a
CLI-polish grab-bag (jq error rendering, `dom` live `value` field, top-level
`await` in `eval`, flag-vs-subcommand hints, spawn-lock GC) with no overlap
with this iteration's `launch`/`screenshot`/`responsive` surfaces. The one
shared file is `crates/ff-rdp-cli/src/cli/args.rs` (the long `--help` text
constants) — iter-132 appended two new doc paragraphs there (top-level
await, dom live-value). Both are append-only, non-overlapping insertions
relative to where Theme A/B/C's help text lands in this plan, so a rebase
onto the merged iter-132 base should be conflict-free.
