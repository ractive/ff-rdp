---
title: "Iteration 257: Firefox 155 changed drawSnapshot's 4th argument to a dictionary and every --full-page screenshot is broken"
type: iteration
date: 2026-09-07
status: planned
branch: iter-257/drawsnapshot-dictionary-arg
depends_on:
  - iteration-92-full-page-and-navigate-parity
first_call_sites: []
dogfood_path: |
  # 1. The break, straight off the CLI. Any --full-page capture on FF 155:
  ff-rdp launch --headless
  ff-rdp navigate https://example.com
  ff-rdp screenshot --full-page --output /tmp/fp.png
  #    OBSERVED 2026-09-07 on Firefox 155.0.1:
  #    {"error":"screenshot: process-drawsnapshot fallback failed (invalid
  #      packet: screenshot_via_process_drawsnapshot: JS returned error:
  #      TypeError: WindowGlobalParent.drawSnapshot: Argument 4 can't be
  #      converted to a dictionary.) — Firefox 155 rendered no image for this
  #      capture. …","error_type":"User"}
  #    EXPECTED: a PNG whose height >= document scrollHeight * DPR.
  #
  # 2. The error message is itself part of the defect. It blames the *page*
  #    ("Very tall pages can exceed the renderer's limits — retry without
  #    --full-page") for what is a signature change in Firefox. A caller
  #    following that advice silently loses full-page capture and never learns
  #    why. Whatever the fix, this hint must stop firing on a TypeError.
  #
  # 3. Blast radius — every --full-page live test, measured in one sweep:
  FF_RDP_LIVE_TESTS=1 FF_RDP_LIVE_NETWORK_TESTS=1 cargo run -p xtask -- live-sweep
  #    OBSERVED 2026-09-07 (iteration 235's closing sweep, FF 155.0.1):
  #    LIVE_SWEEP_SUMMARY executed=320 skipped=0 preexisting=0 vanished=0
  #      launch_timeout=0 timed_out=0 total=320 — 309 passed / 11 failed,
  #    and SEVEN of the eleven are this one TypeError — every test in the
  #    suite that takes a --full-page capture, and nothing else:
  #      live_135_screenshot_ff153::live_135_screenshot_full_page_taller
  #      live_144_session_hygiene_followup::live_144_full_page_no_duplicate_header
  #      live_61l::live_screenshot_full_page
  #      live_61r_screenshot::live_screenshot_full_page
  #      live_92_screenshot_full_page::live_screenshot_full_page_md5_differs_from_viewport
  #      live_92_screenshot_full_page::pre_fix_repro_screenshot_full_page_taller_than_viewport
  #      live_screenshot_shim::live_screenshot_unchanged_after_shim
  #    The remaining four failures are unrelated and are dispositioned in
  #    iteration 235's PR body.
  #
  # 4. The call site, verbatim, in crates/ff-rdp-core/src/actors/screenshot.rs:
  #      await wg.drawSnapshot(rect, 1, "rgb(255,255,255)", {reset_scroll})
  #    where {reset_scroll} interpolates the *boolean literal* `true` or
  #    `false`. FF 155 wants argument 4 to be a dictionary; a boolean is not
  #    convertible, so the call throws before rendering anything.
  #
  # 5. After the fix, on FF 155 AND on the oldest supported build:
  FF_RDP_LIVE_TESTS=1 cargo test-live -p ff-rdp-cli --test live live_92_screenshot_full_page
  #    expected: green, PNG taller than the viewport, on both.
tags: [iteration, screenshot, firefox-155, spec-drift, regression, ff-rdp-core, carry-over]
takeover_reconciliation_252: "2026-09-09: correct source path is dom/chrome-webidl/WindowGlobalActors.webidl. Release155.0 introduced DrawSnapshotOptions{resetScrollPosition=false,drawView=false}, bug2058388 final commit78f289876b9c2022059c951097c71713142c67a0;154.0.1 and120 use boolean. Local Firefox source0088392 predates this change: use recorded release tags. ThemeB premise needs runtime investigation: CLI screenshot.rs607 unconditionally routes fullpage to process fallback, so sweep failures do NOT establish primary actor failure on155. The155 primary helper already uses the dictionary. Dictionary-first try/catch cannot detect old Firefox because objects coerce to boolean true. Original ACs remain intact; oldest-supported120 runtime proof still required. Official120 DMG downloaded and both SHA256/SHA512 verified, not installed/launched. Evidence saved in takeover firefox257-prep; https://bugzilla.mozilla.org/show_bug.cgi?id=2058388."
---

# Iteration 257: Firefox 155 changed `drawSnapshot`'s 4th argument to a dictionary and every `--full-page` screenshot is broken

> Filed 2026-09-07 as carry-over from [[iteration-235-live-bulk-cap-shrinks-a-process-global]]'s
> closing live sweep. Seven of that sweep's eleven failures are this single TypeError — every
> `--full-page` capture in the suite and nothing else — and it is a
> **product** defect, not a test-harness one: the CLI returns `error_type: "User"` and no image.

## The defect

`crates/ff-rdp-core/src/actors/screenshot.rs` builds parent-process JS that calls

```js
const snapshot = await wg.drawSnapshot(rect, 1, "rgb(255,255,255)", true);
```

The comment above it records the contract iteration 92 measured:

> The 4th arg to `drawSnapshot(rect, scale, color, resetScrollPosition)` is `resetScrollPosition`
> (see `dom/webidl/WindowGlobalActors.webidl`), NOT a fullpage flag.

On Firefox 155.0.1 that is no longer true. The call throws
`TypeError: WindowGlobalParent.drawSnapshot: Argument 4 can't be converted to a dictionary.`
before rendering anything, so the fallback path returns an error string and the CLI reports a
failed capture.

This is exactly the class of change `// allow-spec-drift` exists to track, and the call site
already carries `// allow-spec-drift: bug TBD (SD-2: BrowsingContext.drawSnapshot used via
…)` — the drift annotation was there, the *verification against a current Firefox* was not.

## Themes

- **A — Establish the new signature from Firefox source, not by guessing.** Read
  `dom/webidl/WindowGlobalActors.webidl` in the 155 tree (`FF_RDP_FIREFOX_PATH`) and record
  the dictionary's actual member names in `kb/rdp/flows/take-screenshot.md`. Do not ship a
  speculative `{resetScrollPosition: true}` without having read the IDL — a dictionary silently
  ignores unknown members, so a wrong member name produces a screenshot that is quietly wrong
  rather than an error, which is worse than today's loud failure.
- **B — Why is the fallback reached at all?** All seven failures go through
  `screenshot_via_process_drawsnapshot`, i.e. the *primary* screenshot-actor path had already
  failed and the CLI fell back. Non-`--full-page` captures pass, so the primary path works for
  them. Establish why `--full-page` always lands in the fallback on FF 155 before assuming the
  fallback is the whole story — a primary path that handled full-page would make the drift moot.
- **C — Version compatibility.** ff-rdp advertises a minimum supported Firefox of 120. If the
  dictionary form is 155-only, the call needs to work on both; decide between feature-detection
  in the JS (`try` the dictionary, fall back to the boolean) and a version gate, and say which
  and why.
- **D — The misleading hint.** On a TypeError the CLI currently advises "Very tall pages can
  exceed the renderer's limits — retry without `--full-page`". That sentence is true of a real
  renderer limit and false of a signature change, and it sent this sweep's first reading down the
  wrong path. The hint must be conditional on the failure actually looking like a size limit.

## Tasks

### A. Root cause [0/3]
- [ ] Read `dom/webidl/WindowGlobalActors.webidl` at the Firefox 155 tag and record the current
      `drawSnapshot` signature and dictionary members in `kb/rdp/flows/take-screenshot.md`
- [ ] Determine which Firefox release changed it, and note it against the minimum supported 120
- [ ] Establish why the primary screenshot-actor path also failed on FF 155 (Theme B)

### B. The fix [0/2]
- [ ] Pass argument 4 in the form the running Firefox accepts, working on 155 and on the oldest
      supported build
- [ ] Stop the "very tall pages" hint from firing on a `TypeError` (Theme D)

### C. Proof [0/2]
- [ ] The seven failing live tests green on FF 155
- [ ] A live test that fails loudly if `drawSnapshot` ever again throws rather than rendering —
      per CLAUDE.md, a spec-method change needs a live Firefox test, not a unit test

## Acceptance Criteria [0/4]

- [ ] `ff-rdp screenshot --full-page` produces a PNG taller than the viewport on Firefox 155.0.1
- [ ] All seven pass in one sweep: `live_92_screenshot_full_page` (both tests),
      `live_61l::live_screenshot_full_page`, `live_61r_screenshot::live_screenshot_full_page`,
      `live_135_screenshot_full_page_taller`, `live_144_full_page_no_duplicate_header` and
      `live_screenshot_shim::live_screenshot_unchanged_after_shim`
- [ ] The `// allow-spec-drift` comment at the call site names a real Bugzilla issue, or states
      the measured 155 signature — `bug TBD` is no longer honest for a drift we have now hit
- [ ] No error path claims a page-size limit for a failure that was a `TypeError`

## Out of scope

- The other four failures in iteration 235's sweep. They are recorded with their dispositions in
  that iteration's PR body; two are the load-sensitive pair iteration 210 already tracks.

## References

- `crates/ff-rdp-core/src/actors/screenshot.rs` — the `drawSnapshot` JS and its `allow-spec-drift`
- `crates/ff-rdp-cli/src/commands/screenshot.rs` — the fallback and the misleading hint
- `kb/rdp/flows/take-screenshot.md`, `kb/research/screenshot-protocol-ff149.md`
- [[iteration-92-full-page-and-navigate-parity]] — where the `resetScrollPosition` contract was measured
- [[iteration-235-live-bulk-cap-shrinks-a-process-global]] — the sweep that found this
