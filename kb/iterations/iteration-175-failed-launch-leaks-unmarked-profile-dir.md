---
title: "Iteration 175: a launch that fails before its owner marker is written leaks an unattributable, unreclaimable profile dir"
type: iteration
date: 2026-08-17
status: planned
branch: iter-175/failed-launch-leaks-unmarked-profile
depends_on: [iteration-171-stale-owner-pid-marker-and-pid-reuse]
first_call_sites: []
dogfood_path: |
  # Product defect. The observable is a managed profile dir containing only
  # `user.js` — no owner-PID marker, no owner-test marker, no Firefox
  # artefacts — which no prune path will reclaim for seven days.

  # 1. Show the leak exists in the wild.
  ROOT=$(ff-rdp profiles list --jq -r '.results.path')
  for d in "$ROOT"/ff-rdp-profile-*; do
    test -e "$d/.ff-rdp-owner-pid" || echo "UNMARKED: $d  ($(ls -a "$d" | wc -l) entries)"
  done
  # → OBSERVED 2026-08-17 during iteration 171's Theme A: 20 of 20 directories
  #   under the real profile root were unmarked and contained only `user.js`.

  # 2. Force one deterministically: make the spawn fail after build_command has
  #    already created the profile dir and written user.js. A --debug-port that
  #    is already held, or a --profile the process cannot write into, both
  #    reach the same error paths.
  ff-rdp launch --headless --debug-port <port held by something else>
  ls "$ROOT" | wc -l    # before and after

  # 3. Show that nothing reclaims it inside the age gate.
  ff-rdp profiles prune --older-than 1h --dry-run --jq '.results.would_remove'
  # → EXPECTED: the freshly leaked dir is absent, because an unmarked dir falls
  #   back to the mtime heuristic and it is minutes old.

  # 4. Measure how often this fires in a real sweep: count unmarked dirs before
  #    and after one `cargo run -p xtask -- live-sweep`.
tags: [iteration, profiles, launch, cleanup]
---

# Iteration 175: a failed launch leaks an unmarked profile directory

Carry-over from [[iteration-171-stale-owner-pid-marker-and-pid-reuse]] Theme A5.

## What was observed

While measuring iteration 171's Theme A, the real per-user profile root held 20
`ff-rdp-profile-*` directories. Every one of them contained exactly one file, `user.js`, and
**no** `.ff-rdp-owner-pid`. Their mtimes clustered in four groups over a 30-minute window, which
matches concurrent agents running `ff-rdp launch` rather than one pathological session.

A directory with no owner marker falls through to the iter-96 mtime heuristic, which is a 7-day
gate by default. So each of these survives a week, and `ff-rdp doctor`'s 100-entry / 1 GiB warning
is the only thing that ever notices.

## Why iteration 171 did not fix it

Iteration 171 moved the owner-marker writes from *after* the port probe to *immediately after the
spawn*, which shrinks the unmarked window from "tens of seconds under contention" to "the time
between `spawn()` returning and the next statement". That is a large improvement but not a
closure: `build_command` creates the profile directory and writes `user.js` *before* the spawn, so
every failure between those two points — `find_firefox` succeeding but `spawn` failing, a
`--debug-port` collision detected after the profile exists, the process being killed in that
window — still leaks an unattributable directory.

## Themes

- **A — Confirm the mechanism and size it.** Run the `dogfood_path`. Establish which error paths
  actually leak (spawn failure, immediate-exit, probe failure, caller killed) and how many
  directories one live sweep produces. If the sweep produces none, the 20 observed came from
  interactive use and the priority changes — say so.
- **B — Decide between two shapes, do not pick silently.** Either (i) make `build_command`'s
  profile directory RAII-owned so any early return removes it, which is the honest fix but has to
  survive `launch`'s fire-and-forget success path where the directory must *not* be removed; or
  (ii) write the owner marker with the *launching CLI's own PID* before the spawn and overwrite it
  with Firefox's PID after, so an unmarked directory becomes impossible and the dead-owner sweep
  reclaims a failed launch on the next `launch`. Option (ii) reuses machinery that already exists
  and needs iteration 171's start token to be safe.
- **C — Backfill.** Decide whether `profiles prune` should treat an unmarked directory that
  contains only `user.js` as provably-failed regardless of age, and whether that is a new flag or
  the default.

## Tasks

### A. Verify
- [ ] Run every step of `dogfood_path` and paste actual outputs into this plan
- [ ] Enumerate which `launch` error paths leave the profile dir behind, by reading the code and
      forcing each one
- [ ] Count unmarked dirs produced by one full live sweep

### B. Fix
- [ ] The chosen shape, with the rejected alternative recorded and its failure mode named
- [ ] Unit tests over the error paths (no real Firefox — `launch`'s hooks already allow stubbing)
- [ ] A live test that fails on `main` and passes on the branch

### C. Backfill
- [ ] Decide and record whether existing unmarked directories are reclaimed, and under what rule

## Acceptance Criteria [0/4]

- [ ] Theme A's enumeration is recorded here, including any error path that turns out NOT to leak
- [ ] A forced launch failure leaves no `ff-rdp-profile-*` directory behind — asserted by a test
      that fails on `main`
- [ ] No age-gated prune behaviour is loosened for directories that are merely *old* (the 7-day
      gate stays; only provably-failed directories may be reclaimed early)
- [ ] `cargo fmt && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace -q`
      clean, plus a dual-gate live sweep

## Out of scope

- Re-litigating iteration 171's identity token. It closes a different failure (a marker that
  exists but names the wrong process); this one is about a marker that never gets written.
- The `.ff-rdp-owner-test` attribution work iteration 171 did for the live suite.

## References

- [[iteration-171-stale-owner-pid-marker-and-pid-reuse]] — Theme A5, where these 20 directories
  were found, and the marker-write move that shrank but did not close the window
- [[iteration-96-profile-store-hygiene]] — the mtime heuristic that governs unmarked directories
- [[iteration-142-disk-growth]] — the dead-owner immediate-reclaim rule an unmarked dir cannot use
