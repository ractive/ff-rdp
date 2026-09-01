---
title: "Iteration 232: click's missed selector waits the full --timeout before reporting not-found"
type: iteration
date: 2026-09-01
status: planned
branch: iter-232/click-not-found-poll-timeout
depends_on: []
dogfood_path: |
  ff-rdp launch --headless
  ff-rdp navigate 'https://example.com'
  time ff-rdp click --selector '#definitely-not-on-the-page'
  # TODAY: ~10s wall clock (the default --timeout) before "0 elements matched (not found)".
  # AFTER: the poll still gives a late-appearing element its full budget, but a selector that
  # never matches anything reachable in the DOM is reported well under the timeout.
  ff-rdp daemon stop
tags:
  - iteration
  - agent-ergonomics
  - carry-over
  - click
---

# Iteration 232: `click`'s not-found poll costs a full timeout on a guessed selector

## Why

Carried unfixed across two iterations without ever getting its own plan:

- [[iteration-228-two-task-benchmark-after-facts]] first observed it: `link_follow` runs 1 and 2
  both guessed `a[href="/wiki/Charles_Babbage"]`, which really does match zero elements (Wikipedia
  writes that attribute as a relative URL, not absolute), and each guess cost the full 10s
  `--timeout` before `click` reported `0 elements matched (not found)`. Filed as "observed,
  deliberately not acted on" — no turn cost, so out of scope there.
- [[iteration-230-quickstart-navigate-with-page]]'s carry-over table repeated the same disposition
  — "no plan, with reason" — because the defect did not fire in any of that iteration's six
  re-measurement runs (all six clicked by `--ref`, not a guessed selector), and named the
  condition under which it would need a plan: "if a run loses turns to it... it needs its own
  plan."
- [[iteration-231-infobox-facts-refs-and-query-matching]] repeated the "out of scope" note a third
  time without changing the disposition.

That condition — turn cost, not just wall-clock cost — has not yet fired in a measured benchmark
run. But wall-clock cost is a real cost on its own (a 10s stall reads as a hang to anything
watching the process, and CI/dogfood scripts pay it directly), and three iterations citing the same
unfiled defect is itself the thing the carry-over sweep exists to catch. This plan is the fix, not
a fourth deferral.

## Root cause (to confirm, not assumed)

`crates/ff-rdp-cli/src/commands/click.rs`'s `autowait_element` polls the top-level document for a
selector match up to `wait_timeout_ms` (defaulting to `cli.timeout`, 10s). The poll exists because
a selector that has not rendered *yet* — content behind a pending XHR, a route transition — needs
to be retried, and that is the correct behavior for a selector that will eventually match. The
defect is that the same budget is spent on a selector that provably never will: nothing in the DOM
resembles it and nothing is still loading. The fix has to distinguish those two cases, not just
shrink the timeout (which would make the legitimate "not rendered yet" case flaky).

## Themes

- **A — detect the stable-and-empty case early.** If the page has reached a stable/idle state
  (no pending network activity `click` already tracks for `--wait-for-network`, no DOM mutations
  in the last poll interval) and the selector still matches zero elements, there is nothing left
  to wait for — report not-found without spending the rest of the budget. A selector that matches
  late because of an in-flight fetch must still get its full timeout; only the case where the page
  is provably done changing should short-circuit.
- **B — do not regress the retry case.** Every existing live test that relies on `autowait_element`
  retrying a selector that appears after a delay must still pass unmodified — this is a
  short-circuit on top of the existing poll, not a replacement for it.
- **C — measure the actual saving.** Time the dogfood reproduction above before and after; record
  both numbers in the plan rather than asserting the fix worked from the diff alone.

## Tasks

### A. Stable-and-empty short-circuit [0/3]
- [ ] Define "stable" precisely (reuse whatever `settle_page`/network-idle signal `click` already
      computes for `--wait-for-network`, rather than inventing a second notion of idle)
      firstly check `crates/ff-rdp-cli/src/commands/click.rs` for the existing signal before adding
      one
- [ ] Wire the short-circuit into `autowait_element`'s poll loop, gated so it only fires once the
      page is stable, never before
- [ ] Unit test: a selector that appears after 2 polls still resolves (retry case unregressed);
      a selector that never appears on a page that goes stable at poll N reports not-found at
      poll N+1, not at the full timeout

### B. Live coverage [0/2]
- [ ] Live Firefox test: guessed selector against a static page (immediately stable) resolves
      not-found in well under `--timeout`
- [ ] Live Firefox test: selector that appears after a deliberate delay (dynamically inserted via
      `eval`) still resolves successfully within the existing timeout budget — the regression case
      Theme B exists to prevent

### C. Measure [0/1]
- [ ] Re-run this plan's `dogfood_path` reproduction before and after; record both wall-clock
      numbers here

## Acceptance Criteria [0/3]

- [ ] A selector that matches nothing on a page that has gone stable reports not-found in
      measurably less than `--timeout` (record the before/after numbers, not just "faster")
- [ ] Every existing live test covering `autowait_element`'s retry behavior (a selector that
      appears late) still passes unmodified
- [ ] `cargo run -p xtask -- live-sweep` clean with both env gates set

## Out of scope

- Lowering the default `--timeout`. That trades away the legitimate retry case for every caller,
  not just the guessed-selector case this plan targets.
- Any change to how `click --ref` resolves (it does not poll a selector at all — refs are already
  a stable page-view handle, so this defect does not apply to the act-and-see idiom that
  [[iteration-230-quickstart-navigate-with-page]] pushed adoption toward).

## References

- [[iteration-228-two-task-benchmark-after-facts]] — first observation, "deliberately not acted on"
- [[iteration-230-quickstart-navigate-with-page]] — carry-over row repeating the disposition
- [[iteration-231-infobox-facts-refs-and-query-matching]] — carry-over row repeating it a third time
