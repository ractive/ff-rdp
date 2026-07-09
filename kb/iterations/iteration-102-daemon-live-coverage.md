---
title: "Iteration 102: daemon live coverage — cross-process follow + full error-shape parity"
type: iteration
date: 2026-07-09
status: planned
branch: iter-102/daemon-live-coverage
depends_on:
  - iteration-101-daemon-session-correctness
kb_refs:
  - kb/rdp/actors/watcher.md
first_call_sites:
  - primitive: >-
      live cross-process follow test asserting post-nav events reach a
      still-running --follow stream
    site: crates/ff-rdp-core/tests/live_daemon.rs
  - primitive: >-
      daemon-routed error-shape parity for bad-selector / eval-throw scenarios
    site: crates/ff-rdp-cli/tests/e2e/daemon_parity.rs
dogfood_path: |
  ff-rdp launch --headless
  ff-rdp navigate https://example.com
  ff-rdp console --follow &
  ff-rdp navigate https://en.wikipedia.org/wiki/Firefox
  # expected: follow stream keeps delivering events from the new page
tags: [iteration, daemon, watcher, parity, review-2026-07, carry-over]
---

# Iteration 102: daemon live coverage

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

### A. Live cross-process follow [0/1]
- [ ] `live_daemon_follow_survives_cross_process_nav` (`FF_RDP_LIVE_TESTS=1`):
      start a daemon, open a `--follow` stream, navigate cross-origin, assert
      post-nav events appear and no dead-target state leaks. Asserted
      post-condition: at least one event whose source is the post-nav page is
      delivered on the still-open stream.

### B. Daemon error-shape parity [0/1]
- [ ] Extend `e2e_error_shape_parity_daemon` with bad-selector and eval-throw
      scenarios run daemon vs `--no-daemon`; assert identical `error_type` and
      exit code. Asserted post-condition: for each scenario the two modes
      produce byte-identical `error_type` and exit code.

## Acceptance Criteria [0/2]

- [ ] live_daemon_follow_survives_cross_process_nav: post-navigation events
      appear in the still-running `--follow` stream (live Firefox).
- [ ] e2e_error_shape_parity_daemon_extended: bad-selector and eval-throw
      scenarios produce identical `error_type`/exit code daemon vs `--no-daemon`.

## References

- [[iteration-101-daemon-session-correctness]] — parent; deterministic coverage
  for the same behaviors already landed there.
- [[watcher]] — Iter-101 update section documents the target-switch semantics
  this live test exercises.
