---
title: "Iteration 183: two live tests time out against the daemon under sweep load, with no margin to absorb it"
type: iteration
date: 2026-08-22
status: planned
branch: iter-183/live-daemon-timeouts-under-sweep-load
depends_on: []
first_call_sites: []
dogfood_path: |
  # Test-reliability defect in the same family as iteration 177 (a 2% threshold
  # margin) and iteration 179 (a lost arming race). The product is not wrong;
  # the tests bound a wait that machine load can exceed.

  # 1. live_104_security_pwa::live_manifest_fetch_canonical — the single red in
  #    iteration 179's serial sweep, 2026-08-22 (executed=277, 267 passed /
  #    1 failed, 15-min load ~49). The message is self-explaining since
  #    iteration 179 landed Theme A:
  #      manifest must exit 0 (no-manifest is not an error): status=Some(124)
  #      stdout={"error":"daemon did not respond within the timeout after auth
  #      — the daemon may be overloaded or the connection is stale.","error_type":"Timeout"}
  #    Its daemon_args() hard-code --timeout 20000.
  FF_RDP_LIVE_TESTS=1 FF_RDP_LIVE_NETWORK_TESTS=1 \
    cargo test -q -p ff-rdp-cli --test live -- --ignored --exact \
    live_104_security_pwa::live_manifest_fetch_canonical

  # 2. live_145_error_envelope_completeness::live_145_click_element_not_found_unchanged
  #    — FAILED at 21.9s in the -j6 nextest run on 2026-08-22, PASSED in the
  #    serial sweep the same hour. Same shape: a bounded wait, no margin.

  # 3. Reproduce on demand by loading the machine, as iteration 179 did:
  FF_RDP_LIVE_TESTS=1 FF_RDP_LIVE_NETWORK_TESTS=1 \
    cargo nextest run -p ff-rdp-cli --test live --run-ignored all -j6 --no-fail-fast &
  sleep 75
  # then run the two tests above and record `uptime` beside each verdict.

  # 4. Do NOT simply raise the numbers until they go green. Measure the
  #    distribution first (iteration 177's method), and say what margin the new
  #    bound buys.
tags: [iteration, testing, reliability, daemon, live-tests]
---

# Iteration 183: bounded daemon waits with no margin

Carry-over from [[iteration-179-live-62-runner-sees-no-network-events]]'s live sweep and its `-j6`
load experiment.

## What was observed

| test | serial sweep (load ~49) | `-j6` load generator | bound |
|---|---|---|---|
| `live_104_security_pwa::live_manifest_fetch_canonical` | **FAILED**, exit 124, `error_type: Timeout` | passed | `--timeout 20000`, hard-coded in the test's `daemon_args()` |
| `live_145_error_envelope_completeness::live_145_click_element_not_found_unchanged` | passed | **FAILED** at 21.9 s | not yet identified |

Neither is a product defect: in both cases `ff-rdp` returned a truthful, well-formed timeout
envelope. What is wrong is that the test treats a load-dependent wall-clock bound as a pass/fail
contract.

That the `live_104` failure was diagnosable at all is iteration 179's Theme A working on a test it
never touched — before it, the message ended at the colon.

## Themes

- **A — Measure before changing a bound.** For each test, collect the distribution of the wait it
  actually needs, idle and under the `-j6` generator, exactly as
  [[iteration-177-slow3g-assertion-has-two-percent-headroom]] required. A bound picked without a
  measured distribution is the same defect one notch further out.
- **B — Decide bound-vs-precondition.** A daemon that is genuinely overloaded is a legitimate
  reason to *skip loudly*, not to fail — but the loudness rule from
  [[iteration-146-live-suite-reliability]] Theme B applies: a detector softened because it fired
  is worse than no detector. Say which of the two each test is.

## Tasks

### A. Measure [0/2]
- [ ] `live_104`'s required daemon wait, idle and loaded, recorded as a distribution
- [ ] `live_145`'s bound identified, and its required wait recorded the same way

### B. Fix [0/2]
- [ ] Each bound either raised with a stated margin, or replaced by a loud precondition
- [ ] Both tests pass 8/8 under the `-j6` load generator

## Acceptance Criteria [0/2]

- [ ] Neither test's verdict depends on machine load across the measured range, and the margin is
      written down
- [ ] No bound was raised without a measured distribution behind it

## Out of scope

- `#[ignore]`-ing either test to make the sweep green.

## References

- [[iteration-179-live-62-runner-sees-no-network-events]] — the sweep and load experiment
- [[iteration-177-slow3g-assertion-has-two-percent-headroom]] — the method for arguing a bound
