---
title: "Iteration 201: which of the live tier's 200+ launch call sites could run against a recorded fixture instead?"
type: iteration
date: 2026-08-23
status: planned
branch: iter-201/live-tests-onto-recorded-fixtures
depends_on: [kb/iterations/iteration-188-live-sweep-cost-and-parallelism.md, kb/iterations/iteration-09-live-fixture-recording.md]
first_call_sites: []
dogfood_path: |
  # Survey, not conversion. Nothing here changes what a test asserts.

  # 1. Inventory: how many live tests already have a fixture-backed
  #    equivalent, and how many touch protocol surface no fixture covers yet.
  ls crates/ff-rdp-core/tests/fixtures/*.json | wc -l
  grep -rl "MockServerHandle" crates/ff-rdp-cli/tests/e2e/ | wc -l
  grep -rlE 'LiveFirefox::(headless_on_random_port|launch)' crates/ff-rdp-cli/tests/live/ | wc -l

  # 2. For one concrete candidate test, run it live and note exactly which
  #    Firefox behaviors it depends on beyond "the RDP actor replies with the
  #    shape we expect" — DOM timing, real network, a real renderer quirk.
  #    Those are the tests fixtures cannot replace; the inventory in Task A
  #    must separate them from tests that only check envelope/protocol shape.
tags: [iteration, testing, live-tests, tooling, fixtures, design, carry-over]
---

# Iteration 201: the re-tiering iteration 188 declined to attempt

## Where this came from

[[iteration-188-live-sweep-cost-and-parallelism]] made the live tier faster without changing its
size. Its "Out of scope" section named the alternative lever and why 188 did not take it:

> **Moving live tests onto recorded fixtures.** Discussed 2026-08-17: 141 fixtures and a
> `MockServerHandle` already exist, and only 3 e2e files consume them. That is a larger
> re-tiering — and it cannot catch what a live test catches when Firefox itself changes. Separate
> plan; this one keeps every test live.

That "already exist" is load-bearing: [[iteration-09-live-fixture-recording]] built the recording
infrastructure and `MockServerHandle` long before 188, and per the project's own fixture-recording
rule (`.claude/CLAUDE.md`), fixtures are recorded from real Firefox, never hand-crafted — so a
fixture-backed test still validates against real Firefox behavior, just not on every run. The gap
188 flagged is that only 3 e2e files use that machinery today, against ~200 live-tier launch call
sites.

## Why this needs its own plan, and why it is a survey first

A live test and a fixture-backed test answer different questions. A live test catches "Firefox
changed and our assumption broke" (the exact failure mode
[[iteration-155-live-skip-reports-green]] exists to keep visible). A fixture-backed test only
catches "our code stopped matching a Firefox behavior we already recorded" — real regressions in
*our* parsing/handling, but not drift in Firefox itself. Converting a test that exists specifically
to catch Firefox drift into a fixture replay would quietly delete the thing it was for. This plan's
first and largest task is therefore classification, not conversion.

## The question to answer

Of the live tier's ~280 tests, which ones assert something that is:

1. **Protocol/envelope shape only** — the RDP actor replied with fields X/Y/Z in a known
   structure. A recorded fixture of that exact reply answers the same question, replayed, with no
   Firefox process at all.
2. **Firefox-behavior-dependent** — timing, real rendering, real network, a quirk that only a live
   browser reproduces. These must stay live; converting them would be exactly the mistake iteration
   155 was filed to prevent.
3. **Ambiguous** — looks like (1) today but could silently start asserting (2) if the code under
   test changes without the test author noticing. These need a written rule for who decides, not a
   default.

## Tasks

### A. Classify the tier [0/2]
- [ ] Every live test tagged (1)/(2)/(3) above, with the one-line reason, cross-referenced against
      the fixtures that already exist in `crates/ff-rdp-core/tests/fixtures/`
- [ ] For category (1) tests with no existing fixture, estimate the recording cost (one
      `FF_RDP_LIVE_TESTS_RECORD=1` run adds fixtures for every new live test added at once, per
      `.claude/CLAUDE.md`'s recording workflow — so the cost is mostly "write the live test once,
      recorded is free")

### B. Convert a pilot set [0/2]
- [ ] Pick 5-10 category-(1) tests, convert them to `MockServerHandle` + recorded fixture, and
      confirm each still fails the same way when the *code* regresses (mutate the handler under
      test, confirm red; revert, confirm green) — a fixture test that cannot fail is worse than no
      test
- [ ] Measure the wall-clock and cold-start-count delta the pilot set removes from the live sweep

### C. Decide the boundary [0/2]
- [ ] A written rule for category (3) — e.g. "if converting would delete the test's ability to
      catch a *Firefox* regression, it stays live even if it looks fixture-shaped today"
- [ ] Whether to convert beyond the pilot, and if so, in what order (highest cold-start-count wins
      first is the model 188 used for prioritizing what to measure)

## Acceptance Criteria [0/3]

- [ ] The classification in Task A covers every test file under `crates/ff-rdp-cli/tests/live/`,
      not a sample — "we didn't get to the rest" is a legitimate outcome for *conversion*, not for
      the classification itself
- [ ] Every converted test in the pilot demonstrably still catches a code regression (red/green
      pair recorded in the PR), not just "it passes"
- [ ] No test whose purpose is catching Firefox drift is converted — Task C's rule is applied, not
      assumed

## Design notes

This is explicitly a survey-then-pilot plan, not a rewrite of the live tier. If Task A's
classification finds that most of the tier is category (2) or (3), the honest outcome is a small
pilot and a written boundary rule — not a forced conversion to hit a wall-clock target. Speed is
[[iteration-188-live-sweep-cost-and-parallelism]]'s job; this plan's job is not to trade away what
[[iteration-155-live-skip-reports-green]] exists to guarantee.

## Out of scope

- Deleting the recording infrastructure or `MockServerHandle` — both already exist and are kept.
- Reusing a live Firefox process across tests — that is
  [[iteration-200-live-firefox-reuse-across-tests]], a different lever (still live, just fewer cold
  starts) that this plan's fixture conversions are independent of.
- Any change to what `live-sweep`'s accounting tiers mean — a converted test simply leaves the live
  tier's population; it does not change `executed`/`skipped`/`preexisting`/`vanished`.

## References

- [[iteration-188-live-sweep-cost-and-parallelism]] — declined this lever, named it, deferred it here
- [[iteration-09-live-fixture-recording]] — built the recording infrastructure this plan reuses
- [[iteration-155-live-skip-reports-green]] — why a live test exists at all; the boundary this plan
  must not cross
