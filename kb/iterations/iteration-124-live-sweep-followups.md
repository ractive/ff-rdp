---
title: "Iteration 124: post-merge live-sweep follow-ups — navigate probe actor goes stale, cookies test pinned an implementation detail"
type: iteration
date: 2026-07-19
status: completed
branch: iter-124/live-sweep-followups
depends_on:
- "[[iteration-121-cookies-storage-actor-enumeration]]"
- "[[iteration-122-navigate-dom-complete-ff152]]"
firefox_refs: []
kb_refs:
- kb/rdp/actors/watcher.md
first_call_sites: []
dogfood_path: |
  # Default navigate must actually use the iter-122 readystate fast path — the probe
  # must survive the document commit (console actor refresh), not silently noSuchActor:
  /usr/bin/time -p ff-rdp --port <p> navigate https://example.com
  # expected: wall time < timeout/2 (~4s with default 10s timeout); was ~5.6-5.9s when
  #           every probe attempt failed against the pre-commit console actor
  # Cookies: a JS-readable probe cookie is served by the authoritative StorageActor
  # (real flags, no source field) OR the document.cookie fallback (source tagged):
  ff-rdp --port <p> eval 'document.cookie="probe=1"'
  ff-rdp --port <p> cookies --jq '.results[] | select(.name=="probe")'
  # expected: either isHttpOnly present (StorageActor) or source=="document.cookie"
tags:
- iteration
- navigate
- cookies
- live-tests
- firefox-152
- dogfood-61
---

# Iteration 124: post-merge live-sweep follow-ups

After merging iterations 121–123, a full live sweep (`FF_RDP_LIVE_TESTS=1
FF_RDP_LIVE_NETWORK_TESTS=1 cargo test-live`) on main found 2 red tests, both reproduced
serially on a clean single Firefox instance:

1. **`live_navigate_default_fast` (product bug).** `ReadyStateProbe.console_actor` was
   captured from `ctx.target.console_actor` *before* `navigateTo` was dispatched
   (`navigate.rs` `run_core`). Firefox tears down the old docshell/process when the new
   document commits, invalidating that actor ID — instrumentation showed 18/18 probe
   attempts failing with `ActorError{kind: UnknownActor}`. The iter-122 Theme A fast
   path was silently defeated and default navigate fell through to the ~5.6–5.9s
   events-budget burn it was built to eliminate.
2. **`live_cookies_surfaces_js_readable_cookie` (stale test expectation).** The test
   asserted `source == "document.cookie"` for a JS-set probe cookie. Pre-iter-121 that
   was always true because StorageActor enumeration was dead; post-iter-121 the
   StorageActor correctly enumerates the cookie (no `source` field, real
   `isHttpOnly`/`isSecure`/`sameSite`) and the `cookies.rs:73-87` merge correctly drops
   the weaker `document.cookie` duplicate. The merge logic is right; the test pinned an
   implementation detail iter-121 legitimately changed.

## Themes

- **A — Probe survives the document commit.** Re-resolve the probe's console actor via
  `TabActor::get_target` once `dom-loading` commits (plus a lazy fallback before the
  first probe attempt on quiet event streams), mirroring `ConnectedTab::refresh_target`.
- **B — Pin the cookies output contract, not the source.** The live test accepts either
  valid shape: StorageActor-sourced (no `source`, flags present) or fallback-sourced
  (`source: "document.cookie"`).

## Tasks

- [x] Add `refresh_probe_console_actor()` in `navigate.rs`; make `ReadyStateProbe` own a
      mutable `console_actor: ActorId` + `tab_actor`; refresh on `dom-loading` commit
      with lazy fallback; thread `Option<&mut ReadyStateProbe>` through
      `wait_for_doc_complete`. Update the 3 mock-server unit tests with an
      `answer_get_target` helper.
- [x] Rewrite the `live_cookies_surfaces_js_readable_cookie` assertion to pin the
      StorageActor-or-fallback contract; document the iter-124 context in doc-comments.
- [x] Review fix: `refresh_probe_console_actor` now returns `bool`; both call sites in
      `wait_for_doc_complete` only latch `probe_refreshed = true` on `Ok` so a transient
      `getTarget` failure retries on the next probe tick instead of permanently
      stranding the probe on the stale actor. Gate the `dom-loading` refresh to
      `wait_level == WaitLevel::Complete || url.is_empty()` so it isn't paid for
      Loading/Interactive waits that resolve straight from the event's own URL.
      New unit tests: `unit_navigate_probe_refresh_retries_after_transient_error`
      (recovers on the second `getTarget` attempt) and
      `unit_navigate_probe_refresh_persistent_error_falls_back_to_timeout` (falls
      through cleanly to `AppError::Timeout`, no panic, no hang).

## Acceptance Criteria [3/3]

<!-- Each AC names a live test + asserted post-condition, per CLAUDE.md convention. -->

- [x] live_navigate_default_fast: default navigate to a static page returns in
      wall-clock `< timeout/2` with the readystate probe succeeding post-commit
      (verified serially post-fix: 2 passed in 4.07s; was 5.6–5.9s).
- [x] live_cookies_surfaces_js_readable_cookie: JS-readable probe cookie is surfaced
      via the StorageActor-or-fallback contract (verified: PASS,
      storage_actor_sourced=true).
- [x] `cargo fmt && cargo clippy --workspace --all-targets -- -D warnings && cargo test
      --workspace -q` clean, plus the 14 AC-relevant live tests across the
      live_cookies / live_navigate_default_fast / live_123 filters all green.

## Design notes

- The actor-refresh mirrors the existing `ConnectedTab::refresh_target` pattern rather
  than inventing a new resolution path; no new pub items (check-dead-primitives clean).
- The cookies fix is test-only: the iter-83 merge semantics (`cookies.rs:73-87`,
  StorageActor entry wins, fallback only fills gaps) are correct and unchanged.

## Out of scope

- The pre-existing xtask test-isolation gap (`live_check_dogfood_script_*` nested
  `cargo run` deadlocking against a cold outer build) — pre-existing, not touched by
  this diff.
- Remaining dogfood-61 moderate bugs — filed as
  [[iteration-125-perf-audit-lcp-unavailable]],
  [[iteration-126-network-json-shape-consistency]],
  [[iteration-127-a11y-contrast-fail-only-total]].

## References

- [[dogfooding-session-61]]
- [[iteration-121-cookies-storage-actor-enumeration]]
- [[iteration-122-navigate-dom-complete-ff152]]
