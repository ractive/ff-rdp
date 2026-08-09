---
branch: iter-135/screenshot-ff153-capture-drift
date: 2026-08-09
depends_on:
  - kb/iterations/iteration-133-viewport-emulation.md
dogfood_path: |
  ff-rdp launch --headless
  ff-rdp navigate https://example.com >/dev/null
  ff-rdp screenshot -o /tmp/shot.png
  # → must produce a valid PNG on Firefox 153.x, NOT
  #   "screenshotActor capture response missing 'data' field"
  ff-rdp screenshot --full-page -o /tmp/full.png
  # → taller than the viewport capture
first_call_sites: []
status: planned
---

# Iteration 135: screenshot capture drift on Firefox 153

The live RDP screenshot path is **broken on Firefox 153.0.3**. Every capture
fails with:

```
screenshot: screenshotActor.capture failed (invalid packet: screenshotActor
capture response missing 'data' field) — screenshots require headless mode;
relaunch with: ff-rdp launch --headless
```

Found during the post-batch live sweep on main after iterations 128–133
(2026-08-09, `/tmp/ff-sweep.log`). **This is not a regression from that batch.**

## Evidence

- A binary built from `ac3cd30` (the commit immediately before iter-128) fails
  **identically** against the same Firefox 153.0.3 instance. Confirmed by
  building that commit in a throwaway worktree and running both binaries
  back-to-back against one launched browser.
- `crates/ff-rdp-core/src/actors/screenshot.rs` has not changed since iter-117
  (2026-07-10). The only screenshot commit in the 128–133 range (`7ac7d3b`,
  iter-133) adds the `--window-size` batch path and refactors result-building
  via `build_capture_result`; it leaves the live RDP path (`run_core`) intact.
- Therefore this is **Firefox-side reply-shape drift**, landing somewhere in
  152 → 153.
- `ScreenshotActor::capture` (screenshot.rs:189-202) assumes
  `{ "value": { "data": "data:image/png;base64,..." } }`, falling back to the
  response root. Firefox 153 evidently returns neither.
- The **batch capture path added in iter-133 is unaffected** —
  `ff-rdp screenshot --window-size 390x844` produces an exact 390×844 PNG. It
  shells out to `firefox --headless --screenshot` and never touches the actor.
  This is the current workaround and the reason the break is survivable.

## Blast radius

Six live tests, one root cause:

- `live_61r_screenshot::live_screenshot_dpr_string_accepted`
- `live_92_screenshot_full_page::live_screenshot_full_page_md5_differs_from_viewport`
- `live_screenshot_shim::live_screenshot_no_args_on_firefox_151`
- `live_screenshot_ff151::{live_screenshot_ff151_cli,live_screenshot_ff151_produces_valid_png}`
  (catalogued as "known reds" in [[iteration-110-post-batch-live-sweep]] — very
  likely this same drift, mis-filed as environmental)
- `live_screenshot_bulk_fallback::live_screenshot_bulk_fallback_then_eval`

`screenshot` is a headline command; this blocks the v0.4.0 release.

## Themes

### Theme A — find the real Firefox 153 reply shape

Spelunk the live protocol against a real Firefox 153 instance: capture the raw
`capture` response packet and record its actual structure. Do NOT guess from the
published spec dict — iter-129 showed the server's real behaviour and the spec
diverge. Record findings in `kb/rdp/actors/screenshot.md`.

Candidates worth checking explicitly, but let the wire decide:
- the payload moved behind a longString actor (cf. iter-102's longString sweep)
- the field was renamed, or nested one level deeper
- capture is now async and the first reply is only an ack

### Theme B — parse the current shape, keep the old one working

Teach `ScreenshotActor::capture` the Firefox 153 shape **without** dropping the
pre-153 shape — the tool must keep working across Firefox versions. If the
payload is a longString, resolve it through the existing longString machinery
rather than adding a second mechanism.

Annotate with `// allow-spec-drift: bug TBD (<rationale>)` if ff-rdp must read a
field the published spec dict doesn't declare, per the spec-drift convention.

### Theme C — stop the misleading error message

The current error tells users to `relaunch with: ff-rdp launch --headless` even
when they are **already headless** — it fires on any missing-`data` response and
blames the wrong cause. Make the hint conditional on actually detecting a
non-headless session, or drop the hint and report the real parse failure.

### Theme D — re-classify the iteration-110 "known reds"

Cross-check the three screenshot entries catalogued as pre-existing/environmental
in [[iteration-110-post-batch-live-sweep]]. If they share this root cause, they
should go green with Theme B — say so explicitly rather than leaving them parked
as permanent reds.

## Acceptance criteria

- [ ] live_135_screenshot_ff153_capture: `ff-rdp screenshot -o <path>` against
      headless Firefox 153.x writes a valid PNG (magic bytes + non-zero
      dimensions), no `missing 'data' field` error
- [ ] live_135_screenshot_full_page_taller: `--full-page` PNG height > plain
      viewport PNG height on the same page
- [ ] live_135_screenshot_error_not_misleading: a forced parse failure while
      headless does NOT emit the "relaunch with --headless" hint
- [ ] unit_screenshot_capture_parses_ff153_shape: recorded-fixture unit test for
      the Firefox 153 reply shape
- [ ] unit_screenshot_capture_parses_legacy_shape: the pre-153
      `{value:{data:...}}` shape still parses (no version regression)
- [ ] preexisting_reds_recheck: `live_screenshot_ff151::*` and
      `live_screenshot_bulk_fallback::*` re-run and their status recorded —
      green, or explicitly re-triaged with a reason

## Notes

- Fixtures must be **recorded from real Firefox**, never hand-written — see
  `crates/ff-rdp-core/tests/live_record_fixtures.rs`.
- Sweep baseline for comparison: 1276 passed / 15 failed on 2026-08-09, of which
  6 were harness contamination (a stray `ff-rdp launch` Firefox left running
  during the sweep, which breaks the daemon-stop and profile-prune tests) and 6
  were iteration-110 known reds. Re-run sequentially with no other Firefox alive
  to reproduce cleanly.
