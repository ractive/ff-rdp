---
title: "Iteration 198: the daemon's frame-target subscription misses a fixed 15 s bound under a parallel sweep"
type: iteration
date: 2026-08-23
status: planned
branch: iter-198/live-tests-red-under-concurrency
depends_on: [kb/iterations/iteration-188-live-sweep-cost-and-parallelism.md]
first_call_sites: []
dogfood_path: |
  # Both tests pass serially and pass in most concurrent runs, so the only
  # honest reproduction is repetition at the sweep's own concurrency.

  # 1. Serial control — expected green:
  FF_RDP_LIVE_TESTS=1 FF_RDP_LIVE_NETWORK_TESTS=1 cargo test -p ff-rdp-cli --test live \
    live_137_consent_accept_via_daemon -- --include-ignored --test-threads=1
  FF_RDP_LIVE_TESTS=1 FF_RDP_LIVE_NETWORK_TESTS=1 cargo test -p ff-rdp-cli --test live \
    live_145_click_frame_scan_js_exception_envelope -- --include-ignored --test-threads=1

  # 2. Under the real sweep, repeated. Each of these failed once across
  #    iteration 188's parallel sweeps (2026-08-23, --jobs 6, 10-core machine):
  #      live_145_error_envelope_completeness::live_145_click_frame_scan_js_exception_envelope
  #        "daemon never reported live frame targets"
  #      live_137_daemon_mode_parity::live_137_consent_accept_via_daemon
  for i in 1 2 3; do
    FF_RDP_LIVE_TESTS=1 FF_RDP_LIVE_NETWORK_TESTS=1 cargo run -p xtask -- live-sweep --jobs 6
  done
  # Record the failure set per run. A fix means three runs with neither name in it.
tags: [iteration, testing, live-tests, flaky, daemon, carry-over]
---

# Iteration 198: "passes alone, fails under load" for two daemon-route tests

## Where this came from

[[iteration-188-live-sweep-cost-and-parallelism]] made the CLI live tier run concurrently and then
ran it repeatedly. Two tests failed once each across those runs, and neither is explained by any
open plan:

| run | jobs | failing test | message |
|---|---|---|---|
| 1 | 6 | `live_145_error_envelope_completeness::live_145_click_frame_scan_js_exception_envelope` | `daemon never reported live frame targets` |
| 2 | 6 | `live_137_daemon_mode_parity::live_137_consent_accept_via_daemon` | `daemon never reported live frame targets` |
| 4 | 6 | `live_140_element_targeting::live_140_frame_error_bounded` | frame-target assertion |
| 4 | 6 | `live_145_…click_frame_scan_js_exception_envelope` | `daemon never reported live frame targets` |
| 4 | 6 | `live_169_nav_verb_status_parity::live_169_nav_verbs_report_status_daemon` | `status: null, status_reason: "not_observed"` after `elapsed_ms: 21017` |
| 5 | 6 | `live_137_…consent_accept_via_daemon` | `daemon never reported live frame targets` |
| 6 | 6 | `live_160_envelope_honesty::live_160_click_reachable_fires_handler` | click handler assertion |

**No test failed twice in the same way in consecutive runs, and no run repeated another's failure
set** — but three of the seven failures carry the *same* message, which is the thread to pull.

## The shared signature

`live_137_daemon_mode_parity.rs:116`:

```rust
fn wait_for_live_targets(port: u16) -> bool {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    …
}
```

A fixed 15 s bound on "the daemon has established its frame-target subscription". `live_145`
carries its own copy of the same wait with the same message. On an idle machine that bound is
never approached; under a `--jobs 6` sweep (load average 150+ with six Firefox instances starting)
it is. `live_169`'s failure is the same shape one level up: the daemon reported
`status_reason: "not_observed"` after **21 s**, i.e. it did observe the navigation, just not
inside whatever window the assertion allows.

`live_137` was also on iteration 188's Theme A list of tests that failed at `-j8` under
`cargo nextest`, classified there as a "contention artifact, not a defect" — on no evidence beyond
the fact that it passed at lower concurrency. **That classification is the thing this iteration
exists to replace.** "It only fails under load" is a description, not a diagnosis: iteration 179 spent a
whole iteration establishing that exactly this description hid a real arming race
(`assert_network` on the direct route), which iteration 181 then fixed.

Both failures are on the **daemon** route, which is the same neighbourhood as the fix in 181 and
the frame-target bookkeeping in [[iteration-129-consent-and-cross-origin-frames]].

## The question to answer

For each test, decide between:

1. **A real race in the product** — e.g. a `watchTargets`/frame-target subscription that is armed
   after the event it needs, which is 179's shape exactly. Fix the product.
2. **A race in the test** — a poll bound that is generous on an idle machine and too tight at
   load average 150. Fix the bound, and say what the new bound is measured against.
3. **A resource ceiling** — the machine cannot start N browsers and keep them responsive. Then it
   is a concurrency-policy finding for `live-sweep`, not a test fix.

Do not accept (3) without a measurement, and do not accept (2) without stating the observed
timing distribution. A widened timeout that hides a real race is how iteration 179's defect
survived several iterations.

## Tasks

### A. Reproduce [0/3]
- [ ] Each named test 10x serially (control) and across at least 3 sweeps at `--jobs 6`
- [ ] Capture the daemon-route logs from a failing instance, not just the assertion text
- [ ] Instrument `wait_for_live_targets` to record how long the subscription *actually* takes,
      idle and under sweep load — the distribution, not one number

### B. Classify and act [0/2]
- [ ] One of the three verdicts above per test, in writing, with the evidence
- [ ] The fix that follows from the verdict, plus the repeated run that shows it holding

## Acceptance Criteria [0/3]

- [ ] Three consecutive `--jobs 6` sweeps with an empty failure set
- [ ] Neither fix widens a timeout without a stated measurement behind the new value
- [ ] If either turns out to be a product race, it has a live Firefox test that fails before the
      fix and passes after — not only a unit test

## Out of scope

- `live_153_replace_double_envelope` — diagnosed and fixed inside iteration 188 (it was a real
  regression from that iteration's `FF_RDP_HOME` change, not load; it failed serially too, which
  is exactly how it was told apart from the tests listed here).
- Lowering the sweep's concurrency to make these pass. That trades the 8x this batch bought for a
  green that hides the same race; iteration 188 chose to keep the speed and own the flakes here.
- The sweep hanging on `live_158_launch_survives_contended_bind` — that is
  [[iteration-197-live-sweep-has-no-per-test-timeout]].

## References

- [[iteration-188-live-sweep-cost-and-parallelism]] — the parallel sweep that surfaced both
- [[iteration-179-live-62-runner-sees-no-network-events]] — the precedent: "fails only under load"
  turned out to be a real arming race
- [[iteration-181-playbook-scoped-network-subscription]] — its fix, on the daemon/direct split
