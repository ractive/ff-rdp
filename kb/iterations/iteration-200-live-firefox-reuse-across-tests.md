---
title: "Iteration 200: can the live tier reuse one Firefox across several tests instead of paying a cold start each?"
type: iteration
date: 2026-08-23
status: planned
branch: iter-200/live-firefox-reuse
depends_on: [kb/iterations/iteration-188-live-sweep-cost-and-parallelism.md, kb/iterations/iteration-151-residual-live-firefox-leak.md, kb/iterations/iteration-168-livefirefox-drop-does-not-wait-for-exit.md, kb/iterations/iteration-171-stale-owner-pid-marker-and-pid-reuse.md]
first_call_sites: []
dogfood_path: |
  # This iteration starts as a design/measurement pass, not a rewrite. Before
  # writing a line of pooling code, establish the floor and the ceiling.

  # 1. The floor without reuse (already measured, iter-188 A1): 5.64 s +/-
  #    0.02 per cold start, ~200 launch call sites in the CLI tier.

  # 2. What a *shared-profile, shared-process* test actually costs once
  #    Firefox is already up — i.e. the part reuse would leave behind.
  ff-rdp launch --headless --debug-port 7900 --jq '.results.pid'
  for i in 1 2 3 4 5; do
    /usr/bin/time -p ff-rdp --port 7900 navigate "https://example.com"
  done
  ff-rdp --port 7900 daemon stop
  # → records the per-test floor once cold start is out of the picture.

  # 3. Does reusing a profile change any test's observable result? Diff two
  #    runs of the same live test against a fresh profile vs a reused one:
  FF_RDP_LIVE_TESTS=1 cargo test -p ff-rdp-cli --test live live_129 -- --include-ignored --exact
  # then repeat pointed at a Firefox with prior navigation history/cookies and
  # compare — this is the question Task A must answer before Task B is safe.
tags: [iteration, testing, live-tests, tooling, performance, design, carry-over]
---

# Iteration 200: the larger prize iteration 188 declined to chase

## Where this came from

[[iteration-188-live-sweep-cost-and-parallelism]] made the live tier's *outer* execution parallel
(several Firefox processes at once) but left each test paying its own **cold start** — measured at
5.64 s ± 0.02, ~200 call sites, ~1134 s of implied total, roughly half the pre-188 serial wall
clock. 188's own "Out of scope" section named the next lever explicitly:

> **Browser reuse across tests.** The measured floor says a test cannot go below ~5.6 s without
> it, so it is the larger prize — but it changes ownership lifetime, which is the subject of
> iterations 151, 168 and 171, and it would invalidate their guarantees. It needs its own plan,
> after this one establishes whether parallelism alone is enough.

188 has now landed (8.1× at `--jobs 6`) and established that parallelism alone gets a large win
without touching ownership. This plan is the "own plan" 188 deferred to.

## Why this is hard, not just an optimization

Three existing guarantees are built on "each live test owns exactly one Firefox process, spawned by
that test, for that test's lifetime":

- [[iteration-151-residual-live-firefox-leak]] — cleanup on drop assumes a 1:1 test:process mapping.
- [[iteration-168-livefirefox-drop-does-not-wait-for-exit]] — `Drop` waits for *the* process this
  handle spawned to actually exit, not "a" process that happens to be listening.
- [[iteration-171-stale-owner-pid-marker-and-pid-reuse]] — the owner-PID marker names *the*
  spawning test, and iteration 188's `ownership_scan_roots()` (Theme B) leans on that marker being
  unambiguous. A shared browser has no single owning test by construction.

A reuse scheme has to either preserve "exactly one logical owner at a time" (e.g. a pool that hands
one Firefox to one test at a time, never two concurrently) or explicitly renegotiate what "owner"
means for a shared process — and the second option is a bigger change than it sounds, because
`pid_is_ff_rdp_spawned` and the prune/kill paths gate lethal actions on that marker.

## The question to answer

Is there a reuse design that keeps "one test owns this Firefox at a time" (so 151/168/171's
guarantees hold unmodified), and if so, does it actually save wall clock once profile/state
isolation between reusing tests is accounted for (a shared profile can leak cookies, history, and
permission prompts between tests unless each acquisition resets it — which itself costs time)?

## Tasks

### A. Establish whether cross-test state leakage is a real risk [0/2]
- [ ] Enumerate what a live test currently assumes about a *fresh* profile (cookies, permissions,
      navigation history, service workers) by reading the live tier, not by guessing
- [ ] Measure the cost of resetting that state between acquisitions (new profile dir vs. clearing
      an existing one vs. a fresh `about:blank` navigation) against the 5.64 s cold-start floor —
      if reset costs approach the cold start, reuse buys nothing

### B. Design a pool that preserves single ownership [0/3]
- [ ] A pool/handle design where exactly one test holds a given Firefox process at a time — no
      concurrent readers — so [[iteration-171-stale-owner-pid-marker-and-pid-reuse]]'s marker
      scheme needs no redefinition
- [ ] How pool exhaustion behaves under `--jobs 6`: more concurrent tests than pooled browsers
      must not deadlock or silently fall back to serial without saying so
- [ ] How [[iteration-168-livefirefox-drop-does-not-wait-for-exit]]'s exit-wait guarantee applies
      to a process that is *returned to the pool* instead of killed — what "this test is done with
      it" means when the process outlives the test

### C. Measure the actual win [0/2]
- [ ] A prototype (even a subset of the tier) run at `--jobs 6` with and without reuse, three clean
      runs each, same protocol iteration 188 used (orphan-checked, failure set recorded)
- [ ] State the wall-clock delta plainly — including "smaller than expected because reset costs
      ate most of it" as a legitimate, fundable-to-report outcome

## Acceptance Criteria [0/3]

- [ ] The design task (B) is complete and reviewed *before* any pooling code lands — this plan is
      allowed to conclude "not worth it" and stop at Task A or B without writing Task C's code
- [ ] If implemented, no test's isolation guarantee (fresh cookies/permissions/history per test) is
      weakened without that test explicitly opting in and saying why
- [ ] The wall-clock claim is backed by three clean runs, per iteration 188's own rule against
      picking a number from one run

## Design notes

This plan deliberately does not commit to "build a pool" as its outcome. Task A is a real off-ramp:
if resetting shared state costs nearly as much as a cold start, reuse is not the lever 188 thought
it might be, and this plan's honest outcome is "measured, and it's not worth it" — which is exactly
the shape [[iteration-188-live-sweep-cost-and-parallelism]] Theme D warns against skipping (do not
publish a superstition; measure or say you didn't).

## Out of scope

- Rewriting the live tier's test structure to share fixtures/state deliberately (that is
  [[iteration-201-live-tests-onto-recorded-fixtures]], a different lever entirely — recorded
  fixtures over a live browser, not a pooled live browser).
- Changing `--jobs`'s default concurrency — orthogonal to whether each job's browser is reused.

## References

- [[iteration-188-live-sweep-cost-and-parallelism]] — measured the cold-start floor and named this
  as the larger prize it declined to chase
- [[iteration-151-residual-live-firefox-leak]] — the cleanup guarantee a pool must not break
- [[iteration-168-livefirefox-drop-does-not-wait-for-exit]] — the exit-wait guarantee a pool must
  redefine deliberately, not accidentally
- [[iteration-171-stale-owner-pid-marker-and-pid-reuse]] — the single-owner marker scheme a pool
  must preserve or explicitly renegotiate
