---
title: "Iteration 171: a leaked profile dir's owner-PID marker outlives its process, so PID reuse makes dead profiles read as live"
type: iteration
date: 2026-08-16
status: planned
branch: iter-171/stale-owner-pid-marker
depends_on: [iteration-168-livefirefox-drop-does-not-wait-for-exit]
first_call_sites: []
dogfood_path: |
  # Test-harness/product-boundary defect. The observable is a false-positive
  # liveness read on a profile whose owning process is long gone.

  # 1. Show that a dropped LiveFirefox leaves its profile directory behind.
  #    Count ff-rdp-profile-* dirs before and after one live test.
  ff-rdp profiles list --jq '.results.path'
  ROOT=$(ff-rdp profiles list --jq -r '.results.path')
  ls "$ROOT" | grep -c '^ff-rdp-profile-'
  FF_RDP_LIVE_TESTS=1 cargo test -p ff-rdp-cli --test live -q -- --ignored \
    --exact live_128_network_output_fidelity::live_128_meta_route
  ls "$ROOT" | grep -c '^ff-rdp-profile-'
  # → EXPECTED: the count grows by one, and the new dir still contains a
  #   .ff-rdp-owner-pid naming a process that no longer exists.

  # 2. Show the marker is stale, not just present: read the pid and confirm it
  #    is dead.
  for d in "$ROOT"/ff-rdp-profile-*; do
    pid=$(cat "$d/.ff-rdp-owner-pid" 2>/dev/null) || continue
    kill -0 "$pid" 2>/dev/null && echo "$d: pid $pid ALIVE" || echo "$d: pid $pid dead"
  done

  # 3. Force the false positive. Reuse is the hazard, so simulate it rather
  #    than waiting for the OS: write a *live* unrelated pid into a dead
  #    profile's marker and confirm `profiles prune --all` then refuses it.
  #    (Do this in a scratch profile root, never the real one.)

  # 4. Measure how fast this machine actually recycles PIDs — how many process
  #    spawns, and how much wall clock, before a given pid comes round again.
  #    That sizes the real-world exposure over a ~40-minute live sweep.
tags: [iteration, testing, live-tests, harness, profiles]
---

# Iteration 171: stale owner-PID markers and PID reuse

Carry-over from [[iteration-168-livefirefox-drop-does-not-wait-for-exit]] Theme A3 and Theme C.
Iteration 168 fixed a real defect (`LiveFirefox::drop` signalled without waiting; measured window
16–27 ms) but its Theme A measurements **disproved its own causal claim**: that window cannot
explain the `live_96_profile_cleanup` precondition failure iter-165 observed, because 176 tests
run between `live_128_meta_route` and `live_96` in the sequential live binary. This iteration
picks up the explanation iteration 168 left open.

## What is actually suspected

`LiveFirefox::drop` kills the process but leaves the `ff-rdp-profile-*` directory on disk, and
that directory contains `.ff-rdp-owner-pid`. The marker therefore outlives the process it names,
for the rest of the sweep and beyond. `live_96_profile_cleanup`'s precondition — and
`profiles prune`'s own liveness check — ask `kill(pid, 0)`, which cannot distinguish "the process
that wrote this marker" from "whatever process now holds that pid". Once the OS recycles the pid,
a dead profile reads as live-owned and `prune --all` correctly refuses to remove it.

This matches every property of the iter-165 observation that iteration 168's mechanism did not:
it survives arbitrary wall-clock distance between the two tests, it is rare, and it gets likelier
the more processes the run spawns — which is exactly what "load average 18.6" describes.

### Added 2026-08-17 — a second, non-hypothetical way the marker goes stale

Reproduced by accident while re-running the sweep on `main` at `4d639e2`: a sweep **killed
mid-test** orphans that test's browsers outright, because `LiveFirefox::drop` never runs. A run
terminated during `live_158_launch_survives_contended_bind` left four Firefox processes alive for
over an hour. They were still holding their profile dirs when the *next* sweep ran, and broke it
twice — `live_158` failed with `port 7101 is already in use by firefox (PID 66554)`, and
`live_96_profile_cleanup` failed its precondition naming all four dirs.

Two things this adds to the plan, both cheap to act on:

- the false positive does **not** require PID reuse to bite. A killed runner leaves genuinely-live
  processes owning profile dirs that no test will ever clean up, which is the same end state and
  far more likely than pid recycling on a developer machine;
- **the owner-test marker did not name anything.** All four read `spawned by unknown test`, so
  iter-151 Theme A's marker — the thing that exists to name the culprit — does not survive the
  process being killed rather than dropped. Whatever Theme B does about staleness should also make
  the marker durable at *launch* time, not at drop time.

A clean sweep on the same commit, with those orphans cleared first, was 269 passed / 1 failed with
`live_96_profile_cleanup` **green** — so nothing here contradicts iteration 168's fix; it means
the remaining exposure needs either contention or an interrupted run to surface.

**Unverified.** iteration 168 measured the 16–27 ms window but did not measure PID recycling, and
did not force the false positive. Theme A below does both before anything is changed.

## Themes

- **A — Confirm or kill the hypothesis.** Run the `dogfood_path`. Establish (i) that dropped
  `LiveFirefox` instances really do leave markered directories behind, (ii) that a forced pid
  collision really does trip the precondition, and (iii) how fast this machine recycles pids in
  process-spawn and wall-clock terms. If a pid cannot plausibly recycle inside one sweep, this
  hypothesis is wrong — say so and look elsewhere (start with: did that Firefox actually die?).
- **B — Make the marker self-invalidating.** `kill(pid, 0)` is not an identity check. Options to
  weigh explicitly, not silently: record process start time alongside the pid and compare both;
  have `LiveFirefox::drop` remove its own profile directory once the process is confirmed gone
  (iteration 168 Theme C decided this is unhandled, not deliberate); or have the product's
  liveness check verify the process is actually a Firefox it owns. Whichever is chosen, the
  reasoning for rejecting the others belongs in the plan.
- **C — Decide where the fix lives.** The marker is written by the product
  (`util::profile_dir`) and read by both the product (`profiles prune`) and the harness
  (`live_96`, `live_151`, `live_168`). A harness-only fix leaves real users exposed to the same
  false positive; a product-side fix is a wider blast radius. Pick one and say why.

## Tasks

### A. Verify
- [ ] Run every step of `dogfood_path` and paste actual outputs into this plan
- [ ] Record whether a dropped `LiveFirefox` leaves a markered profile dir, with counts
- [ ] Record the forced-collision result: does a live pid in a dead profile's marker trip
      `live_96`'s precondition and `prune --all`?
- [ ] Record this machine's PID recycle rate (spawns and wall clock)

### B. Fix
- [ ] The chosen invalidation mechanism, with the rejected alternatives recorded
- [ ] Unit tests that do not require a real Firefox
- [ ] A live test that fails on `main` and passes on the branch, the way `live_168` does

### C. Placement
- [ ] Record whether the fix is harness-side or product-side, and why

## Acceptance Criteria [0/4]

- [ ] The Theme A verification is recorded in this plan, including the decision that follows if
      the hypothesis does not hold
- [ ] A stale marker naming a recycled pid no longer reads as a live owner — asserted by a test
      that fails on `main`
- [ ] `live_96_profile_cleanup`'s precondition is left as loud as iter-146 Theme B made it (no
      softening back into a skip)
- [ ] `cargo fmt && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace -q`
      clean, plus a dual-gate live sweep

## Out of scope

- Re-litigating iteration 168's bounded wait. It closed a separately measured defect and stays.
- Softening `live_profiles_prune_removes_all_when_no_firefox_running`'s precondition. Same reason
  as in iteration 168: iter-146 Theme B removed the skip on purpose.

## References

- [[iteration-168-livefirefox-drop-does-not-wait-for-exit]] — Theme A3 (why the 16–27 ms window
  cannot explain iter-165) and Theme C (the profile dir is left behind, and that is unhandled
  rather than deliberate)
- [[iteration-165-eval-scope-leak-contradicts-help]] — the sweep that surfaced the original
  `live_96` failure
- [[iteration-151-residual-live-firefox-leak]] — `OWNER_TEST_MARKER`, which names the culprit test
  in the failure message
- [[iteration-146-live-suite-reliability]] — Theme B, the loud precondition that detects this
