---
title: "Iteration 111: daemon live coverage — cross-process follow + full error-shape parity"
type: iteration
date: 2026-07-09
status: completed
branch: iter-111/daemon-live-coverage
depends_on:
  - iteration-101-daemon-session-correctness
kb_refs:
  - kb/rdp/actors/watcher.md
first_call_sites:
  - primitive: >-
      live cross-process follow test asserting post-nav events reach a still-running
      --follow stream
    site: crates/ff-rdp-cli/tests/live/live_111_daemon_follow_cross_process.rs
  - primitive: daemon-routed error-shape parity for bad-selector / eval-throw scenarios
    site: crates/ff-rdp-cli/tests/e2e/daemon_parity.rs
dogfood_path: |
  ff-rdp launch --headless
  ff-rdp navigate https://example.com
  ff-rdp console --follow &
  ff-rdp navigate https://en.wikipedia.org/wiki/Firefox
  # expected: follow stream keeps delivering events from the new page
tags:
  - iteration
  - daemon
  - watcher
  - parity
  - review-2026-07
  - carry-over
---

# Iteration 111: daemon live coverage

## Execution policies (standing, per James)

This iteration's deliverable IS live daemon tests — run the specific live
tests it adds (filtered), not the full live suite (next batch sweep covers
integration). Scoped testing: affected tests only during development; one
full `cargo test --workspace -q` in the final pre-PR gates; the review agent
does not re-run the full workspace suite. CI-wait: merge on required lanes
only; `test (windows-latest)` is expected GREEN since PR #148 — any windows
failure is real and blocks. `live-tests` stays advisory.


Carry-over from [[iteration-101-daemon-session-correctness]]. Iter-101 landed the
daemon session-correctness fixes (target-switch buffer purge, `daemon_busy`
concurrency, per-type buffering, `--since` parity, atomic registry) with
deterministic unit + e2e coverage. Two coverage items were deferred because they
require a live Firefox harness or live-like daemon-proxy choreography that would
have expanded iter-101's blast radius.

## Themes

- **A — Live cross-process follow.** Assert against real Firefox that a
  `console --follow` (and `network --follow`) stream keeps delivering events
  after an example.com → wikipedia.org cross-process navigation, exercising the
  iter-101 top-level-switch purge path end to end.
- **B — Full error-shape parity through the daemon.** Extend
  `e2e_error_shape_parity_daemon` beyond connection-refused to the bad-selector
  and eval-throw scenarios, routed through the daemon proxy (needs mock
  choreography that forwards the failing actor response and lets the daemon
  relay the error frame).

## Tasks

### A. Live cross-process follow [1/1]
- [x] `live_daemon_follow_survives_cross_process_nav` (`FF_RDP_LIVE_TESTS=1`):
      start a daemon, open a `--follow` stream, navigate cross-origin, assert
      post-nav events appear and no dead-target state leaks. Asserted
      post-condition: at least one event whose source is the post-nav page is
      delivered on the still-open stream.

### B. Daemon error-shape parity [1/1]
- [x] Extend `e2e_error_shape_parity_daemon` with bad-selector and eval-throw
      scenarios run daemon vs `--no-daemon`; assert identical `error_type` and
      exit code. Asserted post-condition: for each scenario the two modes
      produce byte-identical `error_type` and exit code.

## Implementation notes

- **Theme A — stream choice.** `console --follow` proved unusable as the live
  signal: on the tested Firefox, ordinary `console.log` (and even page errors)
  are delivered as direct console-actor pushes and are **not** routed through
  the watcher `console-message` resource stream (confirms the iter-71 Theme C
  finding), so a daemon follow never sees them. `network --follow` is the
  reliable stream — its `navigation` event carries the post-nav page URL, an
  event unambiguously *sourced from the post-nav page*, matching the AC.
- **Theme A — navigation channel.** The follow stream holds the daemon's single
  RPC-writer slot for its lifetime (iter-101 Theme B), so a second *daemon
  -routed* command is refused with `daemon_busy`. The driving navigation is
  therefore issued with `--no-daemon` (direct to Firefox); the daemon's own
  watcher still observes the top-level switch and forwards the new page's
  navigation event to the follow stream — the real dogfooding flow.
- **Theme A — cross-process phase.** Verified end to end against real Firefox:
  the network-gated phase drives example.com → wikipedia.org (distinct eTLD+1
  → Fission process switch) and asserts the stream both stays alive and
  delivers a wikipedia-sourced navigation event (`saw_wiki_event=true`,
  206 lines collected).
- **Theme B — both scenarios collapse to exit 1 / absent `error_type`.** A
  bad-selector (`click --no-wait button.missing`) and an eval-throw
  (`eval "throw …"`) both fail inside `evaluateJSAsync`; the CLI maps the
  `evaluationResult.exception` to `AppError::Exit(1)`, which prints the message
  to stderr and emits **no** JSON error envelope on stdout. Parity therefore
  means identical exit code 1 and identical (absent) `error_type` whether
  routed through the daemon proxy or forced direct — the load-bearing behaviour
  is that the daemon forwards the `evaluateJSAsync` request and relays its
  `evaluationResult` followup byte-for-byte.

## Acceptance Criteria [2/2]

- [x] live_daemon_follow_survives_cross_process_nav: post-navigation events
      appear in the still-running `--follow` stream (live Firefox). Test:
      `live_daemon_follow_survives_cross_process_nav` in
      `crates/ff-rdp-cli/tests/live/live_111_daemon_follow_cross_process.rs`;
      asserted post-condition — a follow-stream `navigation` event whose `url`
      contains the post-nav sentinel is delivered after the top-level target
      switch (verified live: PASS, 6 lines core / 206 lines cross-process).
- [x] `e2e_error_shape_parity_daemon_extended`: bad-selector and eval-throw
      scenarios produce identical `error_type`/exit code daemon vs `--no-daemon`.
      Test: `e2e_error_shape_parity_daemon_extended` in
      `crates/ff-rdp-cli/tests/e2e/daemon_parity.rs`; asserted post-condition —
      for each scenario `(exit_code, error_type)` is byte-identical across both
      modes (both exit 1, absent `error_type`).

## References

- [[iteration-101-daemon-session-correctness]] — parent; deterministic coverage
  for the same behaviors already landed there.
- [[watcher]] — Iter-101 update section documents the target-switch semantics
  this live test exercises.
