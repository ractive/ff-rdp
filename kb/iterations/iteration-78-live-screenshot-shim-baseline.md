---
title: "Iteration 78: Live tests for iter-77 spec-drift fixes"
type: iteration
date: 2026-05-25
status: planned
branch: iter-78/live-tests-for-iter-77
depends_on:
  - iteration-77-spec-drift-and-windows-reparse-points
firefox_refs:
  - path: devtools/server/actors/screenshot.js
    lines: "1-50"
    why: "Re-verify the live PNG round-trip is unchanged after the ScreenshotArgsExt shim landed in iter-77."
kb_refs:
  - kb/rdp/actors/screenshot.md
  - kb/rdp/actors/webconsole.md
first_call_sites: []
dogfood_path: |
  # Replays the iter-77 dogfood checks against a live Firefox.
  ff-rdp screenshot --fullpage -o /tmp/iter-78-page.png https://example.com
  ff-rdp --log-rdp-trace eval --frame "$FRAME_ACTOR" 'location.href' https://example.com
  ff-rdp --log-rdp-trace eval --subscribe console \
    'console.log("hello %s, you are %d", "world", 42)' https://example.com
tags: [iteration, protocol, live-tests, carry-over]
---

Carry-over plan filed before iter-77 merged.  Captures the three
live-Firefox-gated tests that were marked `[deferred]` on the
iter-77 ACs:

## Tasks

- [ ] Add `crates/ff-rdp-cli/tests/live_screenshot_shim.rs::live_screenshot_unchanged_after_shim`
- [ ] Add `crates/ff-rdp-cli/tests/live_eval_scope.rs::live_eval_in_frame`
- [ ] Add `crates/ff-rdp-cli/tests/live_console_printf.rs::live_console_printf_e2e`

## Acceptance Criteria [0/3]

- [ ] `live_screenshot_unchanged_after_shim`: PNG hash matches the pre-iter-77 baseline. Gated `FF_RDP_LIVE_TESTS=1`.
- [ ] `live_eval_in_frame`: eval --frame against an iframe returns the iframe's location. Gated `FF_RDP_LIVE_TESTS=1`.
- [ ] `live_console_printf_e2e`: `console.log("hello %s, you are %d", "world", 42)` round-trips through ff-rdp formatted as `"hello world, you are 42"`. Gated `FF_RDP_LIVE_TESTS=1`.

## Out of scope

- Any new wire changes — this iter only adds live tests.
- Filing the Mozilla Bugzilla bug for the `screenshot.args` spec-dict gap. The `// allow-spec-drift: bug TBD` annotation on `ScreenshotArgsExt` stays as-is until the bug is filed by a human; not blocking this iteration.

## References

- [[iteration-77-spec-drift-and-windows-reparse-points]]
