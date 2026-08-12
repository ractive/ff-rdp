---
branch: iter-150/core-longstring-flake
date: 2026-08-12
depends_on: []
dogfood_path: |
  for i in 1 2 3 4 5 6 7 8 9 10; do
    cargo test -p ff-rdp-core -q specs::types::tests::resolve_slot_longstring_grip_fetches_full_value
  done
  # → 10 consecutive runs must pass; today the test fails intermittently and
  #   passes on rerun, which is how it has survived two separate sightings
first_call_sites: []
status: planned
title: "Iteration 150: resolve_slot_longstring_grip_fetches_full_value is intermittently red"
type: iteration
tags: [iteration]
---

# Iteration 150: `resolve_slot_longstring_grip_fetches_full_value` is intermittently red

Filed from two independent sightings during the 138–146 batches, neither of which was in scope
for the iteration that saw it. No dogfooding session — evidence inline.

## Why this is worth an iteration rather than a shrug

`crates/ff-rdp-core` — `specs::types::tests::resolve_slot_longstring_grip_fetches_full_value`
fails, then passes on rerun. It has now been observed twice by different agents in different
iterations:

- during [[iteration-141-output-hygiene]]'s review pass ("one pre-existing flaky ff-rdp-core
  test … observed once, unrelated to this change, and passed on rerun")
- during [[iteration-146-live-suite-reliability]]'s gate run ("reproduced once then passed on
  rerun, documented in the plan as out-of-scope")

Two sightings across two batches is not noise. Each time it was correctly ruled out of scope for
the iteration in hand, and each time that judgement was right — which is exactly how an
intermittent survives indefinitely: always someone else's problem.

It also matters more than a typical flaky unit test. This is **not** a live test: it is a plain
`cargo test -p ff-rdp-core` unit test, so it runs on every CI job on every PR across three
platforms. An intermittent there is a random red on unrelated work, and the established response
in this repo has become "rerun it" — the habit [[iteration-146-live-suite-reliability]] warned
about, of training readers to ignore red.

## What is not yet known

The mechanism is unidentified. Do not assume it is a timing/threading issue just because it is
intermittent — the run guidance that corrected four plan hypotheses across 138–146 (two of them
in iteration-146, written from a symptom that looked conclusive) applies with full force here.

Candidate directions, none confirmed:

- a longstring grip resolution path with an ordering assumption that holds only sometimes
- shared mutable state or a fixture reused across tests in the same binary, making it
  order-dependent rather than genuinely random (check by running the test alone in a loop versus
  the full `-p ff-rdp-core` suite in a loop — if it only fails in-suite, it is contamination, not
  timing)
- a real bug in `resolve_slot`'s handling of a partially-delivered longstring, in which case the
  test is correctly reporting a product defect and must not be "stabilised"

That last possibility is the reason this plan forbids the obvious cheap fix.

## Themes

### Theme A — reproduce deterministically

Get from "fails sometimes" to "fails on demand" before changing any product code. A loop harness
that runs the test N times, and separately the whole `ff-rdp-core` suite N times, is the minimum;
record the observed failure rate for each in this plan.

### Theme B — root-cause and fix

Fix the actual mechanism. If it turns out to be a product bug in longstring grip resolution, the
fix belongs in `ff-rdp-core` and the test stays as-is. If it is test-local contamination, fix the
test's isolation.

**Explicitly forbidden**: `#[ignore]`, a retry wrapper, a sleep, or loosening the assertion. Any
of those converts a visible intermittent into an invisible one — the same trade
[[iteration-146-live-suite-reliability]] Theme C refused.

## Acceptance Criteria [0/3]

- [ ] unit_150_longstring_deterministic_repeat:
      `resolve_slot_longstring_grip_fetches_full_value` passes 50 consecutive runs in isolation
      and 20 consecutive runs of the full `-p ff-rdp-core` suite
- [ ] unit_150_mechanism_documented: this plan's Resolution section names the confirmed
      mechanism and the pre-fix failure rate measured in Theme A, not a hypothesis
- [ ] unit_150_regression_pinned: if the cause was a product bug, a test exercises the specific
      longstring path deterministically; if it was test contamination, a test or harness change
      makes the isolation failure impossible rather than unlikely

## Notes

- If Theme A cannot reproduce the failure at all after a substantial run budget, say so and
  re-defer rather than guessing at a fix — an unreproducible intermittent that resists a
  deliberate hunt is better left visible and documented than "fixed" speculatively. Compare
  [[iteration-144-session-hygiene-followup]] Theme D, which shipped a mitigation for an
  unreproduced symptom; prefer the [[iteration-147-console-locale-repro]] treatment (honest re-defer)
  when the symptom will not appear.
- Cheap first step before anything else: check whether the two recorded sightings share a
  platform, a parallelism setting, or a neighbouring test.
