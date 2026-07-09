---
title: "Iteration 110: post-batch full live-suite sweep — run everything against real Firefox once, fix all fallout"
type: iteration
date: 2026-07-09
status: planned
branch: iter-110/post-batch-live-sweep
depends_on:
  - kb/iterations/iteration-109-network-throttle-block.md
  - kb/iterations/iteration-106-live-test-masking-cascade.md
firefox_refs: []
kb_refs:
  - kb/iterations/iteration-100b-live-test-consolidation.md
first_call_sites: []
dogfood_path: |
  # The sweep itself IS the dogfood: full gated live suite against headless Firefox.
  FF_RDP_LIVE_TESTS=1 FF_RDP_LIVE_NETWORK_TESTS=1 cargo test-live
  # Post-condition: zero failures that are attributable to iterations 101-105.
tags:
  - iteration
  - tests
---

# Iteration 110 — post-batch full live-suite sweep

## Execution policies (2026-07-09, per James)

The full live-suite run IS this iteration's core job — no live-test
restriction applies here. Scoped testing still applies to the fix work: while
iterating on a fix, run only the tests it touches; one full
`cargo test --workspace -q` in the final pre-PR gates, and the review agent
does not re-run the full workspace suite. CI-wait: required lanes only; if
[[iteration-108-windows-ci-preexisting-reds]] merged earlier in this batch,
`test (windows-latest)` should be green and any windows failure is real.

## Motivation

Per James's 2026-07-09 decision, iterations 102–105 and 106–109 do NOT run the full live
Firefox suite per-iteration (it dominated wall-clock: 20–40 min per run, often
run twice per iteration by implement + review agents). Each of those
iterations still runs its own dogfood script and the specific live tests named
in its ACs — only the *full-suite* pass is deferred to here, once, after
iteration 105 merges.

## Theme A — one full sweep

Run `FF_RDP_LIVE_TESTS=1 FF_RDP_LIVE_NETWORK_TESTS=1 cargo test-live` (the
consolidated `live` target from [[iteration-100b-live-test-consolidation]])
against current main. Record the complete pass/fail inventory in this plan's
Results section.

## Theme B — fix the fallout

For every failure:
- If caused by an iteration in the 101–109 range: fix it in this iteration.
- If pre-existing environmental (the 19 reds catalogued during iter-100b,
  tracked in [[iteration-106-live-test-masking-cascade]]): leave to iter-106,
  but cross-reference it in the inventory.
- New live tests introduced by 101–109 whose full-suite interaction was never
  exercised (port contention, daemon-registry sharing, buffer state leaking
  between modules in the consolidated binary) are in scope here.

## Acceptance criteria

- [ ] full_sweep_recorded: complete `cargo test-live` inventory (pass/fail per
      test) attached to Results, run on post-109 main.
- [ ] no_101_105_regressions: every failure attributable to iterations
      101–105 is fixed and its test passes in a re-run; fixes carry their own
      unit/live tests where behaviour changed.
- [ ] preexisting_reds_crossref: remaining failures are each cross-referenced
      to iter-106 (or a filed follow-up), none left untracked.

## Results

(to be filled by the implementing iteration)
