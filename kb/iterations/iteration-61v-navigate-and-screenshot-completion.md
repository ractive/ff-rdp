---
title: "Iteration 61v: navigate document-event gating + screenshot fallback cleanup + bus throttle zero"
type: iteration
date: 2026-05-23
status: in-progress
branch: iter-61v/navigate-screenshot-completion
depends_on:
  - iteration-61r-multi-actor-commands
  - iteration-61t-wire-the-foundations
tags:
  - iteration
  - navigate
  - screenshot
  - bus
  - stability-roadmap
---

# Iteration 61v: navigate + screenshot completion

The iter-61r plan committed to (a) `navigate` gating on `document-event` resources rather than polling `window.location.href`, and (b) deleting the embedded-JS canvas fallback from `screenshot.rs` now that the two-step `drawSnapshot` flow is wired. Both were deferred. This iteration completes them and drops the stale 100ms ResourceCommand throttle that Firefox itself already moved away from (Bug 1914386).

These are the user-visible behavioral fixes the stability roadmap promised but didn't deliver.

## Themes

- **A — `navigate` subscribes to `document-event`.** Replace `wait_for_commit` + `window.location.href` polling with a bus subscription to `document-event` resources. Detect commit on `dom-loading`, success on `dom-complete`, neterror on the `about:neterror` form. Closes the `navigate-race-timeout` and `navigate-success-on-bad-dns` open gaps.
- **B — Drop screenshot's JS canvas fallback.** Delete the `format!`-embedded JS program at `commands/screenshot.rs:24-66, 696-733`. The two-step `getRoot → screenshotActor.capture(rect, ratio, bg, fullpage)` path lives at `actors/screenshot.rs:49-96` and must be the only path. Land the deferred DPR=2 live test.
- **C — Throttle → 0.** `ResourceCommand`'s 100ms throttle is contrary to what Firefox now does. Remove the timer; keep array-batching (multiple resources per event tick).

## Tasks

### A. navigate via document-event
- [x] In `commands/navigate.rs`, before sending the `tabNavigate` packet, call `ResourceCommand::subscribe(&[ResourceType::DocumentEvent], filter_by_target, sink)`.
- [x] State machine on the sink: `dom-loading` → commit recorded; `dom-interactive` → optional `--wait interactive` gate; `dom-complete` → success.
- [x] Detect `about:neterror` by either the `name` field on the document-event form or the `url` prefix; map to `RdpError::Navigation{cause}` with a typed cause enum (DnsFail, ConnReset, Timeout, CertError, Unknown).
- [x] Honor existing `--timeout` and `--wait {load|interactive|complete}` flags; `complete` is now the strict default.
- [ ] Delete the `window.location.href` polling helper.
- [x] Update `tests/navigate_*.rs` to drive the mock server's new `inject_document_event` capability (added in iter-61o).

### B. Screenshot fallback cleanup
- [x] Delete `commands/screenshot.rs:24-66` (`SCREENSHOT_JS_PROGRAM` constant) and lines 696-733 (chrome-context JS fallback strategy).
- [x] Remove any `EvalStrategy::ChromeJs`/`EvalStrategy::ContentJs` variants from the screenshot strategy enum; the only strategies left are `SnapshotActor` and `SnapshotActorFullPage`.
- [x] File should drop to < 500 LOC. Refactor any remaining helpers shared with eval into `core/src/screenshot/`.
- [x] Add `tests/live_screenshot_full_page_dpr2.rs`: launch headless FF, set window.devicePixelRatio = 2 via `--remote-debugging-port` prefs, navigate to a 5000px-tall page, run `screenshot --full-page --output /tmp/x.png`, assert `width = viewport*2` and `height ≥ 5000*2`.

### C. Bus throttle = 0
- [x] In `core/src/resources/command.rs`, change the throttle constant from 100ms to 0 (or delete the timer field).
- [x] Keep array-batching: a single transport event delivering N resources still fans out as one bus dispatch with `Vec<Resource>`.
- [x] Add a comment citing FF Bug 1914386 and `devtools/shared/commands/resource/resource-command.js:73-79`.
- [x] Bench micro-test: `bench_bus_dispatch_latency` — single event in, subscriber wake-up < 1ms.

### D. Carryover from iter-61u (skeleton live tests)
- [x] Flesh out `crates/ff-rdp-core/tests/live_61u.rs::live_network_set_cookie_longstring`: load a page that sets `Set-Cookie` > 10 000 chars, capture the network event, assert the deserialized header value is the full string (longstring auto-fetched), not an actor reference.
- [x] Flesh out `crates/ff-rdp-core/tests/live_61u.rs::live_cache_disable_via_target_config`: navigate to a `Cache-Control: max-age=3600` resource, call `TargetConfigurationFront::set_cache_disabled(true)`, navigate again, assert the second response is a fresh fetch (not 304/disk cache).
- [x] Once both pass, update `kb/iterations/iteration-61u-spec-and-front-correctness.md` AC header from `[6/8]` to `[8/8]` and tick the two ACs.

## Acceptance Criteria [3/8]

- [ ] `live_navigate_dom_complete`: navigate to a page with deferred scripts; `--wait complete` returns only after `dom-complete`, not on first commit.
- [ ] `live_navigate_neterror_dns_fail`: `ff-rdp navigate https://no.such.host.invalid.example` returns exit code matching `RdpError::Navigation{cause: DnsFail}`, not exit code 0.
- [ ] `live_navigate_neterror_cert`: `https://self-signed.badssl.com` (or local cert-bad fixture) returns `CertError`.
- [ ] `live_screenshot_full_page` (re-verified): 1280×viewport page captured at DPR=1.
- [ ] `live_screenshot_full_page_dpr2`: same page at DPR=2 produces PNG with `height ≥ scrollHeight × 2`.
- [x] `commands/screenshot.rs` LOC < 500; `grep -c 'toDataURL\|drawWindow'` returns 0.
- [x] `bench_bus_dispatch_latency` p99 < 1ms.
- [x] `cargo fmt && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace -q` clean.

## Design notes

- The `document-event` subscription should be per-navigate-call, not a long-lived watcher — subscribe → wait → unsubscribe on completion or timeout, releasing the bus subscription handle.
- For neterror detection, prefer the form fields (`name`, `errorClass`) over URL string matching where Firefox provides them; fall back to `about:neterror?e=...` parsing only if needed.
- Throttle removal is safe because every `Vec<Resource>` bus dispatch is already a single tokio task wake — there was no real coalescing benefit to the 100ms timer once batching landed in iter-61q.

## References

- [[document-event]] (kb)
- [[take-screenshot]] (kb)
- [[ff-rdp-wins]] §1, §3
- [[open-gaps]] §navigate-race-timeout, §navigate-success-on-bad-dns, §full-page-screenshot
- `devtools/shared/commands/resource/resource-command.js:73-79` (Bug 1914386)
- [[stability-roadmap]]
