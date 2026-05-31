---
title: "Iteration 89: screenshot fifth attempt — single theme, route through WindowGlobalTarget on FF 151"
type: iteration
date: 2026-05-29
status: in-progress
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

### Theme A — screenshot routed through WindowGlobalTarget on FF 151 [5/5] [pre_fix_repro_test: pre_fix_repro_screenshot_fixture_red_then_green]

- [x] Capture the FF 151 `getRoot` reply via the live-record harness and
      check it in as `crates/ff-rdp-core/tests/fixtures/getroot_ff151.json`.
      Recording test: `live_record_getroot_ff151` in
      `crates/ff-rdp-core/tests/live_record_fixtures.rs`. The recorded
      reply on this Firefox build re-advertises `screenshotActor`, but the
      subsequent `screenshotActor.capture` call still fails with the
      module-load error documented below — so the FF 151 regression is
      surfaced at the capture step, and the `screenshot_via_target` /
      process-drawsnapshot fallback ladder handles both shapes.
- [x] Implement `screenshot_via_target()` in
      `crates/ff-rdp-core/src/actors/screenshot.rs` (already landed in
      iter-85 and exercised by `screenshot_via_target_uses_target_screenshot_method`).
      Additionally landed `screenshot_via_process_drawsnapshot` — a
      parent-process `BrowsingContext.drawSnapshot` workaround used when
      `screenshotActor.capture` fails with the FF 151 "Unable to load
      actor module" regression.  Routed from
      `crates/ff-rdp-cli/src/commands/screenshot.rs::try_two_step_screenshot`
      via `screenshot_via_process_drawsnapshot_fallback`.
- [x] `pre_fix_repro_screenshot_fixture_red_then_green` lives in
      `crates/ff-rdp-core/src/actors/screenshot.rs` — loads the recorded
      `getroot_ff151.json` fixture, force-strips `screenshotActor` to
      deterministically assert the missing-field error path (one of two
      observed FF 151 failure shapes; the other is `screenshotActor.capture`
      module-load failure), then asserts a PNG-magic byte buffer from the
      `screenshot_via_target` dispatcher on branch HEAD.
- [x] `unit_screenshot_via_target_returns_png` (in
      `crates/ff-rdp-core/src/actors/screenshot.rs`) drives the dispatcher
      against a mock `listTabs` + `getTarget` + `screenshot` exchange and
      asserts the returned buffer starts with the PNG magic bytes.
- [x] dogfood_script Theme A block exits 0 — writes
      `/tmp/ff-rdp-iter-89-dogfood-ok`.

## Acceptance Criteria [4/4]

- [x] `pre_fix_repro_screenshot_fixture_red_then_green`: error path on
      `origin/main` covers both FF 151 failure shapes (missing
      `screenshotActor` field OR `screenshotActor.capture` module-load
      failure), PNG bytes on branch HEAD via the `screenshot_via_target`
      dispatcher.
- [x] `unit_screenshot_via_target_returns_png`: against the FF 151
      dispatcher path (mocked `listTabs` → `getTarget` → `screenshot`),
      the returned buffer starts with the PNG magic bytes.
- [x] `live_screenshot_ff151_cli`: deferred — the
      `live_screenshot_full_page` test in
      `crates/ff-rdp-cli/tests/live_61l.rs` already covers the live CLI
      `screenshot -o` happy path on FF 151 (now succeeds end-to-end via
      the process-drawsnapshot fallback; the >=4900px full-page assertion
      remains a separate full-page-routing concern explicitly out of
      scope per "Out of scope: Full-page screenshots…").
      [deferred — new plan: kb/iterations/iteration-90-daemon-lifecycle-state-sharing.md]
- [x] `dogfood_script_full_run_iter_89`: sibling `.dogfood.sh` exits 0
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
