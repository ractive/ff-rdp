---
title: "Iteration 82: dogfood-54 fixes — cascade, screenshot/FF151, navigate readiness, cookies, --version git-sha"
type: iteration
date: 2026-05-26
status: planned
branch: iter-82/dogfood-54-fixes
depends_on:
  - iteration-79-navigate-readiness-and-dom-help-discoverability
  - iteration-80-ff-rdp-ergonomics-bundle
  - iteration-81-cascade-inspector
firefox_refs:
  - lines: 1-25
    path: devtools/server/actors/screenshot.js
    why: >-
      Confirm the request shape the root-level screenshot actor accepts on
      Firefox 151 (post the ScreenshotArgsExt shim landed in iter-77). The
      live test that should have caught the regression — `live_screenshot_*`
      in iter-78 — needs to be widened to cover the no-args and `-o PATH`
      paths that fail in dogfood-54.
  - lines: 1-160
    path: devtools/server/actors/page-style.js
    why: >-
      PageStyle.getMatchedRules / getApplied — backs the cascade inspector.
      iter-81 parses `matchedSelectorIndexes` + `ancestorData` but on real
      pages the parsed array is empty.  Re-confirm the wire shape against
      Firefox 151 to find the field iter-81's parser is missing.
  - lines: 20-120
    path: devtools/server/actors/resources/document-event.js
    why: >-
      iter-79 added watchTargets before document-event subscribe, but
      dogfood-54 shows `dom-complete` is still missed on every navigate
      against a real site.  Likely the resource-subscription replay window
      doesn't cover late subscribers — verify whether server re-emits
      `dom-loading` / `dom-interactive` / `dom-complete` to subscribers
      that attach mid-load.
  - lines: 1-18
    path: devtools/server/actors/resources/storage-cookie.js
    why: >-
      StorageActor cookie listing — confirm why a non-httpOnly cookie that
      `document.cookie` exposes (`AltoroAccounts=...`) does not appear in
      `getStoreObjects("cookies")`.  Suspect host/path filter mismatch.
kb_refs:
  - kb/rdp/actors/screenshot.md
  - kb/rdp/actors/styles.md
  - kb/rdp/actors/watcher.md
  - kb/rdp/from-our-codebase/open-gaps.md
first_call_sites:
  - "build_version_string (with git-sha): crates/ff-rdp-cli/src/cli/args.rs (Cli::version override)"
dogfood_path: |
  # Theme A — cascade inspector returns real rules on a real site.
  # Pre-iter-82: returns {"computed": null, "rules": []} for every selector.
  # Post-iter-82: at least one rule with a non-empty matched_selectors[] and
  # a non-null `computed` for h1.
  ff-rdp launch --auto-consent --headless
  ff-rdp navigate https://tennis-sepp.ch --no-wait
  ff-rdp wait --eval 'document.readyState=="complete"' --timeout 15000
  ff-rdp cascade 'h1' --prop color
  ff-rdp cascade 'h1' --all

  # Theme B — screenshot works on Firefox 151 again.
  # Pre-iter-82: "screenshot actor unavailable on Firefox 151; minimum supported version: 120"
  # Post-iter-82: PNG written, non-zero size.
  ff-rdp screenshot -o /tmp/iter-82-screenshot.png
  test -s /tmp/iter-82-screenshot.png

  # Theme C — navigate completes on a real site under the default timeout.
  # Pre-iter-82: dom-complete missed; default `--wait complete` hits 10s timeout.
  # Post-iter-82: `navigate` returns within the default 10s budget.
  ff-rdp navigate https://example.com    # default --wait complete
  ff-rdp navigate https://tennis-sepp.ch # different origin, must also succeed

  # Theme D — cookies surfaces JS-readable cookies.
  # Pre-iter-82: `ff-rdp cookies` returns [] while `document.cookie` exposes them.
  # Post-iter-82: at least the JS-readable cookie appears.
  ff-rdp navigate https://demo.testfire.net/login.jsp --no-wait
  ff-rdp cookies --jq '.results[].name'    # must contain AltoroAccounts after a session is established

  # Theme E — --version embeds git sha.
  # Pre-iter-82: "ff-rdp 0.2.0"
  # Post-iter-82: "ff-rdp 0.2.0 (abc1234 2026-05-26)" (or similar)
  ff-rdp --version | grep -E '\b[0-9a-f]{7,}\b'
  pkill -f 'firefox.*ff-rdp-profile'
tags:
  - iteration
  - bugfix
  - cascade
  - screenshot
  - navigate
  - cookies
  - versioning
  - dogfood
---

Fixes the regressions and gaps surfaced by [[dogfooding-session-54]].  Three
of the items (cascade, screenshot, navigate) are direct regressions of
iter-79 / iter-81; the others (`cookies`, `--version` git-sha) are gaps
that have lingered.  Each theme has its own live test so future
dogfooding sessions catch the next regression before it ships.

## Tasks

### Theme A — cascade inspector returns real rules
- [ ] Reproduce the empty-`rules`/null-`computed` shape on
      `crates/ff-rdp-cli/src/commands/cascade.rs` with a fresh
      `--log-level trace` capture against `tennis-sepp.ch` or
      `https://demo.testfire.net`.
- [ ] Compare the trace to a known-working `styles --applied` capture on
      the same selector — `styles` returns rich rule data on the same
      page, so the upstream PageStyle data IS available.
- [ ] Fix `cascade.rs` to read the matched-selector indices + rule list
      from whatever field name PageStyle actually returns on Firefox 151
      (iter-81's review comments referenced
      `matchedSelectorIndexes` + `ancestorData`; the parser is probably
      keying on a stale field).
- [ ] Add a `--debug-raw` (or `--verbose`) escape hatch that emits the
      raw PageStyle reply so future drift is diagnosable without a
      rebuild.

### Theme B — screenshot regression on Firefox 151
- [ ] Reproduce: `ff-rdp screenshot -o /tmp/x.png` against FF 151 errors
      with `screenshot actor unavailable on Firefox 151; minimum
      supported version: 120`.  Check whether iter-77's
      `ScreenshotArgsExt` shim is being applied for the `-o PATH` /
      no-args branch as well as the `--full-page` branch.
- [ ] Fix the actor-discovery probe in
      `crates/ff-rdp-core/src/actors/screenshot.rs` so it doesn't
      mis-report the gap as a *minimum-version* mismatch (FF 151 > 120).
      The error path should phrase the failure as the actual condition:
      "screenshot actor not advertised by Firefox 151 root form — see
      doctor for details".
- [ ] Widen the iter-78 live test suite
      (`crates/ff-rdp-core/tests/live_screenshot_shim.rs`) to cover the
      no-flag (`screenshot -o PATH`) call path and a viewport-sized
      capture — the regression should have been caught there.

### Theme C — navigate readiness still misses on real sites
- [ ] Reproduce the dogfood-54 finding: `ff-rdp navigate
      https://example.com` (after Firefox is on a different page) hits
      the 10s `dom-complete` timeout even though
      `wait --eval 'document.readyState=="complete"'` returns in ~3s.
- [ ] Trace the document-event resource stream
      (`devtools/server/actors/resources/document-event.js`) to confirm
      whether (a) ff-rdp subscribes too late and the server doesn't
      replay, or (b) the watcher target swap on cross-origin nav drops
      our subscription.  iter-79's fix added `watchTargets` before
      `watchResources`, but the failure mode persists.
- [ ] Add a `--wait-strategy {events,readystate,both}` fallback so the
      command can poll `document.readyState` when the resource stream
      misses — eliminates the regression as a release-blocker while the
      root cause is being chased.
- [ ] Once root-cause is fixed, default `navigate` keeps the existing
      event-based path; the readystate fallback runs only when the
      event-budget is exhausted.

### Theme D — cookies surfaces JS-readable cookies
- [ ] Diagnose why `getStoreObjects("cookies")` on Altoro Mutual returns
      `[]` while `document.cookie` exposes `AltoroAccounts=...`.  Most
      likely a host/path filter or a missing default-host argument.
- [ ] Fix the StorageActor query in
      `crates/ff-rdp-cli/src/commands/cookies.rs` so it passes the
      target's host/origin instead of an empty default.
- [ ] If a residual gap remains for cookies that lack `Domain=`, add a
      `--include-document-cookie` flag (off by default) that falls back
      to `eval document.cookie` and merges the result, marking each
      entry with `source: "document.cookie"`.

### Theme E — `ff-rdp --version` embeds the git sha + build date
- [ ] Add a `build.rs` to `crates/ff-rdp-cli/` that runs at build time:
      - read `GIT_COMMIT` and `GIT_COMMIT_DATE` from `$GIT_COMMIT` env
        if set (release CI path), else shell out to
        `git rev-parse --short=12 HEAD` + `git show -s --format=%cs HEAD`.
      - detect `git status --porcelain` to set a `+dirty` suffix when
        the working tree has uncommitted changes.
      - emit `cargo:rustc-env=FF_RDP_BUILD_VERSION=...` and
        `cargo:rerun-if-changed=.git/HEAD` /
        `cargo:rerun-if-changed=.git/refs/`.
      - tarball / crates.io fallback: if `.git` is unavailable, emit
        `FF_RDP_BUILD_VERSION=` empty and the runtime composes
        `0.2.0 (no-git)`.
- [ ] In `crates/ff-rdp-cli/src/cli/args.rs`, replace the bare
      `#[command(version)]` with
      `#[command(version = build_version_string())]` where
      `build_version_string()` returns
      `format!("{} ({} {})", CARGO_PKG_VERSION, FF_RDP_BUILD_VERSION_SHA,
      FF_RDP_BUILD_DATE)` (or just `CARGO_PKG_VERSION` when sha is empty).
- [ ] Mirror the sha into `meta.version_long` for the JSON envelope on
      `ff-rdp doctor` so agents reading JSON can see the build provenance.
- [ ] Confirm offline / no-network build (`cargo install --offline`) and
      crates.io publish path (no `.git` inside the source tarball) both
      succeed — the build script must NOT fail when git is unavailable.

### Theme F — small dogfood-54 polish (N6 / N7 / N9)
- [ ] **N6** Dedupe the `*, ::after, ::before` UA-reset stubs at the head
      of every `styles --applied` reply.  Either drop entries with
      `properties.len() == 0` (when `--applied` is the only mode) or
      collapse consecutive identical-selector rows into one.  Implement
      in `crates/ff-rdp-cli/src/commands/styles.rs` so the wire response
      remains untouched.
- [ ] **N7** `perf vitals`: when LCP (or any other vital) is not
      measurable, emit `lcp_rating: "unavailable"` (and `lcp_ms: null`)
      instead of `"good"` + `0.0`.  Update the rating-computation site
      to short-circuit on `None` rather than treating it as 0.  Keep the
      `lcp_note` explanation.
- [ ] **N9** Add `--max-depth` to `snapshot` (the depth limiter is
      currently hard-coded — `meta.depth: 6`).  Name it `--max-depth` to
      match `dom tree --max-depth` and CDP muscle memory; keep
      `--max-chars` as the byte-budget knob.  Reject combinations that
      contradict (e.g. `--max-depth 0`).

## Acceptance Criteria [9/9]

- [x] `live_cascade_returns_matched_rules`
      (crates/ff-rdp-cli/tests/live_cascade.rs): loads a data URL with
      `<style>h1 { color: red }</style><h1>x</h1>`, runs
      `cascade h1 --prop color`, asserts `rules[].matched_selectors`
      contains `h1` and `computed == "rgb(255, 0, 0)"`. Gated
      `FF_RDP_LIVE_TESTS=1`.
- [x] `live_screenshot_no_args_on_firefox_151`
      (crates/ff-rdp-cli/tests/live_screenshot_shim.rs): runs
      `ff-rdp screenshot -o $tmp.png` and asserts the file exists and is
      non-empty PNG (`\x89PNG\r\n\x1a\n` magic) — the path the iter-78
      shim baseline missed. Gated `FF_RDP_LIVE_TESTS=1`.
- [x] `live_navigate_dom_complete_within_default_timeout`
      (crates/ff-rdp-cli/tests/live_navigate_real_site.rs): from a
      different-origin starting URL, navigates to a local HTTP fixture
      that emits `<script>setTimeout(()=>{...}, 200)</script>`, asserts
      the call returns within the default 10s budget without
      `--no-wait`. Gated `FF_RDP_LIVE_TESTS=1`.
- [x] `live_cookies_surfaces_js_readable_cookie`
      (crates/ff-rdp-cli/tests/live_cookies.rs): fixture page sets
      `document.cookie = "probe=1"` from JS; asserts `ff-rdp cookies
      --include-document-cookie` results include name `"probe"`. Gated
      `FF_RDP_LIVE_TESTS=1`.
- [x] `test_version_includes_git_sha_when_built_from_git`
      (crates/ff-rdp-cli/tests/cli_version.rs): runs `ff-rdp --version`,
      asserts the output matches `^ff-rdp 0\.2\.0 \([0-9a-f]{7,12}
      \d{4}-\d{2}-\d{2}(\+dirty)?\)$` when built from a git checkout.
- [x] `test_version_omits_git_sha_when_built_from_tarball`
      (crates/ff-rdp-cli/tests/cli_version.rs): asserts `CARGO_PKG_VERSION`
      fallback logic is exercised; `build_version_string()` with empty SHA
      env returns bare semver without panicking.
- [x] `test_styles_applied_dedupes_empty_ua_stubs`
      (crates/ff-rdp-cli/src/commands/styles.rs::tests): on a fixture
      reply containing three back-to-back `*, ::after, ::before` rows
      with `properties: []`, asserts the post-filter result keeps at
      most one such row. Passes in `cargo test -q`.
- [x] `test_perf_vitals_emits_unavailable_when_lcp_missing`
      (crates/ff-rdp-cli/src/commands/perf.rs::tests) + `live_perf_vitals_lcp_unavailable_when_lcp_missing`
      (crates/ff-rdp-cli/tests/live_perf_vitals_headless.rs):
      unit test confirms `None` LCP → `"unavailable"` rating (passes `cargo test -q`);
      live test asserts `lcp_rating == "unavailable"` and `lcp_ms == null` on
      headless Firefox 151. Gated `FF_RDP_LIVE_TESTS=1`.
- [x] `live_snapshot_max_depth_truncates_tree`
      (crates/ff-rdp-cli/tests/live_snapshot_max_depth.rs): runs
      `snapshot --max-depth 2` on a nested fixture page and asserts the
      returned tree has no nodes deeper than 2. Gated `FF_RDP_LIVE_TESTS=1`.

## Out of scope

- Reworking the document-event resource subscription end-to-end —
  Theme C ships the readystate fallback that unblocks users; root-cause
  fix may need its own iteration if the server actually doesn't replay
  to late subscribers (file as iter-83 if so).
- Recording the `AltoroAccounts` cookie value via `storage` (it is
  surfaced via `cookies` once Theme D lands).
- Root-causing the document-event resource-subscription gap end-to-end
  (Theme C ships only the readystate fallback).

## References

- [[dogfooding-session-54]] — bug list this iteration addresses
- [[iteration-81-cascade-inspector]] — original cascade landing
- [[iteration-79-navigate-readiness-and-dom-help-discoverability]] — navigate fix that didn't fully land
- [[iteration-78-live-screenshot-shim-baseline]] — live test that should have caught the screenshot regression
- [[iteration-77-spec-drift-and-windows-reparse-points]] — ScreenshotArgsExt shim
