---
title: "Iteration 92: screenshot --full-page regression on FF 151 + navigate/run/index dom-complete parity"
type: iteration
date: 2026-06-01
status: in-progress
branch: iter-92/full-page-and-navigate-parity
depends_on:
  - iteration-89-screenshot-fifth-attempt-single-theme
  - iteration-90-daemon-lifecycle-state-sharing
firefox_refs: []
kb_refs:
  - kb/dogfooding/dogfooding-session-59.md
  - kb/iterations/iteration-83-dogfood-55-real-fixes.md
  - kb/iterations/iteration-89-screenshot-fifth-attempt-single-theme.md
first_call_sites:
  - primitive: WindowGlobalTarget screenshot path forwards `fullPage:true`
    site: crates/ff-rdp-cli/src/commands/screenshot.rs
  - primitive: navigate short-circuit guard (reject stale readyState before commit observed)
    site: crates/ff-rdp-cli/src/commands/navigate.rs
dogfood_script: iteration-92-full-page-and-navigate-parity.dogfood.sh
tags:
  - iteration
  - screenshot
  - navigate
  - regression
  - dogfood-59
---

# Iteration 92 — full-page screenshot regression + navigate parity

Two correctness regressions surfaced by [[dogfooding-session-59]] against
a fresh MDN target. Both regress documented ACs and produce silently
wrong output (no error, no warning) — the worst failure mode.

## Themes

### A. `screenshot --full-page` silently no-ops on FF 151

iter-89 rerouted the screenshot path through `WindowGlobalTarget` to
restore PNG capture on FF 151. The reroute dropped the `fullPage:true`
option somewhere between CLI parse and the actor request, so
`screenshot --full-page` now produces a viewport-sized PNG that is
**byte-identical (md5)** to a non-`--full-page` capture of the same page.

Regresses iter-83 Theme B AC: *"live_screenshot_full_page: PNG height ≥
scrollHeight × DPR"*.

Evidence: dogfooding-session-59 §Issues 1 — `ff-rdp screenshot
--full-page -o /tmp/df59-fp.png` on `https://developer.mozilla.org/.../JavaScript`
produced a 1366×683 PNG identical to the viewport capture.

### B. `navigate` vs `run`/`index` dom-complete mismatch

`ff-rdp navigate <url>` returns `elapsed_ms: 0, ready_state: "complete"`
on the second nav to the same tab — it observes the *pre-existing*
`document.readyState` from the prior load rather than waiting for the
new commit. `ff-rdp run` and `ff-rdp index` route through the same
`navigate::run()` entry point yet hit a real dom-complete timeout at
10s on `example.com`, suggesting a divergence in how the watcher is
primed or how `tabNavigated` is consumed across entry points.

Pick one truth: `navigate` should reject stale `readyState == complete`
unless a fresh `tabNavigated` event was observed for *this* call, and
`run`/`index` should not time out on a page that has genuinely
completed.

## Pre-fix repro

Per the pre-fix-repro convention (iter-87): each theme ships a test
named `pre_fix_repro_*` that **fails on `origin/main`** and passes on
the branch.

- Theme A: `pre_fix_repro_screenshot_full_page_taller_than_viewport`
  — live test on a fixture page with `body { height: 4000px }`; assert
  captured PNG height ≥ `scrollHeight × devicePixelRatio - 1` (allow
  one row rounding). Must fail on main (current viewport-only output).
- Theme B: `pre_fix_repro_navigate_second_call_waits_for_new_commit`
  — live test: navigate to URL A, then navigate to URL B in the same
  tab; assert the second call's `elapsed_ms > 0` AND that querying
  `document.location.href` after the call returns B (not A). Must fail
  on main (current `elapsed_ms: 0` short-circuit).

## Hard rule

Single iteration, two themes. Each AC names a live test (per
CLAUDE.md AC checkbox convention). No bundling beyond these two themes
— the polish list from session 59 (cascade note, network text, daemon
race, render_blocking divergence) is **out of scope** and goes to a
follow-up iter-94.

## Tasks

### Theme A — restore `fullPage:true` plumbing through WindowGlobalTarget [5/5] [pre_fix_repro_test: pre_fix_repro_screenshot_full_page_taller_than_viewport]

- [x] Trace `full_page` from `Command::Screenshot { full_page, .. }`
      (crates/ff-rdp-cli/src/commands/screenshot.rs) through
      `screenshot_via_window_global_target` and any helper introduced
      in iter-89; identify where the boolean stops being forwarded.
      Document the gap in a short comment at the regression site.
- [x] Forward `full_page` end-to-end to the `WindowGlobalTarget`
      capture request. The core actor request builder in
      `crates/ff-rdp-core/src/actors/screenshot_content.rs` already
      accepts `full_page` and emits `"fullPage": <bool>` (line 34);
      the CLI-side reroute must pass the flag, not drop it.
- [x] Add a unit test in `screenshot_content.rs` (or alongside the
      reroute) that captures the JSON payload sent by the
      WindowGlobalTarget path and asserts `"fullPage": true` when
      requested. Mirrors the existing
      `capture_sends_full_page_true_when_requested` test for the
      legacy path.
- [x] Land the pre-fix repro live test
      `pre_fix_repro_screenshot_full_page_taller_than_viewport`
      (gated by `FF_RDP_LIVE_TESTS=1`). Use a `data:text/html` URL
      with a tall body so the assertion does not depend on network
      content.
- [x] dogfood_script Theme A: capture both viewport and full-page
      PNGs of a tall fixture page; assert their md5 differs AND
      `identify -format '%h'` reports full-page height > viewport
      height.

### Theme B — navigate reject-stale-readyState + run/index parity [5/5] [pre_fix_repro_test: pre_fix_repro_navigate_second_call_waits_for_new_commit]

- [x] In `crates/ff-rdp-cli/src/commands/navigate.rs`, audit the
      commit-detection loop (the "Ignore pre-existing/stale
      dom-complete events" comment around line 237 hints the watcher
      already has a guard — verify it covers the `readyState == complete`
      poll path too, not just the event path).
- [x] Add a navigation epoch/SHA: capture `document.location.href` +
      `performance.timing.navigationStart` (or equivalent) *before*
      dispatching navigate; reject any commit signal whose
      `navigationStart` ≤ the captured value. Applies to both the
      events path and the readyState-poll path.
- [x] Diagnose why `run`/`index` time out where `navigate` succeeds.
      Hypothesis: both call `navigate_run` but `run`/`index` may
      consume the same `tabNavigated` event the watcher needs (or
      vice versa). Fix at the watcher subscription site, not at the
      caller.
- [x] Land the pre-fix repro live test
      `pre_fix_repro_navigate_second_call_waits_for_new_commit`.
      Use two distinct `data:` URLs so DNS/network variance does not
      flake the assertion.
- [x] dogfood_script Theme B: `navigate A`, `navigate B`, `run -e
      "fetch(A).then(r=>r.status)"` against a stable target; assert
      both navigates report `elapsed_ms > 0` after the first, and
      `run` exits 0 (no dom-complete timeout).

## Acceptance Criteria [8/8]

- [x] `pre_fix_repro_screenshot_full_page_taller_than_viewport`:
      live test on a tall `data:` page; PNG height ≥ `scrollHeight ×
      DPR - 1`; fails on origin/main, passes on branch.
- [x] `unit_window_global_target_screenshot_forwards_full_page`:
      mocks the WindowGlobalTarget transport; asserts the captured
      request JSON contains `"fullPage": true` when the CLI flag is
      set.
- [x] `live_screenshot_full_page_md5_differs_from_viewport`: against
      a tall fixture page, assert md5 of full-page PNG differs from
      viewport PNG (catches the exact session-59 regression mode).
- [x] `pre_fix_repro_navigate_second_call_waits_for_new_commit`:
      live test; second navigate to a different URL reports
      `elapsed_ms > 0`; fails on origin/main.
- [x] `unit_navigate_rejects_stale_ready_state`: feed the
      commit-detector a `readyState == complete` reading whose
      `navigationStart` predates the call's dispatch timestamp;
      assert the detector keeps waiting.
- [x] `live_run_navigate_parity`: `ff-rdp run --url <url> -e "1"`
      exits 0 within the default navigate budget for a URL where
      `ff-rdp navigate <url>` also exits 0; covers the run/navigate
      divergence directly.
- [x] `live_index_navigate_parity`: `ff-rdp index <url> --depth 0`
      exits 0 for the same URL as above; covers the index path.
- [x] `dogfood_script_full_run_iter_92`: the sibling `.dogfood.sh`
      exits 0 and writes `/tmp/ff-rdp-iter-92-dogfood-ok`.

## Out of scope

- **`eval --via-debugger` / CSP bypass** (session-59 §2). Bigger
  design (DevTools console sandbox vs the current `script`-element
  injection). Files as its own iteration — iter-93.
- **`dom stats` vs `perf audit` render_blocking divergence**
  (session-59 §4). Pure counter alignment; folds into iter-94 polish
  bundle.
- **`cascade` empty-rules note / `inherited_from`** (session-59 §5).
  Cosmetic ergonomics; iter-94.
- **`network --format text` bare-number rows** (session-59 §6).
  Formatter polish; iter-94.
- **`daemon stop` 3s race window** (session-59 §8). iter-86 Theme A
  was "partial"; needs its own surgical fix; iter-94.

## References

- [[dogfooding-session-59]]
- [[iteration-83-dogfood-55-real-fixes]] (origin of the
  `live_screenshot_full_page` AC)
- [[iteration-89-screenshot-fifth-attempt-single-theme]] (the iter
  whose reroute caused Theme A regression)
- [[iteration-87-gate-hardening-required-checks-and-dogfood-linter]]
  (pre-fix-repro convention)
