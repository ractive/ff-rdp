---
branch: iter-146/live-suite-reliability
date: 2026-08-11
depends_on: [kb/iterations/iteration-142-session-hygiene.md]
dogfood_path: |
  FF_RDP_LIVE_TESTS=1 cargo test -p ff-rdp-cli -- --include-ignored --test-threads=1
  pgrep -fl 'firefox.*ff-rdp-profile'
  # → after the suite exits, zero ff-rdp-owned Firefox processes may remain and
  #   zero ff-rdp-profile-* directories may be left pinned
first_call_sites: []
status: planned
title: "Iteration 146: live suite reliability — leaked Firefox, order-dependent tests, daemon-parity flake"
type: iteration
tags: [iteration]
---

# Iteration 146: live suite reliability — leaked Firefox, order-dependent tests, daemon-parity flake

Found during the post-batch live sweep on main after [[iteration-142-session-hygiene]] merged
(2026-08-11). No dogfooding session behind this one; the evidence is inline.

## Why this matters more than "tests are flaky"

The live suite is the only gate that exercises the **default daemon path** end to end. iter-129
shipped a feature that did not work at all because its tests passed `--no-daemon`; iter-137 added
`crates/ff-rdp-cli/tests/no_daemon_live_test_guard.rs` to stop that recurring. A live suite that
leaks state between tests and fails intermittently is a weak guard — and worse, it *trains
readers to ignore red*, which is exactly how a real regression slips through.

## Themes

### Theme A — the live suite leaks Firefox processes

After a full sequential run of `-p ff-rdp-cli -- --include-ignored --test-threads=1`, four
headless Firefox instances were still alive, each pinning an `ff-rdp-profile-*` directory:

```
9714  firefox -no-remote --start-debugger-server 64372 --headless --profile .../ff-rdp-profile-EWbkFJ5GPgv7d2Ny
20719 firefox -no-remote --start-debugger-server 52497 --headless --profile .../ff-rdp-profile-r0sbJ51SbJoITjsN
21458 firefox -no-remote --start-debugger-server 52893 --headless --profile .../ff-rdp-profile-Cdi88KtbcjUWBqBg
30331 firefox -no-remote --start-debugger-server 56504 --headless --profile .../ff-rdp-profile-jxW3kTKXvGcvMqHA
```

Note this is a **live-leak**, distinct from what iteration-142 fixed. iter-142 made dead-owner
profiles reclaimable on the next launch; these owners are alive, so the GC correctly leaves them
alone. The defect is upstream: the test harness (`LiveFirefox` / `firefox_with_daemon`) is not
reliably tearing down every instance it starts — most likely on the paths that spawn a daemon,
since a daemon deliberately outlives the CLI invocation that started it.

### Theme B — `live_96_profile_cleanup` is order-dependent, not broken

`live_96_profile_cleanup::live_profiles_prune_removes_all_when_no_firefox_running` failed the
sweep with `expected zero ff-rdp-profile-* dirs after prune --all, found 1`. It passes in
isolation. The cause is Theme A: `prune --all` **correctly** refused to delete a profile owned by
a live Firefox left over from an earlier test. The product behaviour is right; the test's
precondition ("no firefox running") is silently false when it runs late in the suite.

Fixing Theme A may fix this outright. If it does not, the test must assert its own precondition
and fail with a diagnostic naming the offending PIDs rather than an opaque `left: 1 / right: 0`.

### Theme C — `live_137_daemon_mode_parity` is intermittently red

Two tests in the suite fail intermittently:

- `live_137_frame_targets_via_daemon` (`:179`)
- `live_137_click_cross_origin_via_daemon` (`:237`)

Both fail the same way — `wait_for_live_targets` times out after 15 s and `daemon status` reports
`target_count: 0`, `live_target_count: 0`, `connections: 0`, `uptime_seconds: 16`. Both passed in
one full sweep and failed in the next on **identical product code**.

**This is not a regression from iterations 138–142.** Verified by bisect: both reproduce at
`f42b12b` (the iter-137 merge, before any of 138–142). Network is not the cause either —
`https://example.com`, which the `CROSS_ORIGIN_FIXTURE` iframe loads, responded in ~20 ms during
the failing run.

The `connections: 0` with `uptime_seconds: 16` reading suggests the daemon that served the
`navigate` is not the daemon answering the `status` — i.e. a daemon restart mid-test, which the
test's own comment anticipates ("a daemon that restarted mid-test re-establishes it on a
background thread") without making the test robust to it. **Root-cause this before changing
anything**: a daemon restarting mid-test may itself be the real defect, in which case widening
the timeout would paper over a genuine product bug.

## Acceptance Criteria [0/5]

- [ ] live_146_no_orphan_firefox_after_suite: after a full sequential live run, zero processes
      matching `firefox.*ff-rdp-profile` remain and zero `ff-rdp-profile-*` directories are left
      pinned by a live owner
- [ ] live_146_harness_teardown_kills_daemon_spawned_firefox: a test that starts Firefox via
      `firefox_with_daemon` leaves no surviving process once its guard drops, including when the
      test body panics
- [ ] live_96_profile_cleanup_precondition_asserted: the prune test asserts "no ff-rdp Firefox
      running" as an explicit precondition and, on violation, names the offending PIDs in the
      failure message instead of reporting a bare count mismatch
- [ ] live_146_daemon_parity_stable_repeat: `live_137_frame_targets_via_daemon` and
      `live_137_click_cross_origin_via_daemon` each pass 5 consecutive runs, with the root cause
      of the restart documented in this plan (not merely a raised timeout)
- [ ] live_146_daemon_restart_observable: if a daemon restart mid-test is confirmed as the
      mechanism, `daemon status` exposes enough signal (e.g. a restart counter or start
      timestamp) for a test to distinguish "subscription not yet live" from "daemon never
      subscribed"

## Notes

- Themes are independent and A is the highest value: fixing the leak may resolve B for free and
  removes a whole class of order-dependent failure.
- Theme C explicitly forbids the cheap fix. Raising the 15 s deadline without explaining the
  restart would convert a visible flake into an invisible one.
- A raised timeout or a retry loop is only acceptable with the mechanism documented and a
  product-side fix filed if one is warranted.
- Every live test added here must exercise the **default daemon path**. Do not add entries to the
  shrink-only grandfather list in `crates/ff-rdp-cli/tests/no_daemon_live_test_guard.rs`.

### Scope check after iter-145 (2026-08-12)

[[iteration-145-error-envelope-completeness]] merged and added a sixth live test file
(`live_145_error_envelope_completeness.rs`) with its own local `firefox_with_daemon` wrapper —
the same thin-duplicate pattern iterations 137–141 already established. The teardown logic itself
(`LiveFirefox`, its `Drop` impl, `with_daemon`) lives once in `tests/common/mod.rs`, so Theme A's
fix there covers this new file for free — no separate propagation needed. Two things worth
carrying into this iteration's execution, not a scope change to the Acceptance Criteria:

- Theme A's "full sequential run" dogfood check now also exercises
  `live_145_click_js_exception_envelope`, `live_145_click_frame_scan_js_exception_envelope`, and
  `live_145_click_element_not_found_unchanged` — three more data points for confirming the leak
  fix, all using the default daemon path per this plan's own rule.
- The six duplicated `firefox_with_daemon(test: &str) -> Option<LiveFirefox>` copies (137, 138,
  139, 140, 141, 145) are a candidate for consolidating into `tests/common/mod.rs` once Theme A's
  root cause is fixed there — not required for this iteration's ACs, but worth a follow-up note if
  Theme A's fix ends up touching `with_daemon()`'s signature or return type.
- Separately (unrelated code area, **not** in this iteration's scope): the iter-145 review pass
  hit a one-off flaky failure in `ff-rdp-core`'s unit suite —
  `specs::types::tests::resolve_slot_longstring_grip_fetches_full_value` — under
  `cargo test --workspace`'s default parallelism. Root cause: it depends on
  `transport::max_frame_bytes()`'s global `MAX_FRAME_BYTES_CELL` but doesn't take the
  `FRAME_CAP_LOCK` mutex that `transport.rs`'s own cap-mutating tests (e.g.
  `max_frame_mb_knob_works`) use to serialize access, so it can race against them and observe a
  stale 1024-byte cap. Reproduced isolated (5/5 pass) and full-workspace (5/5 pass after the first
  observed failure) — genuinely rare, and CI's three platforms were green on this PR. This is a
  `ff-rdp-core` unit-test isolation bug, a different class from this plan's live-Firefox-process
  themes; flagged here only so it isn't lost, not folded into Theme A/B/C.
