---
title: "Iteration 199: is Spotlight indexing the profiles root a measurable sweep cost, or a superstition?"
type: iteration
date: 2026-08-23
status: obsolete
branch: iter-199/spotlight-indexing-ab
depends_on:
  - kb/iterations/iteration-188-live-sweep-cost-and-parallelism.md
first_call_sites: []
dogfood_path: |
  # This is a measurement iteration. The A/B is one env var now that iter-188
  # Theme B landed: FF_RDP_HOME can point the profiles root at an unindexed
  # path with no product change.
  
  # 1. Confirm the indexed path is actually indexed, and the unindexed one isn't.
  mdutil -s / | head -3
  mkdir -p /private/tmp/ff-rdp-199-probe && touch /private/tmp/ff-rdp-199-probe/x
  mdfind -onlyin /private/tmp/ff-rdp-199-probe x
  # → expect: / enabled; the /private/tmp probe returns nothing.
  
  # 2. A — indexed (today's default; do NOT set FF_RDP_HOME):
  FF_RDP_LIVE_TESTS=1 FF_RDP_LIVE_NETWORK_TESTS=1 \
    cargo run -p xtask -- live-sweep --jobs 6
  # repeat 3x on a quiet machine, record wall clock + failure set + `uptime`/`ps` snapshot
  
  # 3. B — unindexed (profiles root under /private/tmp via FF_RDP_HOME):
  FF_RDP_HOME=/private/tmp/ff-rdp-199-home \
    FF_RDP_LIVE_TESTS=1 FF_RDP_LIVE_NETWORK_TESTS=1 \
    cargo run -p xtask -- live-sweep --jobs 6
  # repeat 3x on the same quiet machine, same measurements
  
  # → compare A's and B's wall-clock distributions against the run-to-run noise
  #   band iteration 188 already measured (256-342 s at --jobs 6). If B's mean
  #   sits inside that band, the theme is closed with "no measurable effect".
tags:
  - iteration
  - testing
  - live-tests
  - tooling
  - performance
  - carry-over
---

# Iteration 199: Theme D from iteration 188, actually measured this time

## Where this came from

[[iteration-188-live-sweep-cost-and-parallelism]] Theme D asked whether Spotlight indexing the
profiles root (`~/Library/Application Support/ff-rdp/profiles` on macOS, confirmed indexed via
`mdutil -s /` and `mdfind -onlyin`) is a measurable contributor to the live sweep's run-to-run
wall-clock variance. Immediately after one `--jobs 6` run, load average was 99 on a 10-core box
with `mds_stores` at 45.9% and `mds` at 20.7%.

Theme D was **not run** in 188: every candidate quiet window on that machine was consumed by a run
that either carried failures worth chasing on their own ([[iteration-198-live-tests-red-only-under-concurrency]])
or hung outright ([[iteration-197-live-sweep-has-no-per-test-timeout]]), and the theme's own rule —
do not publish a superstition — forbids acting on Spotlight without a comparison against the noise
band. This iteration is that comparison, now that it can be done for the cost of one environment
variable: [[iteration-188-live-sweep-cost-and-parallelism]] Theme B gave `secure_profile_root()` an
`$FF_RDP_HOME` override, and `/private/tmp` is confirmed unindexed on macOS.

## The question to answer

Does moving the profiles root off the Spotlight-indexed path change the live sweep's wall clock or
failure rate by more than the run-to-run noise iteration 188 already measured (256-342 s at
`--jobs 6`, 0-3 failures per run at any concurrency)?

- **If yes** (B's runs are consistently faster/more stable, outside the noise band): act on it —
  either document `FF_RDP_HOME=/private/tmp/...` as the recommended sweep setup, or have
  `live-sweep` default the profiles root off the indexed path itself when no override is set.
- **If no** (B sits inside the band A already showed): close the theme explicitly, in this plan and
  by ticking Theme D's boxes in 188, with the comparison numbers as the receipt. A closed theme
  with data beats an open one with a plausible story.

Do not accept "it's obviously the indexing" without the paired runs. Iteration 188 already showed
that a single run's headline (`-j6`, 427 s, one failure) was wrong until three more runs corrected
it — the same discipline applies here.

## Tasks

### A. Confirm the premise [0/1]
- [ ] `mdutil -s /` shows indexing enabled for the real profiles root, and `mdfind -onlyin
      /private/tmp/<probe>` returns nothing for a file seeded there, on the machine this A/B runs on
      (indexing status is per-machine and per-volume; do not assume iteration 188's finding travels)

### B. Paired measurement [0/2]
- [ ] Three `--jobs 6` sweeps with the profiles root on its default (indexed) path, on a quiet
      machine, each orphan-checked before and after per iteration 188's protocol
      (`pgrep -f 'MacOS/firefox.*ff-rdp-profile'`)
- [ ] Three `--jobs 6` sweeps with `FF_RDP_HOME` pointed at an unindexed `/private/tmp` path, same
      machine, same protocol, run interleaved with the A runs (not all-A-then-all-B) to avoid
      conflating indexing with whatever else changes over the course of an hour on a dev machine

### C. Verdict [0/2]
- [ ] State the two wall-clock distributions (not just means) and whether B falls outside A's band
- [ ] Act on the result (recommend/default the unindexed path) or close the theme explicitly with
      the numbers — either outcome ticks this task, a shrug does not

## Acceptance Criteria [0/3]

- [ ] `iteration_199_ab_indexing`: the plan (this file) records six real sweep runs (3 indexed, 3
      not) with wall clock, failure set and an orphan check for each — no run described without
      having been executed, per this run's standing constraints
- [ ] The verdict names a concrete effect size (seconds, or "inside the ±40 s noise band already on
      record") — not "seemed faster"
- [ ] [[iteration-188-live-sweep-cost-and-parallelism]]'s Theme D task checkboxes are updated to
      point at this plan's result once it lands, so a reader of 188 does not have to guess whether
      D was ever closed

## Design notes

The A/B does not require a product change to run — `FF_RDP_HOME` already does the redirection. A
product change (defaulting `live-sweep` or `secure_profile_root()` off the indexed path) is only
warranted if Task C's verdict says the effect is real; do not pre-build it speculatively.

`mdutil -i off` on the real profiles root was considered and rejected as the A/B mechanism: it
requires `sudo` on most machines, is a global per-volume/per-directory setting that outlives this
experiment and could surprise whoever runs it next, and would not be reproducible on CI or another
contributor's machine the way an env var is.

## Out of scope

- Any change to what the sweep asserts. This is a measurement iteration, same as Theme A of 188.
- Fixing anything in [[iteration-197-live-sweep-has-no-per-test-timeout]] or
  [[iteration-198-live-tests-red-only-under-concurrency]] — those are their own plans, filed from
  the same source iteration, and their failure signatures must not be allowed to contaminate this
  A/B. If a quiet run cannot be had without one of them firing, wait for a machine/window where it
  can, rather than publishing a comparison with a known confound in it (188 Theme D's own mistake).

## References

- [[iteration-188-live-sweep-cost-and-parallelism]] — Theme D, filed unmeasured; this plan is that
  measurement
- [[iteration-197-live-sweep-has-no-per-test-timeout]] — a confound to avoid, not a subject here
- [[iteration-198-live-tests-red-only-under-concurrency]] — same

## Closed as obsolete (2026-08-23)

Filed as a carry-over from iteration 188's review. This is a **performance idea, not a defect** —
nothing is broken and no test is failing on it. Closed during the post-run backlog prune to keep
the open set to real defects. Re-open by setting `status: planned` if the sweep's wall clock
becomes a constraint again.
