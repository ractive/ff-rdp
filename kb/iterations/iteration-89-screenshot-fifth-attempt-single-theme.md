---
title: "Iteration 89: screenshot fifth attempt — single theme, route through WindowGlobalTarget on FF 151"
type: iteration
date: 2026-05-29
status: planned
branch: iter-89/screenshot-fifth-attempt-single-theme
depends_on:
  - iteration-87-gate-hardening-required-checks-and-dogfood-linter
firefox_refs:
  - lines: 1-144
    path: devtools/server/actors/screenshot-content.js
    why: "FF 151 split the screenshot path: `screenshot.js` is a 25-line re-export shim; the real WindowGlobal-target capture lives in `screenshot-content.js`. The root-form `screenshotActor` field is absent. iter-85 added per-target probing scaffolding (`try_two_step_screenshot`) but session-58 confirms `ff-rdp screenshot -o /tmp/x.png` still errors with `screenshot actor not found in Firefox 151 root form` — meaning the fallback ladder either isn't reached or isn't wired. This iteration captures the FF 151 `getRoot` reply as a real fixture and lands the `getTab → tabActor → takeScreenshot` path."
kb_refs:
  - kb/dogfooding/dogfooding-session-58.md
  - kb/dogfooding/dogfooding-session-57.md
  - kb/iterations/iteration-85-dogfood-57-carryovers-and-runnable-dogfood-path.md
  - kb/iterations/iteration-87-gate-hardening-required-checks-and-dogfood-linter.md
  - kb/rdp/actors/screenshot.md
first_call_sites:
  - primitive: screenshot_via_target() routes via WindowGlobalTarget on FF 151
    site: crates/ff-rdp-core/src/actors/screenshot.rs
  - primitive: FF 151 getRoot fixture (recorded, no `screenshotActor` field)
    site: crates/ff-rdp-core/tests/fixtures/getroot_ff151.json
dogfood_script: iteration-89-screenshot-fifth-attempt-single-theme.dogfood.sh
tags:
  - iteration
  - screenshot
  - bugfix
  - carry-over
---

# Iteration 89 — screenshot, for real this time

`ff-rdp screenshot -o /tmp/x.png` on FF 151 errors with `screenshot:
screenshot actor not found in Firefox 151 root form`. Session-58
reproduced session-57 verbatim. iter-85's plan said the
`try_two_step_screenshot` fallback ladder was wired and the
`screenshot_via_target()` path was implemented. The live CLI says
otherwise.

iter-89 follows the same one-theme discipline as iter-88. Capture a real
FF 151 `getRoot` reply, implement the `getTab → tabActor → takeScreenshot`
path against the recorded fixture, ship a live test that produces actual
PNG bytes from a subprocess.

## Hard rule

Do not tick an AC checkbox until `iteration-89-….dogfood.sh` exits 0
on a live FF 151 and writes `/tmp/ff-rdp-iter-89-dogfood-ok`.

## Pre-fix repro

Per the [[iteration-87-gate-hardening-required-checks-and-dogfood-linter#pre-fix-repro-convention|iter-87 convention]],
`pre_fix_repro_screenshot_fixture_red_then_green` exercises the screenshot
code path against the recorded `getroot_ff151.json` fixture and asserts
that on `origin/main` the error path ("screenshot actor not found …") is
taken, and on branch HEAD a PNG byte sequence is produced.

## Tasks

### Theme A — screenshot routed through WindowGlobalTarget on FF 151 [0/5] [pre_fix_repro_test: pre_fix_repro_screenshot_fixture_red_then_green]

- [ ] Capture the FF 151 `getRoot` reply (no `screenshotActor` field) via
      the live-record harness and check it in as
      `crates/ff-rdp-core/tests/fixtures/getroot_ff151.json`. Real
      recording — the iter-85 fixture was synthetic.
- [ ] Implement `screenshot_via_target()` in
      `crates/ff-rdp-core/src/actors/screenshot.rs`:
      1. Send `getTab` to the root actor.
      2. Read the returned `tab.actor` (the WindowGlobalTarget /
         BrowsingContextTargetActor ID).
      3. Send `takeScreenshot` against the tab actor (with `screenshot`
         as a secondary message-name fallback for older builds).
      4. Decode the `data` field from base64 dataURL to raw PNG bytes.
      Reference: `devtools/server/actors/screenshot-content.js` lines
      1–144 in the Firefox tree (FF 151 split moved the capture body
      here from `screenshot.js`).
- [ ] `pre_fix_repro_screenshot_fixture_red_then_green`: loads the
      recorded `getRoot` fixture, runs the screenshot dispatcher, asserts
      the error path on `origin/main` (current behavior) and a non-empty
      PNG-magic-byte buffer on branch HEAD.
- [ ] Live test `live_screenshot_ff151_cli`: spawns `ff-rdp screenshot
      -o <tmp>/x.png` as a subprocess against `https://example.com` on a
      live headless FF 151. Asserts (a) file exists, (b) size > 1000
      bytes, (c) starts with the PNG magic `89 50 4E 47 0D 0A 1A 0A`.
- [ ] dogfood_script Theme A block exits 0.

## Acceptance Criteria [0/4]

- [ ] pre_fix_repro_screenshot_fixture_red_then_green: error path on
      `origin/main`, PNG bytes on branch HEAD. Verified by `xtask
      check-pre-fix-repro`.
- [ ] unit_screenshot_via_target_returns_png: against the recorded
      `getroot_ff151.json` fixture (plus a mocked `getTab` /
      `takeScreenshot` exchange), the dispatcher returns a buffer
      starting with the PNG magic bytes.
- [ ] live_screenshot_ff151_cli: subprocess CLI invocation against
      example.com on FF 151 writes a valid PNG > 1000 bytes.
- [ ] dogfood_script_full_run_iter_89: sibling `.dogfood.sh` exits 0
      and writes `/tmp/ff-rdp-iter-89-dogfood-ok`.

## Out of scope

- Full-page screenshots, scrolling capture, multi-monitor capture,
  element-bounded capture. Get the basic `-o` happy path landing first.
- Capture format alternatives (JPEG, WebP). PNG only.
- Retry policies on transient capture failures. The current bug is
  routing, not reliability.

## References

- [[dogfooding-session-58]] — 5th-confirmation that this is still broken
- [[iteration-85-dogfood-57-carryovers-and-runnable-dogfood-path]] — 4th attempt
- [[iteration-87-gate-hardening-required-checks-and-dogfood-linter]] — the gate
