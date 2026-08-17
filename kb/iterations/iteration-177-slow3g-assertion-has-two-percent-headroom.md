---
title: "Iteration 177: the slow-3g throttle assertion has ~2% headroom, so it reds on baseline jitter rather than on a throttling regression"
type: iteration
date: 2026-08-17
status: planned
branch: iter-177/slow3g-assertion-headroom
depends_on: []
first_call_sites: []
dogfood_path: |
  # Test-reliability defect, not a product defect. The measurement itself is
  # honest — throttling really does slow the fetch — but the threshold sits so
  # close to the observed ratio that ordinary baseline jitter decides the
  # verdict.

  # 1. Run the test in isolation on an idle machine and read the numbers it
  #    already prints. They are the whole repro.
  FF_RDP_LIVE_TESTS=1 FF_RDP_LIVE_NETWORK_TESTS=1 \
    cargo test -p ff-rdp-cli --test live live_throttle_slow3g_slows_fetch \
      -- --include-ignored --nocapture
  # → observed 2026-08-17, idle:  baseline 378ms  throttled 775ms  = 2.05x  PASS
  # → observed 2026-08-17, sweep: baseline 409ms  throttled 779ms  = 1.90x  FAIL
  #   The throttled figure barely moved (775 → 779 ms, +0.5%). The BASELINE
  #   moved 8%, and that is what flipped the result.

  # 2. Repeat it a few times to see the spread for yourself before changing
  #    anything — the fix must be argued from a measured distribution, not from
  #    one pass and one fail.
  for i in 1 2 3 4 5; do
    FF_RDP_LIVE_TESTS=1 FF_RDP_LIVE_NETWORK_TESTS=1 \
      cargo test -p ff-rdp-cli --test live live_throttle_slow3g_slows_fetch \
        -- --include-ignored --nocapture 2>&1 | grep -E "baseline fetch|throttled fetch"
  done
tags: [iteration, testing, reliability, throttle]
---

# Iteration 177: a 2× assertion measured at 2.05×

Carry-over from [[iteration-172-daemon-registry-torn-read-on-autostart]]'s dual-gate live sweep.

## What was observed

```text
FF_RDP_LIVE_TESTS=1 FF_RDP_LIVE_NETWORK_TESTS=1 cargo run -p xtask -- live-sweep
LIVE_SWEEP_SUMMARY executed=277 skipped=0 preexisting=0 total=277  -> 276 passed / 1 failed

live_109_throttle_block::live_throttle_slow3g_slows_fetch   FAILED
  baseline fetch: a=455ms b=409ms → 409ms
  throttled fetch: a=779ms b=780ms → 779ms
  under slow-3g the fetch must take at least 2x baseline:
    baseline=409ms throttled=779ms
```

Re-run in isolation on the same machine, idle, immediately afterwards:

```text
baseline fetch: a=378ms b=384ms → 378ms
throttled fetch: a=775ms b=781ms → 775ms
... ok    (2.05x)
```

## Why this is not "just load"

It is tempting to write this off as a contended sweep, file nothing, and move on. The numbers say
otherwise:

- The **throttled** measurement is stable across load: 775 ms idle, 779 ms under a 2320-second
  contended tier. slow-3g is a bandwidth/latency emulation, so it dominates and it does not care
  about the machine.
- The **baseline** measurement is the volatile one: 378 ms idle, 409 ms loaded (+8%).
- The assertion is `throttled >= baseline * 2.0`, and the idle ratio is **2.05**. There is
  roughly two percent of headroom. Any baseline drift above ~3% reds the test.

So the test does not fail because throttling broke. It fails because a *fast* baseline is what the
assertion needs, and the baseline is the half of the measurement nothing controls. That is a
defect in the assertion's design, and the disposition rules in the `iteration-close` skill are
explicit that "environmental" is a diagnosis, not a disposition.

It also has a real cost already paid: this red is indistinguishable at a glance from a genuine
throttling regression, which is precisely what iterations 155/158/164 spent themselves making
impossible elsewhere.

## Themes

- **A — Measure the distribution before touching the threshold.** Run the fetch pair enough times,
  idle and under synthetic load, to state what the ratio actually is and how much it moves. A
  threshold picked from two samples is how the current one got here. Record the numbers in this
  plan.
- **B — Make the assertion depend on the stable half of the measurement.** The throttled figure is
  the reproducible one. Options to weigh, with the rejected ones recorded:
  - assert an absolute floor on the throttled time (e.g. `throttled >= 600 ms`) — pins the property
    that actually holds, but hard-codes a number that a faster network invalidates;
  - keep the ratio but take the **minimum** baseline over more samples, so contention can only make
    the baseline look faster, never slower;
  - keep the ratio and lower it to something the measured distribution supports, saying in the
    message what the observed spread was;
  - assert on the throttle profile's *declared* bandwidth rather than on wall-clock at all.
  Whatever is chosen, the failure message must print both samples of both measurements — it
  already does, and that is the only reason this was diagnosable at all.
- **C — Check the neighbours.** `live_109_throttle_block.rs` is not the only live test asserting a
  wall-clock ratio. Enumerate the others and say for each whether it has the same thin margin, or
  state that it does not.

## Tasks

### A. Measure
- [ ] Record ≥10 idle baseline/throttled pairs in this plan
- [ ] Record ≥5 pairs taken while a live sweep (or equivalent load) is running
- [ ] State the observed ratio distribution: min, median, max

### B. Fix
- [ ] The chosen assertion change, with the rejected alternatives recorded
- [ ] The test still fails if throttling is genuinely disabled — demonstrate it, do not assume it

### C. Neighbours
- [ ] Enumerate every live test asserting a wall-clock ratio or duration, with a verdict per test

## Acceptance Criteria [0/4]

- [ ] `live_throttle_slow3g_slows_fetch`: the assertion is stated against the measurement that the
      Theme A data shows is stable, and the plan records the distribution it was chosen from
- [ ] `live_throttle_slow3g_slows_fetch` still fails when throttling is disabled — shown by a run
      with the throttle step removed, pasted into this plan
- [ ] `live_throttle_slow3g_slows_fetch` passes in a contended dual-gate sweep
- [ ] `cargo fmt && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace -q`
      clean, plus a dual-gate live sweep

## Design notes

Do **not** simply delete the assertion or mark the test `#[ignore]`. It is the only live coverage
that `throttle slow-3g` has any effect at all, and iteration 164 already had to fix a case where
`throttle --block` was accepted and then silently discarded. A weaker-but-honest assertion is the
goal; no assertion is not.

## Out of scope

- Changing the throttling implementation. Nothing here suggests the product misbehaved — the
  throttled figure was reproducible to within 0.5% across a 2320-second load swing.

## References

- [[iteration-172-daemon-registry-torn-read-on-autostart]] — the sweep that surfaced this
- [[iteration-164-two-failures-the-158-sweep-uncovered]] — the previous throttle defect, which is
  why the assertion must not simply be removed
