---
title: "Iteration 177: the slow-3g throttle assertion has ~2% headroom, so it reds on baseline jitter rather than on a throttling regression"
type: iteration
date: 2026-08-17
status: in-review
branch: iter-177/slow3g-assertion-headroom
depends_on: []
first_call_sites: []
dogfood_path: |
  # Test-reliability defect, not a product defect. The measurement itself is
  # honest — throttling really does slow the fetch — but the assertion compared
  # two measurements from the same jittery population, so origin latency, not
  # throttling, decided the verdict.
  
  # 1. Run the test in isolation and read the numbers it prints. Since this
  #    iteration it prints DELIVERY DELAY (total minus the request's
  #    responseStart-requestStart), which is the throttler's own contribution.
  FF_RDP_LIVE_TESTS=1 FF_RDP_LIVE_NETWORK_TESTS=1 \
    cargo test -p ff-rdp-cli --test live live_throttle_slow3g_slows_fetch \
      -- --include-ignored --nocapture
  # → expected: baseline median 1-12 ms, throttled median ~410 ms, i.e. the
  #   400 ms round-trip latency slow-3g declares, against a 200 ms requirement.
  
  # 2. Repeat it to see the spread. 20 runs (10 idle, 10 loaded) gave
  #    402-413 ms; anything in that band is the throttler working.
  for i in 1 2 3 4 5; do
    FF_RDP_LIVE_TESTS=1 FF_RDP_LIVE_NETWORK_TESTS=1 \
      cargo test -p ff-rdp-cli --test live live_throttle_slow3g_slows_fetch \
        -- --include-ignored --nocapture 2>&1 | grep -E "delivery delay"
  done
  
  # 3. The pre-177 behaviour, for comparison: the old assertion compared TOTAL
  #    fetch times (throttled >= baseline * 2.0) and reddened on 2 of 10 idle
  #    runs. See "What iteration 177 measured" below.
tags:
  - iteration
  - testing
  - reliability
  - throttle
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

## What iteration 177 measured (Theme A)

All figures below are from one machine (macOS, 10 cores, `example.com` over h2 with the
connection already established), taken 2026-08-23 with `cargo test -p ff-rdp-cli --test live
live_throttle_slow3g_slows_fetch -- --include-ignored --nocapture`.

### The old assertion, 10 idle runs (2 baseline + 2 throttled samples, `min` of each, ratio ≥ 2.0)

```text
run  baseline a/b → min   throttled a/b → min   ratio  verdict
 1   369 370 → 369        880 782 → 782         2.12   ok
 2   379 458 → 379        771 802 → 771         2.03   ok
 3   366 468 → 366        837 762 → 762         2.08   ok
 4   129 371 → 129        506 836 → 506         3.92   ok
 5   374 365 → 365        783 779 → 779         2.13   ok
 6   379 363 → 363        780 783 → 780         2.15   ok
 7   102 361 → 102        791 512 → 512         5.02   ok
 8   103 362 → 103        777 765 → 765         7.43   ok
 9   374 423 → 374        546 772 → 546         1.46   FAILED
10   361 393 → 361        507 848 → 507         1.40   FAILED
```

**2 of 10 idle runs already failed** — on an idle machine, without any load at all. The observed
ratio ranged 1.40 – 7.43. That is a wider spread than the plan's premise assumed.

### Why: the totals are bimodal, and the bimodality is not throttling

A resource-timing probe (`performance.getEntriesByName`, run through
`cargo run -p ff-rdp-cli -- eval`) decomposes each fetch:

```text
baseline burst:   total 433 ttfb 431 | total 101 ttfb  99 | total 370 ttfb 367
                  total 470 ttfb 468 | total 409 ttfb 408 | total 102 ttfb 101
throttled burst:  total 518 ttfb 105 | total 786 ttfb 374 | total 802 ttfb 390
                  total 513 ttfb 101 | total 789 ttfb 382 | total 769 ttfb 362
(all samples: dns 0, tcp 0, h2 — the connection is reused every time)
```

Two facts fall out:

1. `example.com` answers in **either ~100 ms or ~370–470 ms**, at random, roughly one sample in
   five landing in the fast mode. This is origin/edge variance and it is present *identically*
   throttled and un-throttled. It is what produced the 1.40× and the 7.43× runs alike: `min` over
   two samples picks whichever mode it happened to hit, and comparing a fast-mode baseline against
   a slow-mode throttled sample (or vice versa) is meaningless.
2. **Throttling does not move TTFB at all.** Firefox's parent-process throttler holds the response
   back *after* `responseStart`, before handing it to content. `total - ttfb` is ~2 ms
   un-throttled and 407–413 ms throttled — against slow-3g's declared 400 ms round-trip latency.

So the original diagnosis ("the baseline is the volatile half") is right in its conclusion and
wrong in its mechanism: the volatility is not machine load, it is origin latency, and it sits in
*both* halves. The load figures in the plan header (baseline 378 → 409 ms) are consistent with a
mode shift, not with contention.

### The delivery-delay estimator: 10 idle runs

`delivery delay = total − (responseStart − requestStart)`, median of 5 samples per side:

```text
run   baseline delays (sorted) → median   throttled delays (sorted) → median   delta
 1    1,1,2,2,2      → 2                  408,409,412,412,414 → 412            410
 2    1,2,2,3,7      → 2                  406,410,412,419,435 → 412            410
 3    2,2,2,2,2      → 2                  405,408,412,412,412 → 412            410
 4    1,2,2,3,4      → 2                  411,411,411,413,420 → 411            409
 5    1,2,3,3,3      → 3                  406,410,411,411,414 → 411            408
 6    1,2,2,2,3      → 2                  405,412,413,420,433 → 413            411
 7    1,2,2,2,2      → 2                  408,409,413,413,444 → 413            411
 8    1,2,2,3,37     → 2                  409,411,412,412,414 → 412            410
 9    1,2,2,3,3      → 2                  405,405,411,412,412 → 411            409
10    1,2,2,2,2      → 2                  403,404,415,420,443 → 415            413
```

**Distribution: min 408 ms, median 410 ms, max 413 ms.** 10/10 pass. Against the 200 ms
requirement that is 104–107% headroom, where the ratio assertion had ~2%.

### The delivery-delay estimator, 11 runs under synthetic load

Load generated with `yes > /dev/null` spinners on a 10-core machine (1×, then 2× the core count;
load average 24 → 191). A live sweep was deliberately *not* used as the load source — running a
second copy of this test concurrently with a sweep would contaminate the sweep.

```text
 1  (10 spinners)  baseline —                  throttled —                    FAILED before sampling
 2  (10 spinners)  3,4,5,5,17  → 5             407,407,410,411,445 → 410      405
 3  (10 spinners)  1,3,4,5,6   → 4             411,413,417,433,447 → 417      413
 4  (10 spinners)  6,6,9,16,41 → 9             410,410,411,413,451 → 411      402
 5  (10 spinners)  4,4,6,8,8   → 6             409,409,413,416,470 → 413      407
 6  (10 spinners)  3,3,5,7,28  → 5             408,410,412,429,448 → 412      407
 7  (10 spinners)  3,4,5,5,6   → 5             408,408,409,431,432 → 409      404
 8  (10 spinners)  2,3,3,4,7   → 3             406,408,412,424,452 → 412      409
 9  (20 spinners)  3,4,5,14,18 → 5             410,412,414,431,451 → 414      409
10  (20 spinners)  4,8,12,18,53 → 12           410,415,422,426,431 → 422      410
```

**Under-load distribution: min 402 ms, median 408.5 ms, max 413 ms** — statistically
indistinguishable from idle. The throttled figure is the stable half, as the plan predicted; the
delivery-delay estimator makes the baseline half stable too (1–12 ms medians).

Run 1 failed **before printing any sample**, i.e. in Firefox launch or daemon start under a load
average of ~190, not in the assertion. Its message was not captured and two later attempts at the
same spinner count did not reproduce it, so it is recorded here as unattributed startup
flakiness rather than claimed as anything. It is not evidence for or against this iteration's
change.

## The chosen assertion (Theme B)

```rust
let declared_latency_ms = f64::from(u32::try_from(ThrottleProfile::Slow3g.latency_ms())?);
let required_delta = declared_latency_ms * MIN_LATENCY_FRACTION; // 0.5 → 200 ms
assert!(throttled_median_delay - baseline_median_delay >= required_delta, ...);
```

- The measured quantity is **delivery delay** (`total − TTFB`), which isolates the throttler's own
  contribution and discards origin latency entirely.
- The threshold is **derived from the profile** (`ThrottleProfile::Slow3g.latency_ms()`), not
  hard-coded, so it follows the preset if the preset ever changes.
- The estimator is the **median of 5**, not the min of 2, so two anomalous samples per side cannot
  decide the verdict.
- The failure message prints every sample of both measurements, as the plan requires.

### Cost

The test now takes 10 samples where it took 4, so its wall time went from ~6.2 s to ~10.2 s idle
(~12 s under load). That is +4 s on a sweep that runs 277 tests, and it is the price of the
headroom; [[iteration-188-live-sweep-cost-and-parallelism]] is where sweep cost is being addressed
and already names this test.

### Rejected alternatives

| Option | Why rejected |
| --- | --- |
| Absolute floor on the throttled *total* (`throttled >= 600 ms`) | On this network the un-throttled total already reaches 470 ms and the throttled total drops to 506 ms in the fast TTFB mode — the two populations overlap, so no single constant separates them. Also hard-codes a number a faster origin invalidates. |
| Keep the ratio, take `min` over more samples | Does not address the cause. The min of a bimodal population is the *fast mode*, so more samples makes the baseline min converge on ~100 ms while the throttled min converges on ~510 ms — a 5× ratio that would pass even with throttling half-broken. |
| Keep the ratio, lower the multiplier to what the data supports | The measured ratio spread was 1.40 – 7.43. Any multiplier low enough to never red (≤1.3) is too low to catch a real regression. The ratio is not salvageable on this measurement. |
| Assert only on the profile's declared bandwidth/latency, no wall clock | That is a unit test, and it already exists (`network_parent.rs`). It would delete the only live evidence that `throttle slow-3g` reaches Firefox at all — exactly what iteration 164 had to fix. |
| Additive delta on *totals* (`throttled_total − baseline_total >= 200 ms`) | Better than the ratio (baseline error costs 1:1, not 2:1) but still crosses the TTFB modes: a slow-mode baseline against a fast-mode throttled sample gives a 144 ms delta. Measured failing: run 10 of the median-of-totals experiment. |

### Negative control (the test still fails when throttling is off)

The `throttle slow-3g` step was temporarily replaced with `throttle off` (and the envelope
assertion adjusted to match) and the test re-run:

```text
baseline delivery delay: [0,2,5,5,18] → median 5ms
throttled delivery delay: [2,2,2,2,3] → median 2ms
panicked at crates/ff-rdp-cli/tests/live/live_109_throttle_block.rs:262:5:
under slow-3g the fetch must pay at least 200ms more delivery delay than baseline (half of the
profile's declared 400ms round-trip latency), but paid -3ms: baseline median=5ms [0,2,5,5,18]
throttled median=2ms [2,2,2,2,3]
```

The separation is ~200×: 2 ms un-throttled against 410 ms throttled. The change was reverted with
`git checkout --` immediately afterwards; it is not in the branch.

## Neighbours (Theme C)

Every live test that reads a wall clock, and whether it shares the defect. The defect is
specifically **comparing one measurement against another measurement**; every other test here
compares a measurement against a *constant the test itself chose*, which no amount of jitter can
move.

| Test | Assertion | Verdict |
| --- | --- | --- |
| `live_109_throttle_block::live_throttle_slow3g_slows_fetch` | was `throttled_total >= baseline_total * 2.0` | **The defect.** Fixed by this iteration. The only measurement-vs-measurement assertion in the live suite. |
| `live_113_launch_timeout::…` | `elapsed < 5 s` against a declared 1 s launch budget | Fixed constant, 5× slack over the budget it proves. Not thin. |
| `live_129_frames_and_consent::live_129_click_zero_match_error` | `elapsed < 8 s`, proving the 10 s auto-wait was not paid | Measured 2026-08-23: **1.006 s**. 8× headroom against a fixed bound. Not thin. |
| `live_145_error_envelope_completeness::live_145_click_element_not_found_unchanged` | `elapsed < 8 s`, same shape | Measured 2026-08-23: **0.946 s**. 8.4× headroom. Not thin. |
| `live_138_navigation_truthfulness_2` (3 sites) | `wall_ms < timeout_ms / 2` where the test passes `--timeout 8000`/`3000` itself | The bound scales with the budget the test set; both sides are under the test's control and the operation is expected to be sub-second. Not thin. |
| `live_139_perf_honesty_2` | `measured_at_ms ∈ [before, after]` | A containment check on the call's own window, not a duration budget. No margin to erode. |
| `live_130_navigation_truthfulness` | `elapsed_ms > 0` | Presence check. Not a budget. |
| `live_151_residual_leak` | `elapsed` is printed only, never asserted | No assertion. |
| `ff-rdp-core::live_record_fixtures::live_cookies_httponly` | `elapsed < HTTP_SERVER_ACCEPT_DEADLINE` (30 s) on a loopback round trip | Fixed 30 s constant against a local HTTP request. Not thin. |
| `live_129_frames_and_consent` (`scroll_height > viewport_height * 2.0`) | a ratio, but of page geometry | Deterministic layout of a fixture page, no clock involved. Out of scope. |


## Tasks

### A. Measure
- [x] Record ≥10 idle baseline/throttled pairs in this plan
- [x] Record ≥5 pairs taken while a live sweep (or equivalent load) is running
- [x] State the observed ratio distribution: min, median, max — old ratio: min 1.40, median 2.12, max 7.43 (2/10 idle runs red). New delivery-delta: min 402 ms, median 410 ms, max 413 ms across 20 runs idle + loaded, against a 200 ms requirement

### B. Fix
- [x] The chosen assertion change, with the rejected alternatives recorded
- [x] The test still fails if throttling is genuinely disabled — demonstrate it, do not assume it

### C. Neighbours
- [x] Enumerate every live test asserting a wall-clock ratio or duration, with a verdict per test

## Acceptance Criteria [4/4]

- [x] `live_throttle_slow3g_slows_fetch`: the assertion is stated against the measurement that the
      Theme A data shows is stable, and the plan records the distribution it was chosen from
- [x] `live_throttle_slow3g_slows_fetch` still fails when throttling is disabled — shown by a run
      with the throttle step removed, pasted into this plan
- [x] `live_throttle_slow3g_slows_fetch` passes in a contended dual-gate sweep [2026-08-23: ok in
      `LIVE_SWEEP_SUMMARY executed=284 skipped=0 preexisting=0 vanished=0 launch_timeout=0
      total=284`. The sweep machine was otherwise idle; contention is covered by the 10
      spinner-loaded runs recorded above, min delta 402 ms against a 200 ms requirement]
- [x] `cargo fmt && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace -q`
      clean, plus a dual-gate live sweep [2026-08-23: all three clean; sweep above]

## Closing sweep (2026-08-23)

```text
FF_RDP_LIVE_TESTS=1 FF_RDP_LIVE_NETWORK_TESTS=1 cargo run -p xtask -- live-sweep
LIVE_SWEEP_SUMMARY executed=284 skipped=0 preexisting=0 vanished=0 launch_timeout=0 total=284
  -> 274 passed / 1 failed  (ff-rdp-cli live target, 752.57s), every other target ok

test live_109_throttle_block::live_throttle_slow3g_slows_fetch ... ok
test live_109_throttle_block::live_block_url_pattern ... ok
```

The single failure was **self-inflicted and not a product defect**:

```text
live_96_profile_cleanup::live_profiles_prune_removes_all_when_no_firefox_running ... FAILED
  precondition violated — 1 ff-rdp-managed profile dir(s) ... still owned by a live process,
  so `prune --all` would rip a profile out from under it: .../ff-rdp-profile-19tVNJH3N2mUoNuq
  (pid 61353, spawned by unknown test).
```

pid 61353 was the port-6000 browser started for the `preexisting` tier. It was started with
`ff-rdp launch --port 6000 --headless` instead of the raw
`firefox -no-remote --start-debugger-server 6000 --headless` the closing procedure documents, so
it owned an **ff-rdp-managed** profile and `live_96` correctly refused to prune it. Killed and
re-run in isolation:

```text
test live_96_profile_cleanup::live_profiles_prune_removes_all_when_no_firefox_running ... ok
test live_96_profile_cleanup::live_daemon_stop_profile_path_matches_launch_json ... ok
test live_96_profile_cleanup::pre_fix_repro_daemon_stop_removes_active_profile ... ok
test live_109_throttle_block::live_throttle_slow3g_slows_fetch ... ok
  (baseline delivery delay [2,2,3,3,3] → 3ms; throttled [404,407,407,420,440] → 407ms)
```

The sweep itself was **serial and otherwise idle** — nothing else was running on the machine.
Contention was measured separately, in the 10 spinner-loaded runs above, because running a second
copy of this test alongside a sweep would contaminate the sweep it is supposed to validate.


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
