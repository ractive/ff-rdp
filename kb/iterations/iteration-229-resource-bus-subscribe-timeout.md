---
title: "Iteration 229: live_resource_dedupe times out on its first subscribe under sweep load"
type: iteration
date: 2026-08-31
status: planned
branch: iter-229/resource-bus-subscribe-timeout
depends_on:
  - 225
dogfood_path: |
  # Reproduce under contention — the failure does not appear in isolation.
  FF_RDP_LIVE_TESTS=1 FF_RDP_LIVE_NETWORK_TESTS=1 cargo run -p xtask -- live-sweep
  # 2026-08-31 (iter-225's first sweep, 311 CLI-tier tests at --test-threads=6):
  #   live_61q_resource_bus::live_resource_dedupe ... FAILED
  #   panicked at crates/ff-rdp-cli/tests/live/live_61q_resource_bus.rs:258: subscribe A: Timeout
  FF_RDP_LIVE_TESTS=1 cargo test -p ff-rdp-cli --test live -q \
    live_61q_resource_bus::live_resource_dedupe -- --include-ignored --test-threads=1
  # ok in 2.6s — which is exactly why this needs its own plan rather than a re-run
tags: [iteration, live-tests, resource-bus, flake, carry-over]
---

# Iteration 229: `live_resource_dedupe`'s first subscribe times out under load

## Why

`ResourceCommand::subscribe` returned `Timeout` on the **first** of two subscriptions during
iteration 225's live sweep — one failure in 311 CLI-tier tests running at `--test-threads=6`, and
green in 2.6 s when re-run alone. Nothing in 225's diff touches the resource bus (it changed
`commands/page_view.rs`, `commands/page_view_js.rs` and their tests), so this is not that
iteration's defect; it is an unowned, load-sensitive failure that has never been filed.

`kb/discipline-rationale.md`'s rule applies: **"environmental" is a diagnosis, not a
disposition.** A live test that times out under contention is either

- a test that under-budgets a real round trip that legitimately takes longer when six Firefox
  processes share the machine — a test defect, fixable by budgeting honestly rather than by
  raising a number until it stops failing; or
- `getWatcher` / `watchResources` genuinely serialising behind something on the parent process,
  in which case a caller with a busy browser sees the same timeout and it is a product defect.

Which of the two it is has not been established, and a sweep that quietly re-runs it green is
precisely how that question stops being asked.

## Themes

- **A — Reproduce deliberately.** Run the suite under a controlled load (`--test-threads=6` over
  the CLI tier, or a narrower harness that spawns N browsers) until the failure recurs, and
  capture *which* leg of the subscribe is slow: the `getWatcher` request, the `watchResources`
  reply, or the first resource frame.
- **B — Attribute it.** Instrument the timing on the failing leg. If the wire round trip is
  inside the budget and the test's own wait is not, it is a test defect; if the reply genuinely
  does not arrive, it is a product defect and gets its own follow-up.
- **C — Fix the attributed cause, not the symptom.** A raised timeout is acceptable *only* with a
  measured distribution behind it, recorded in the plan.

## Tasks

### A. Reproduce [0/2]
- [ ] Recur the failure under controlled load, with the run recorded
- [ ] Identify which leg of `subscribe` exceeds its budget

### B. Attribute [0/1]
- [ ] State, with timings, whether this is a test-budget defect or a product defect

### C. Fix [0/1]
- [ ] Land the fix the attribution points at; no timeout raised without a measured distribution

## Acceptance Criteria [0/3]

- [ ] The failure is reproduced deliberately at least once, not merely waited for
- [ ] The cause is attributed to the test or to the product, in writing, with timings
- [ ] Three consecutive full live sweeps at the default thread count with
      `live_61q_resource_bus` green

## Out of scope

- Any other flaky live test. If the investigation finds a shared cause (parent-process
  serialisation under N browsers), file that as its own plan rather than widening this one.

## References

- [[iteration-225-reader-excerpt-infobox]] — the sweep that surfaced it
- `crates/ff-rdp-cli/tests/live/live_61q_resource_bus.rs:258` — the failing assertion
