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
status: completed
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

## Acceptance Criteria [6/6]

- [x] live_135_screenshot_ff153_capture: `ff-rdp screenshot -o <path>` against
      headless Firefox 153.x writes a valid PNG (magic bytes + non-zero
      dimensions), no `missing 'data' field` error
- [x] live_135_screenshot_full_page_taller: `--full-page` PNG height > plain
      viewport PNG height on the same page
- [x] live_135_screenshot_error_not_misleading: a forced parse failure while
      headless does NOT emit the "relaunch with --headless" hint
- [x] `unit_screenshot_capture_parses_ff153_shape`: recorded-fixture unit test
      for the Firefox 153 reply shape
- [x] `unit_screenshot_capture_parses_legacy_shape`: the pre-153
      `{value:{data:...}}` shape still parses (no version regression)
- [x] preexisting_reds_recheck: `live_screenshot_ff151_cli`,
      `live_screenshot_ff151_produces_valid_png`, and
      `live_screenshot_bulk_fallback_then_eval` re-run and their status
      recorded — green, or explicitly re-triaged with a reason

## Results

### Theme A — the reply shape never drifted

Raw wire capture (`FF_RDP_TRACE_RAW=1 RUST_LOG=trace ff-rdp screenshot`) against
Firefox 153.0.3:

```json
{"value":{"data":null,
          "filename":"Bildschirmfoto am 2026-08-09 um 12.17.19.png",
          "messages":[{"level":"error",
                       "text":"Fehler beim Erstellen der Grafik. Sie war wahrscheinlich zu groß."}]},
 "from":"server1.conn6.screenshotActor7"}
```

The `data` key is present and `null`; no longString, no rename, no extra
nesting, no async ack. The plan's three candidate hypotheses were all wrong.

**Actual root cause**: ff-rdp omitted `snapshotScale` whenever
`windowDpr * windowZoom == 1.0` (an iter-77 "keep the packet minimal"
optimisation). `capture-screenshot.js` has **no default** for it —
`const ratio = args.snapshotScale;` goes straight into
`drawSnapshot(rect, ratio, …)`, `snapshot.width / undefined` is `NaN`,
`canvas.toDataURL` throws, and the catch returns `null`. The retry guard
`!data && ratio > 1.0` is also `false` for `undefined`, so there is no second
attempt.

**Why 153 and not earlier**: on 149–152 `screenshotActor.capture` failed first
at actor-module load, and ff-rdp fell back to the parent-process `drawSnapshot`
path. Firefox 153 fixed that load —
[Bug 2043900](https://bugzilla.mozilla.org/show_bug.cgi?id=2043900),
`414cbad5bf8b` — so the request reached the renderer for the first time and the
latent omission became fatal on every capture. This also confirms the plan's
"not a regression from 128–133": the omission has been on the wire since
iter-77.

Recorded in `kb/rdp/actors/screenshot.md` (§ iter-135) and
[[decision-log]] DEC-025.

### Theme B — parse both shapes

- `ScreenshotArgsExt::snapshot_scale` is now `f64`, always serialised;
  `ScreenshotFront::capture` always sends `Some(scale)`.
- New `ff_rdp_core::parse_capture_response()` accepts the success shape from
  every Firefox 149→153 and folds `messages` into the error when `data` is
  absent/null/empty. Used by both `ScreenshotActor::capture` and
  `screenshot_via_target`.
- `specs::screenshot::response::CaptureValue.data` became `Option<String>` and
  gained `messages: Vec<CaptureMessage>` — the old `data: String` failed to
  deserialise a `null` outright.
- No longString branch was added: the wire proved the payload is a plain inline
  string, and speculative untested code is worse than none.
- **No new `allow-spec-drift` annotation was needed** — `snapshotScale` is
  already covered by the existing SD-1 annotation on `ScreenshotArgsExt`.

### Theme C — the misleading error is gone

`relaunch with: ff-rdp launch --headless` and `screenshots require headless mode`
are removed from every screenshot error path, plus the false
`version_mismatch_message()` ("screenshot actor not found …") on the
drawSnapshot-fallback path — that path is only reached *after* an actor was
found and called. Replaced by `capture_failure_message()`. See DEC-026.

### Theme D — the iteration-110 "known reds" were this bug

All 16 screenshot live tests are green on Firefox 153.0.3
(`--test-threads=1`, no other Firefox running), including every test in the
plan's blast radius:

| test | status |
| --- | --- |
| `live_61r_screenshot::live_screenshot_dpr_string_accepted` | green |
| `live_92_screenshot_full_page::live_screenshot_full_page_md5_differs_from_viewport` | green |
| `live_screenshot_shim::live_screenshot_no_args_on_firefox_151` | green |
| `live_screenshot_ff151::live_screenshot_ff151_cli` | green |
| `live_screenshot_ff151::live_screenshot_ff151_produces_valid_png` | green |
| `live_screenshot_bulk_fallback::live_screenshot_bulk_fallback_then_eval` | green |

The three catalogued in [[iteration-110-post-batch-live-sweep]] as
pre-existing/environmental were **the same root cause, mis-filed**. They are no
longer known reds; a future red in them is a real regression.

### Follow-up not taken here

The SD-2 workaround (parent-process `drawSnapshot` for the Firefox 151
module-load regression) is no longer *needed* on 153, but it still guards
151/152 and is now also the fallback for a null-data capture, so it stays.
Version-gating it is a separate decision with its own live-matrix cost.

## Notes

- Fixtures must be **recorded from real Firefox**, never hand-written — see
  `crates/ff-rdp-core/tests/live_record_fixtures.rs`.
- Sweep baseline for comparison: 1276 passed / 15 failed on 2026-08-09, of which
  6 were harness contamination (a stray `ff-rdp launch` Firefox left running
  during the sweep, which breaks the daemon-stop and profile-prune tests) and 6
  were iteration-110 known reds. Re-run sequentially with no other Firefox alive
  to reproduce cleanly.
