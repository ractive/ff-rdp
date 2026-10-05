---
title: "Iteration 295: Session-feedback quick fixes — perf compare errors, --cold, navigate --wait-idle"
type: iteration
date: 2026-10-05
status: done
branch: iter-295/feedback-quick-fixes
depends_on: []
dogfood_path: >-
  Against a live headless Firefox: `perf compare --cold` on two real pages reports non-zero
  transfer sizes; `perf compare --timeout 1` fails naming URL and step; `navigate --wait-idle`
  on a lazy-loading page returns with images_complete true and no `wait --sleep-ms` needed.
tags: [iteration, perf, navigate, a11y, docs]
---

# Iteration 295: Session-feedback quick fixes

A user session on 2026-10-05 ([[backlog]] items tagged `feedback 2026-10-05`) hit four
problems that are each a bounded change: `perf compare` times out against live sites with
a bare `phase: recv` error that names no URL; `perf` on a cached page reports
`total_transfer_size: 0` with no way to bypass the cache; `navigate` has no idle wait, so
users fall back to `wait --sleep-ms`; and the parent `a11y --help` still shows `a11y summary`
returning an array. All of them edit `cli/args.rs`, so they land as one PR rather than four
merge-conflicting ones. The phone-width viewport work from the same feedback is a separate
plan (design needed) and stays out.

## Tasks

Work in this order. Item 5 lands last and is dropped back to the backlog if its live test is
not green after one honest attempt — the other four ship regardless.

- [x] **perf compare error context.** In `commands/perf_compare.rs`, replace the bare
      `.map_err(AppError::from)` on `navigate_to`, `evaluate_js_async` and `collect_page_perf`
      with errors that carry the URL and the step (`navigate`, `readystate`, `collect`), using
      the existing `AppError::with_timeout_hint` so an `RdpTimeout` says what it was waiting
      for. Say in `perf compare --help` that `--timeout` bounds each step, not the whole run.
- [x] **`a11y --help` output shape.** Fix the `With a11y summary:` line in the parent `a11y`
      help (`cli/args.rs` ~1094) to the real object shape
      `{"results": {"landmarks": [...], "headings": [...], "interactive": [...]}, "total": 1, ...}`,
      matching the subcommand's own help. Tick the `a11y summary` part of backlog item
      [[dogfooding-session-64]] #35 in its line (the rest of #35 stays open).
- [x] **`--cold` on `perf summary` / `perf vitals` / `perf audit` / `perf compare`.** Add
      `cache_disabled: Option<bool>` to `specs::target_configuration::request::Configuration`
      (wire name `cacheDisabled`, in Firefox's `SUPPORTED_OPTIONS`, so no spec-drift marker).
      When `--cold` is set, call `TargetConfigurationActor::update_configuration` with
      `cacheDisabled: true` on the command's connection before navigating; for the non-compare
      subcommands, which measure the current page, `--cold` performs a `reload` first with the
      flag set, otherwise it has nothing to measure cold. Help says "bypass the HTTP cache
      (`LOAD_BYPASS_CACHE`) for this command's loads" and names what it does not do: service
      worker caches, warm TCP/TLS/DNS state. Envelope `meta.cache: "bypassed"` when set.
- [x] **Doc tidy.** In `kb/rdp/actors/target-configuration.md`, move `cacheDisabled` into the
      current-use table and reword the historical `emulate` table into the past tense.
- [x] **`navigate --wait-idle [--idle-ms N]`.** Port `run_reload_wait_idle`'s network-quiet
      drain (`commands/nav_action.rs`) to navigate: after the document commits, keep draining
      `network-event` resources until none arrive for `--idle-ms` (default 500) or the
      `--timeout-ms` budget ends, then run one eval that returns
      `[...document.images].every(i => i.complete)` and poll it under the same budget. Add
      `results.idle = {"idle_at_ms": N, "requests_observed": M, "images_complete": bool}`.
      `--wait-idle` conflicts with `--no-wait` and with `--with-network` (which already
      settles on network drain). It composes with `--wait-for`, which runs after it.
- [x] Tests: mock-server test asserting the `cacheDisabled` field is sent and the echo check
      fails on a dropped key; unit test for the perf compare error text; `--help` snapshot
      update for `a11y`; live tests in a new `tests/live/live_295_feedback_quick_fixes.rs`
      for `--cold` (second load of the fixture page reports `total_transfer_size > 0`) and
      `navigate --wait-idle` (fixture page with a delayed image and a delayed `fetch`
      returns `images_complete: true` and `requests_observed >= 2`).
- [x] Tick the five `feedback 2026-10-05` backlog lines that this PR closes (the phone-width
      line stays open) in the same commit as the code.

## Acceptance Criteria [5/6]

- [x] `ff-rdp --timeout 1 perf compare https://example.com https://example.org` fails with an
      error whose message names the URL it was processing and the step (`navigate`,
      `readystate` or `collect`), never a bare `RdpTimeout { phase: "recv" }`.
- [x] `ff-rdp a11y --help` describes `a11y summary` output as an object with `landmarks`,
      `headings` and `interactive` keys, identical in shape to what `ff-rdp a11y summary`
      prints against the test fixture page.
- [x] After `ff-rdp navigate <fixture>` twice, `ff-rdp perf summary --cold` reports
      `total_transfer_size > 0` and `meta.cache: "bypassed"`, while `ff-rdp perf summary`
      without the flag on the same page reports `total_transfer_size: 0`.
- [ ] `ff-rdp perf compare --cold <a> <b>` reports `total_transfer_size > 0` for both URLs
      against the mock-server fixtures, and the next `ff-rdp perf summary` on the same tab
      (new connection) is served from cache again, proving the override died with the
      connection.
      — Not ticked: the premise is wrong. `perf summary` reports the last load, so straight
      after `--cold` it still shows the cold bytes on a new connection. The property itself
      was proven with a different sequence: `perf compare --cold a b` → 2935 bytes for both,
      then plain `navigate b` + `perf summary` → 0 (PR #320).
- [x] `ff-rdp navigate <fixture-with-delayed-image-and-fetch> --wait-idle` returns only after
      both the delayed request and the delayed image have landed, with
      `results.idle.images_complete: true` and `requests_observed >= 2`; the same command with
      `--idle-ms 100 --timeout-ms 200` on a page that polls every 50 ms fails with a timeout
      error naming `--wait-idle`, not a success envelope.
- [x] `ff-rdp navigate <url> --wait-idle --no-wait` and `--wait-idle --with-network` are
      rejected by clap with a conflict message before any connection is opened.

## Outcome (2026-10-05)

Landed as two parallel PRs with disjoint file ownership, not one: #320 (perf, a11y help,
doc; merge 80921e67) and #321 (`navigate --wait-idle`; merge 333b3919). Deviations from the
plan, each with evidence in the PR body:

- **`perf compare` timeouts were not slow sites.** After `navigateTo` the command checked
  `readyState` on the old page (already `complete`) and sent the collect eval to that page's
  dead console actor, so no reply ever came — it failed at the default timeout on a local
  fixture. Each URL now uses the same commit wait as `navigate`. A fourth step label
  `connect` was added since `--timeout 1` fails before any URL.
- **`--idle-ms` has a 200 ms floor.** Firefox batches network events every 100 ms
  (`RESOURCES_THROTTLING_DELAY`), so a 100 ms quiet window reported idle on a page fetching
  every 50 ms. The AC's failing case holds only because of the floor.
- **Off-screen `loading="lazy"` images are not waited for.** The plan assumed Firefox reports
  a never-requested lazy image as `complete`; it reports `false` (76 of 86 on a Wikipedia
  article). They are skipped and counted in `results.idle.lazy_images_deferred`; lazy images
  inside the viewport are still waited for. This is the phone-width plan's territory.
- `requests_observed` counts from navigation start, like reload; the live test asserts ≥ 3.
- Test fixture server now sends `Cache-Control: no-store` only when a route sets none, so a
  page can be cached at all (`tests/common/mod.rs`).
- Live tests live in `live_295_perf_cold.rs` and `live_295_navigate_wait_idle.rs`, not one file.
- No mock-RDP test of the `--wait-idle` flow; coverage is the live fixture test, clap e2e and
  unit tests. Follow-ups filed in [[backlog]]: `reload --wait-idle` lacks the floor; a request
  silent longer than the quiet window is not waited for; plain `navigate` on MDN sometimes
  reports an `mdnplay.dev` iframe URL as `committed_url`.

## Design notes

- **Why `cacheDisabled` and not a fresh profile.** `cacheDisabled` sets the top browsing
  context's `defaultLoadFlags` to `LOAD_BYPASS_CACHE`; the platform propagates it to child
  frames, and the actor restores `LOAD_NORMAL` on connection close
  (`devtools/server/actors/target-configuration.js`, `_setCacheDisabled`,
  `_restoreParentProcessConfiguration`). That gives the per-command lifetime ff-rdp already
  relies on for `--color-scheme` and `--user-agent`, costs one request, and needs no profile
  juggling. It is an HTTP-cache bypass, not a first-visit simulation: the flag is named
  `--cold` for discoverability but the help must say "bypass the HTTP cache". A
  fresh-profile-per-URL mode, if ever wanted, belongs with the viewport plan.
- **Why the error-context fix is separate from a timeout change.** The user's `--timeout
  60000` still failed because the value bounds each RDP read and each URL's readystate poll,
  not the run. That is the documented contract elsewhere in the CLI; the fix is to say so in
  `perf compare --help` and make the error name the step, not to add a second timeout flag.
- **`--wait-idle` image check.** `img.complete` is true for broken and lazy-not-yet-requested
  images as well as loaded ones; it is the cheapest signal that nothing is still decoding.
  Do not call `decode()` on every image: it forces decodes the page did not ask for and
  skews the very measurements the user wanted. If `loading="lazy"` images outside the
  viewport are the user's problem, that is the viewport plan's job.
- **Flag table for navigate.** `--wait-idle` runs after the readiness wait chosen by `--wait`
  / `--wait-strategy` and before `--wait-for`. The help for `--wait-idle` states this
  order in one sentence; no change to how the existing flags interact.

## Out of scope

- Phone-width `--viewport` / `--device` presets, DPR and touch emulation, and a lazy-image
  wait in the one-shot `screenshot --window-size` path (backlog, needs a plan).
- A `--quiet` flag on `navigate`: plain `navigate` is already small; `--jq` and `--fields`
  trim the rest.
- An overall (whole-run) timeout for `perf compare`.
- The rest of [[dogfooding-session-64]] #35 (`styles`, `dom --count`, `network`,
  `a11y contrast`, `snapshot --query` help text).

## References

- [[backlog]] — the five `feedback 2026-10-05` lines
- [[target-configuration]] — `cacheDisabled`, lifetime, echo check
- [[iteration-103-target-configuration-cli]] — the removed `emulate --cache` this revives per command
- [[iteration-133-viewport-emulation]] — why viewport work is a separate plan
- `commands/nav_action.rs::run_reload_wait_idle` — the drain loop to port
