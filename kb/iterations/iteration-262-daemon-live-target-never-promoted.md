---
title: "Iteration 262: the daemon counts a frame target and never promotes it to live"
type: iteration
date: 2026-09-07
status: planned
branch: iter-262/daemon-live-target-never-promoted
depends_on:
  - 246
first_call_sites:
  - primitive: (to be decided — a repair path on the frame-target subscription)
    site: crates/ff-rdp-cli/src/daemon/server.rs
dogfood_path: |
  # Reproduce under sweep contention — it does not appear in isolation.
  firefox -no-remote --start-debugger-server 6000 --headless   # raw browser, NOT ff-rdp launch
  FF_RDP_LIVE_TESTS=1 FF_RDP_LIVE_NETWORK_TESTS=1 cargo run -p xtask -- live-sweep 2>&1 | tee /tmp/sweep.log
  grep "LIVE_TARGET_WAIT" /tmp/sweep.log
  # expected TODAY: some runs show `reached=false elapsed_ms≈15000` with the daemon
  # reporting target_count=1, live_target_count=0 and a healthy dispatcher.
  # expected AFTER: every LIVE_TARGET_WAIT line reports reached=true, or a
  # reached=false line is accompanied by live_target_count>0 having been observed
  # and lost — i.e. the bookkeeping is repaired rather than latched.
  #
  # Serial control, expected green today and after:
  FF_RDP_LIVE_TESTS=1 FF_RDP_LIVE_NETWORK_TESTS=1 cargo test -p ff-rdp-cli --test live \
    live_137_consent_accept_via_daemon -- --include-ignored --test-threads=1
tags: [iteration, daemon, frame-targets, race, live-tests, carry-over]
iteration_252_followup_observations: "2026-09-09 final dual-gate sweep: live_137_consent_accept_via_daemon failed AFTER LIVE_TARGET_WAIT reached=true in 116 ms, with Sourcepoint detected_not_actioned/consent_not_actioned. The exact isolated test failed again after readiness in 113 ms. An untouched origin/main checkout at 05634cbd42f02995f50e06c35d68b868cd39d0ba independently reproduced the same consent error, establishing this predates iteration 252. This is DISTINCT from the original target_count>0/live_target_count=0 failure; do not attribute it to promotion without evidence. Folded here because this plan already requires three full sweeps with this named consent test green: retain both failure shapes and resolve or file the separate consent-action failure when addressing that original AC. The earlier diagnostic sweep also saw live_140_frame_filter_count_accurate report zero frames; it passed the final sweep and isolated rerun, and no target-count instrumentation was captured for that row. Evidence: iteration252 PR closure report and .git/ralph-loop/20260909-takeover/iter252/resume-1 logs. Original scope and acceptance criteria remain unchanged."
---

# Iteration 262: the daemon counts a frame target and never promotes it to live

> Filed by [[iteration-246-sweep-load-misclassification]] Part D, which instrumented the wait,
> got one honest failure, and reached verdict (1) — a product race — on the evidence below.
> Iteration 246 deliberately did not attempt the fix: it is daemon bookkeeping, it needs its own
> live test that fails before and passes after, and 246 already carried four merged plans.

## The evidence

From iteration 246's closing sweep (`FF_RDP_LIVE_TESTS=1 FF_RDP_LIVE_NETWORK_TESTS=1`,
default `--jobs`, macOS, 2026-09-07, `executed=328 … total=328`, 308 passed / 11 failed):

```
---- live_137_daemon_mode_parity::live_137_consent_accept_via_daemon stdout ----
LIVE_TARGET_WAIT port=55418 reached=false elapsed_ms=15301 polls=49 bound_ms=15000
… last daemon status: status=Some(0) stdout={
  "running": true, "pid": 57215, "port": 55556, "uptime_seconds": 20,
  "connections": 0, "buffer_sizes": { "network-event": 446 },
  "target_count": 1, "live_target_count": 0,
  "dispatcher": { "alive": true, "frames_started": 130, "frames_finished": 130,
                  "in_flight": 0, "last_frame_kind": "resources-updated-array" }, … }

---- live_145_…::live_145_click_frame_scan_js_exception_envelope stdout ----
LIVE_TARGET_WAIT port=57307 reached=false elapsed_ms=15163 polls=48 bound_ms=15000

---- live_145_…::live_145_click_element_not_found_unchanged stdout ----
LIVE_TARGET_WAIT port=57200 reached=false elapsed_ms=15048 polls=47 bound_ms=15000
```

Three things in that status rule out the two non-product explanations:

1. **`target_count: 1, live_target_count: 0` after 20 s of uptime.** The daemon saw a frame
   target and never promoted it. A machine too slow to answer within 15 s would show the count
   arriving late, not a target counted and permanently not-live.
2. **`dispatcher.alive: true`, `frames_started: 130 == frames_finished: 130`, `in_flight: 0`.**
   The daemon is neither wedged nor behind; it answered 47–49 `daemon status` polls inside the
   window, each well under the loop's 300 ms spacing.
3. **All three failures exhaust the bound within 300 ms of each other** (15048 / 15163 /
   15301 ms). A bound that is merely too tight produces a distribution straddling it. This
   produces a cliff, which is what a latched state looks like.

Same shape as [[iteration-179-live-62-runner-sees-no-network-events]]: a subscription armed after
the event it needs, so the arming is missed and never repaired.

## A second, deliberately-unattributed observation from the same sweep

```
live_network_default_watcher::live_network_watcher_source_after_navigate_with_network
  at least one network entry should have source='watcher' after navigate --with-network.
  Got 0 entries, all with sources: []
```

Another daemon-side subscription reporting **nothing** in the same run — and the daemon status
quoted above shows `buffer_sizes: { "network-event": 446 }`, i.e. the network buffer was far from
empty on a *different* daemon in the same sweep. It is folded in here rather than given its own
plan because it is the same *family* (a subscription that produced no observations under sweep
load) and it has never been filed anywhere.

**Do not assume it shares a cause with the frame-target rows.** It is a different resource type
and a different code path. If the frame-target fix lands and this still fails, it earns its own
plan at that point, with whatever the fix's instrumentation says about it.

## Themes

- **A — Find where `live_target_count` is set and why it can stay 0 with `target_count` at 1.**
  The two counters disagree, so the promotion step is the suspect, not the enumeration.
- **B — Decide whether the fix is ordering (arm before the event) or repair (re-derive liveness
  when a target is observed).** 179/181's precedent is ordering; a latched counter may need both.
- **C — A live test that fails before the fix and passes after.** Iteration 246 could not write
  one because the trigger is contention; find a deterministic trigger, or drive the daemon's
  subscription directly.

## Tasks

### A. Locate [0/2]
- [ ] Identify every write to the live-target bookkeeping in `daemon/server.rs`
- [ ] Explain, in writing, how `target_count: 1` and `live_target_count: 0` coexist

### B. Fix [0/2]
- [ ] Land the fix the explanation points at
- [ ] State whether it is ordering, repair, or both

### C. Test [0/1]
- [ ] A live Firefox test that fails before the fix and passes after — not only a unit test

## Acceptance Criteria [0/5]

- [ ] The coexistence of `target_count > 0` and `live_target_count == 0` is explained in writing
- [ ] A live test fails on the pre-fix build and passes on the post-fix one
- [ ] Three consecutive full live sweeps with `live_137_consent_accept_via_daemon`,
      `live_145_click_frame_scan_js_exception_envelope` and
      `live_145_click_element_not_found_unchanged` green
- [ ] `live_network_watcher_source_after_navigate_with_network` is either green in those same
      three sweeps or filed as its own plan with the fix's evidence
- [ ] `cargo fmt && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace -q` clean

## Out of scope

- Raising the 15 s bound in `common::wait_for_live_targets`. Iteration 246 established that the
  bound is not the defect and that raising it would have hidden this; `FF_RDP_TEST_LIVE_TARGET_WAIT_S`
  exists for a deliberate one-run measurement, not for a new default.
- The seven `--full-page` screenshot failures in the same sweep — those are
  [[iteration-257-firefox-155-drawsnapshot-dictionary-arg]].

## References

- [[iteration-246-sweep-load-misclassification]] — the instrumentation and the verdict
- [[iteration-179-live-62-runner-sees-no-network-events]] — the precedent for this shape
- [[iteration-181-playbook-scoped-network-subscription]] — its fix, on the daemon/direct split
- `crates/ff-rdp-cli/tests/common/mod.rs` — `wait_for_live_targets`, `LIVE_TARGET_WAIT`
