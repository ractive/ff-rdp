---
branch: iter-155/live-skip-reports-green
date: 2026-08-13
depends_on:
  - kb/iterations/iteration-154-ac-fidelity-evidence.md
dogfood_path: |
  # A live test whose env gate is unset must NOT be counted as passed.
  # Before: reports `ok` and contributes to a green summary.
  cargo test -p ff-rdp-cli --test live -- --include-ignored live_109 2>&1 | tail -3
  # After: reports `ignored` (or fails loudly), and the count of executed tests
  # is visible in the summary rather than inferred.
first_call_sites: []
status: in-review
title: "Iteration 155: a skipped live test reports green, so a green live sweep can mean 'did not run'"
type: iteration
tags:
  - discipline
  - testing
  - live-tests
---

# Iteration 155: a skipped live test reports green

Filed out of [[iteration-154-ac-fidelity-evidence]], which fixed the *gate* half of the
green-means-nothing problem and explicitly scoped this half out: it is the same failure family
but a different mechanism in different code (the test harness, not the discipline gate), and
folding it into a hand-driven skill-edit iteration would have turned that iteration into a
test-harness refactor.

## The defect

Live tests gate themselves twice: `#[ignore]` (so they are skipped unless `--include-ignored`),
and an **early `return`** when `FF_RDP_LIVE_TESTS` / `FF_RDP_LIVE_NETWORK_TESTS` is unset. The
second gate is the problem. libtest counts a test that returns without panicking as **passed**,
not ignored. So:

- `FF_RDP_LIVE_TESTS=1 cargo test-live` runs every `#[ignore]` test, and every test that
  additionally needs `FF_RDP_LIVE_NETWORK_TESTS` returns immediately and reports `ok`.
- The summary line reads `N passed; 0 failed`, which is indistinguishable from `N` tests having
  actually exercised Firefox.

Measured statically 2026-08-13: **94 env-var early-return sites across 45 files** under
`crates/ff-rdp-cli/tests/live/`, of which **69 bail via a bare `return;`**, plus 3 sites in
`crates/ff-rdp-core/tests/`. iteration-154's notes record a dynamic measurement from 2026-08-12
— nine files / eighteen tests silently no-op under `FF_RDP_LIVE_TESTS=1` alone, including the
then-known-red `live_109_throttle_block::live_block_url_pattern`. **Re-measure before fixing:**
across iterations 135–151 the stated root cause diverged from reality at least eight times, and
the static and dynamic counts above disagree enough that one of them is being read wrong.

This is what makes iteration-154's Theme B annotation forgeable *in good faith*: an agent that
runs `cargo test-live`, sees `109 passed / 0 failed`, and pastes that into
`[verified: <date>, 109 passed / 0 failed]` has done everything asked of it and still recorded a
number that may describe tests which never ran.

## Themes

### Theme A — a skipped test must not report as passed

Replace the bare `return` with something libtest counts as *not run*. Options, to be decided in
the iteration and recorded in [[decision-log]]:

1. `eprintln!` + `return` — status quo plus noise. Rejected in advance: still green.
2. Drop the runtime check and rely on `#[ignore]` alone, so the operator's `--include-ignored`
   choice is the only gate. Simplest, but tests needing `FF_RDP_LIVE_NETWORK_TESTS` would then
   fail (not skip) on a network-less run.
3. A shared `skip_unless_live!()` macro that panics with a distinctive message, plus a harness
   convention that treats it as a skip. Loud, but turns a skip into a failure.
4. Nightly `#[feature(test)]` / a custom harness that can report "ignored" at runtime.

The decision hinges on a question worth answering first: **is a network-less live run supposed to
be green?** If yes, the current behaviour is arguably correct and the real fix is Theme B alone.
If no, option 2 is the honest one. Do not implement before answering.

### Theme B — make the executed count visible, not inferred

Whatever Theme A concludes, a live sweep should end with a line stating how many tests actually
reached Firefox. A `--jq`-able summary the `[verified: …]` annotation can quote verbatim would
close the loop iteration-154 left open, and is useful even if Theme A changes nothing.

### Theme C — teach the gate what a real live number looks like

`ac-fidelity-check.sh`'s Theme B accepts any `[verified: <date>, <digits>]`. If Theme B produces
a machine-readable executed-count, the gate could require the annotation to quote *that* format.
Weigh honestly: this couples a shell gate to a test-output format and may not be worth it. Say so
in the Resolution if the conclusion is no.

## Resolution

**Is a network-less live run supposed to be green?** Yes for the tests that don't need the
network — no for the summary line's implicit claim that *all* of them ran. Neither of the plan's
four Theme A options answers both halves at once: options 1 and 4 don't change the outcome or
aren't available on stable, and options 2/3 turn "skip" into "fail" for a case (a contributor
running `FF_RDP_LIVE_TESTS=1` without network fixtures wired) that is not a defect.

**Theme A + B, implemented together**: `cargo run -p xtask -- live-sweep`
(`crates/xtask/src/live_sweep.rs`) — a new tool, not a change to the ~90 test bodies. It
classifies every `#[ignore]`-gated live test from its own ignore-reason text, then runs `cargo
test` in two phases: tests whose required env var(s) are set run for real with
`--include-ignored` (libtest reports genuine `ok`/`FAILED`); the rest are named explicitly
*without* `--include-ignored`, so `#[ignore]` alone keeps them from running and libtest reports
them `ignored` — its own vocabulary. The unqualified test's body, and the early-`return` inside
it, is never reached at all. It prints `LIVE_SWEEP_SUMMARY executed=N skipped=M total=T`, with
`executed` known from classification before any subprocess spawns. Full write-up:
[[decision-log#DEC-031]].

**Theme C: no.** `ac-fidelity-check.sh` reads a plan and a diff — it has no persisted run log to
check a `[verified: …, executed=N]` annotation against, because nothing in the current pipeline
invokes `live-sweep` (or `cargo test-live`) and keeps the output. Requiring the annotation to
quote `executed=N` would add a second string format to parse without adding verification power —
an agent can fabricate `executed=17` exactly as easily as `109 passed / 0 failed`. Worth
revisiting once a persisted run-artifact store exists for the gate to check against; that is a
different iteration. See [[decision-log#DEC-031]].

## Acceptance Criteria [3/3]

- [x] test_155_skipped_live_test_is_not_counted_passed: a live test whose env gate is unset is
      reported as ignored/skipped rather than `ok`, asserted by parsing libtest's own summary
      output in a unit or integration test — not by reading the code. Implemented as an
      integration test in `crates/xtask/src/live_sweep.rs` that classifies the real
      `live_109_throttle_block::live_block_url_pattern` test (the one named in this plan's own
      `dogfood_path`), partitions it as unqualified with the network gate unset, spawns
      `cargo test -p ff-rdp-cli --test live` for real without `--include-ignored`, and asserts
      libtest's stdout contains `test live_109_throttle_block::live_block_url_pattern ...
      ignored` and never `... ok`.
- [x] test_155_executed_count_is_reported: a live sweep emits a machine-readable count of tests
      that actually reached Firefox, and a test asserts the count is 0 when the env gates are unset.
      `live_sweep::run` prints `LIVE_SWEEP_SUMMARY executed=N skipped=M total=T`; the
      `test_155_executed_count_is_reported` unit test asserts `executed == 0` with both env gates
      unset and rises deterministically as gates are set, computed from static classification
      alone before any `cargo test` process spawns.
- [x] check_155_baselines_unmoved: `cargo run -p xtask -- check-discipline-regression` still
      reports `61v=FAIL, 61t=PASS` and all mirrors in sync. Re-run after this change:
      `check-discipline-regression: ralph-loop mirror in sync (3 files); new-ralph-loop mirror in
      sync (5 files); replay baselines OK (61v=FAIL, 61t=PASS)` — unchanged, since this iteration
      never touches `ac-fidelity-check.sh` or its mirrors (see [[decision-log#DEC-031]] Theme C).

## Notes

- Re-measure the dynamic counts before touching code; see the warning above.
- This plan changes test-harness scaffolding across ~45 files. Expect the diff to be wide and
  mechanical; keep the behavioural change in one shared helper so the per-file change is a
  one-liner.
- Not a skill-edit iteration — this one *can* run through the loop, unlike
  [[iteration-154-ac-fidelity-evidence]].
