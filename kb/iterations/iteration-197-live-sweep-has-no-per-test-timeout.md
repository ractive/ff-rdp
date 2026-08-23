---
title: "Iteration 197: a single hung live test hangs the whole sweep, forever"
type: iteration
date: 2026-08-23
status: planned
branch: iter-197/live-sweep-per-test-timeout
depends_on: [kb/iterations/iteration-188-live-sweep-cost-and-parallelism.md]
first_call_sites: []
dogfood_path: |
  # 1. Reproduce the shape (no Firefox needed): libtest reports a slow test and
  #    then waits for it with no bound of its own.
  cargo run -p xtask -- live-sweep --dry-run        # see the plan/concurrency split
  # A hung test prints exactly one line and nothing further ever arrives:
  #   test <name> has been running for over 60 seconds
  # …and the sweep produces no LIVE_SWEEP_SUMMARY at all.

  # 2. Observed for real, 2026-08-23, iteration 188's third sweep (--jobs 4):
  #    live_158_launch_lifecycle::live_158_launch_survives_contended_bind
  #    276 of 277 CLI-tier tests reported; the log froze at 18:54 and was still
  #    frozen 20+ minutes later, holding four live Firefox processes open.
  #    The run had to be abandoned on its outer 60-minute harness timeout.

  # 3. What "fixed" looks like: a hung phase is killed at a stated bound and
  #    reported as a failure with the test named, and the sweep still prints a
  #    LIVE_SWEEP_SUMMARY with `total` conserved.
  FF_RDP_LIVE_TESTS=1 cargo run -p xtask -- live-sweep --jobs 6
tags: [iteration, testing, live-tests, tooling, xtask, carry-over]
---

# Iteration 197: the sweep has every timeout except the one that matters

## Where this came from

[[iteration-188-live-sweep-cost-and-parallelism]]'s Theme C, which made the CLI live tier run
concurrently. Its third sweep (`--jobs 4`) hung on
`live_158_launch_lifecycle::live_158_launch_survives_contended_bind` after 276 of 277 tests, and
never recovered: libtest printed `has been running for over 60 seconds` and then waited forever,
because **libtest has no per-test timeout at all**. The whole sweep produced no summary line, and
the four Firefox processes the hung test had launched stayed alive.

This is not a parallelism defect — a serial sweep hangs exactly the same way, and has (iteration
146's postmortem chased orphaned browsers from a sweep that had to be interrupted). Parallelism
only makes it likelier, because the hung test is one that deliberately contends for ports while
five siblings are launching browsers of their own.

## Why it matters more now

`new-ralph-loop` runs iterations unattended. A sweep that exits red costs one iteration; a sweep
that hangs costs the rest of the night, and leaves orphaned browsers that poison every later run
(the run-wide rule "never kill a sweep mid-run" exists precisely because interrupting one is
destructive).

## The two candidate fixes, and the honest trade

1. **A watchdog inside `live_sweep::run_phase`.** Give each phase a deadline, kill the child
   *process group* when it expires (killing `cargo test` alone leaves the test binary and its
   Firefox children alive — that is the part that needs care), and report the phase as failed
   with whatever libtest had printed so far. Keeps one test runner and one output format; every
   accounting guarantee in `live_sweep.rs` is written against libtest's prose.
2. **`cargo nextest`,** which runs each test in its own process and already has
   `slow-timeout` + `terminate-after`. Iteration 188 declined it to avoid re-deriving
   `classify_failures`/`failure_blocks` against a second output format, and said so; the hang is
   the strongest argument on the other side. Costs: a required dev tool, a second failure-output
   parser, and re-proving the `executed`/`skipped`/`preexisting`/`vanished`/`launch_timeout`
   tiers against it.

Pick one **with the accounting as the acceptance test**, not the wall clock.

## Also worth establishing

Why `live_158_launch_survives_contended_bind` hangs at all. It spawns four concurrent
`ff-rdp launch --headless` on fixed ports 7101-7104 and joins their threads. `launch` has its own
bounded port wait (`FF_RDP_LIVE_LAUNCH_TIMEOUT_SECS`, 30 s), so an indefinite hang means
something ahead of that bound is blocking — a pre-spawn occupancy check against a port held by an
orphan, or a `Command::output()` whose child never closes its pipes. That diagnosis belongs here,
because a per-test timeout that fires every run is a worse gate than no timeout at all.

## Tasks

### A. Diagnose [0/2]
- [ ] Reproduce the hang and identify which of the four launches blocks, and on what
- [ ] State whether the fix belongs in the test, in `launch`, or in the sweep

### B. Bound it [0/3]
- [ ] A stated per-phase (or per-test) bound, with the bound written down and justified against
      the p99 test time measured in iteration 188 (38.2 s)
- [ ] A hung phase is reported as a failure naming the test, and the sweep still prints
      `LIVE_SWEEP_SUMMARY` with `total` conserved
- [ ] Whatever the bound kills leaves no orphaned Firefox behind — verified with
      `pgrep -f 'MacOS/firefox.*ff-rdp-profile'`, not with the checker that matches itself

## Acceptance Criteria [0/3]

- [ ] A sweep containing a deliberately hung test terminates within the stated bound and exits
      non-zero, naming the test
- [ ] The runner choice (watchdog vs nextest) is argued in this plan against the accounting
      guarantees, not only against wall clock
- [ ] No orphaned `ff-rdp`-managed Firefox survives a timed-out sweep

## Out of scope

- Making `live_158_launch_survives_contended_bind` faster. It is a contention test; it is
  supposed to be slow.
- Changing the concurrency iteration 188 chose.

## References

- [[iteration-188-live-sweep-cost-and-parallelism]] — where the hang was observed, and why the
  tier is concurrent
- [[iteration-155-live-skip-reports-green]] — the accounting any new runner must preserve
- [[iteration-173-live-sweep-port-6000-firefox-does-not-survive]] — the tier accounting in detail
