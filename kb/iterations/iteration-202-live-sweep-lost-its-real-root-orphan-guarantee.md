---
title: "Iteration 202: restore the whole-run guarantee that a live sweep leaves no owned profile in the real per-user root"
type: iteration
date: 2026-08-23
status: planned
branch: iter-202/live-sweep-real-root-orphan-guarantee
depends_on: [kb/iterations/iteration-188-live-sweep-cost-and-parallelism.md, kb/iterations/iteration-146-live-suite-reliability.md]
first_call_sites: []
dogfood_path: |
  # A whole-sweep check, so exercise it through the sweep itself, not a
  # single test.
  FF_RDP_LIVE_TESTS=1 FF_RDP_LIVE_NETWORK_TESTS=1 cargo run -p xtask -- live-sweep --jobs 6
  # → expect the new post-phase-1 check to run and report clean on a machine
  #   with no other ff-rdp-managed Firefox alive.

  # Then prove it actually catches something: leave one Firefox running
  # under the real root on purpose and re-run.
  ff-rdp launch --headless --debug-port 7999 --jq '.results.pid'
  FF_RDP_LIVE_TESTS=1 FF_RDP_LIVE_NETWORK_TESTS=1 cargo run -p xtask -- live-sweep --jobs 6
  # → expect the new check to name the leftover profile/PID, not a bare
  #   pass, and expect the sweep to still complete (not hang or panic) on
  #   this condition.
  ff-rdp --port 7999 daemon stop
tags: [iteration, testing, live-tests, tooling, xtask, carry-over]
---

# Iteration 202: the guarantee iteration 188's review deleted the test for

## Where this came from

Reviewing [[iteration-188-live-sweep-cost-and-parallelism]]'s PR (#223), `live_96_profile_cleanup`'s
`live_profiles_prune_removes_all_when_no_firefox_running` was found to have become dead weight:
188 gave it its own isolated `$FF_RDP_HOME` so its "no ff-rdp-managed Firefox is running anywhere"
precondition could be satisfied under the tier's new concurrency — but once isolated, nothing else
ever writes into that root, so the precondition can never fire and the test's remaining behavior
(seed unowned dirs, `prune --all`, assert removed) became a strict duplicate of
`tests/e2e/profiles.rs::profiles_prune_is_scoped_to_ff_rdp_home`. It was deleted from the live tier
in that review rather than kept as a live-Firefox-gated no-op.

That deletion is correct on its own terms — the test could never have failed for the reason its
name and doc comment claimed — but it means the guarantee the old (pre-188) test actually stood
for is now asserted **nowhere**: that a completed live-sweep run leaves no live-owned
`ff-rdp-profile-*` directory behind in the *real* per-user profile root (not an isolated one). That
guarantee traces to [[iteration-146-live-suite-reliability]] Theme B, which made the precondition
loud specifically because a quiet skip had let real leaks go unnoticed.

## Why this needs a sweep-level check, not a test-level one

The old test could assert the real-root property because, pre-188, it ran serially and could
assume "no sibling test is using the real root right now." Post-188 that assumption is false for
any *single* test — but it is still true and checkable at exactly one point: **after phase 1 of
`live-sweep` completes**, every self-launching test has finished (successfully cleaned up via
`daemon stop`/`Drop`, per [[iteration-146-live-suite-reliability]] and
[[iteration-168-livefirefox-drop-does-not-wait-for-exit]]) or is accounted for in the failure set.
That is the sweep's own vantage point, which `crates/xtask/src/live_sweep.rs` already has and no
individual test can reconstruct.

## The question to answer

Can `live-sweep` add a post-phase-1 check — "scan the real per-user profile root
(`secure_profile_root()` with no `$FF_RDP_HOME` override) for `ff-rdp-profile-*` directories with a
live owner-PID marker; report any as a named finding" — without:

1. Coupling `xtask` (which does not currently depend on `ff-rdp-cli`'s internals) to
   `crate::util::profile_dir`'s private marker format. Decide whether that means duplicating the
   marker-reading logic (as the live tests already do, per `live_96_profile_cleanup.rs`'s and
   `live_151_residual_leak.rs`'s own doc comments about this exact duplication) or exposing a
   narrow `pub` read-only helper from `ff-rdp-cli` for `xtask` to call.
2. Producing a false positive against a profile some *other*, unrelated ff-rdp invocation on the
   same machine legitimately owns (e.g. a developer's own interactive session running during the
   sweep) — the check must distinguish "leaked by this sweep" from "somebody else's business,"
   which the old test's design already had to solve once (see `live_146` and `live_171`'s owner-PID
   markers) and this reuses, but the *sweep* has less context than a single test about which PIDs
   are "its own."
3. Making `live-sweep`'s summary line or exit code ambiguous — decide whether a finding here is a
   new named failure category (joining `executed`/`skipped`/`preexisting`/`vanished`/
   `launch_timeout`) or a separate warning that does not affect the pass/fail verdict, and say why.

## Tasks

### A. Design the check [0/2]
- [ ] Decide the marker-reading approach (duplicate vs. expose a helper) and record the trade-off
- [ ] Decide whether a finding fails the sweep, warns, or both — and update the
      `LIVE_SWEEP_SUMMARY` line's documented shape if it changes

### B. Implement and prove it catches something [0/2]
- [ ] The check runs after phase 1, scans the real root only (never a `$FF_RDP_HOME`-isolated one —
      scanning those would be meaningless, they are always empty when their owning test exits
      cleanly)
- [ ] A reproduction: deliberately leave a live-owned profile in the real root, run `live-sweep`,
      confirm the check names it (per this plan's `dogfood_path`); clean up, re-run, confirm clean

## Acceptance Criteria [0/2]

- [ ] `live-sweep` run against a real root with one deliberately-left live-owned profile reports it
      by name (directory + PID), not silently
- [ ] `live-sweep` run against a clean real root reports no finding, and existing accounting
      (`executed`/`skipped`/`preexisting`/`vanished`/`launch_timeout`/`total`) is unchanged by this
      addition

## Out of scope

- Re-adding the deleted `live_96` live test. The e2e test it duplicated stays as the coverage for
  "prune removes unowned managed dirs" — this plan is only about the whole-suite real-root claim,
  which is a different property.
- Fixing anything the check might find on a real machine (that would be a fresh leak investigation,
  not this plan).
- **Walking the whole `$FF_RDP_HOME` chain in `root_is_trustworthy`.** Iteration 188's PR review
  (`profile_dir.rs`'s `root_is_trustworthy`) added an ownership+mode check on the profile root
  itself but does not vet `$FF_RDP_HOME` or `$FF_RDP_HOME/ff-rdp` above it — a writable parent lets
  another account `rename()` the vetted leaf away and substitute one it owns that still passes.
  Documented as a precondition instead ("`$FF_RDP_HOME` must itself be a directory only you can
  write" — `profile_dir.rs`'s doc comment and `README.md`'s `FF_RDP_HOME` bullet). Noted here as the
  tracking location per that review's own suggestion; pick this up if a task ever needs the
  precondition enforced rather than merely documented.

## References

- [[iteration-188-live-sweep-cost-and-parallelism]] — where the isolated test that used to stand in
  for this guarantee was deleted, in PR review, as dead weight
- [[iteration-146-live-suite-reliability]] — Theme B, why the precondition was made loud in the
  first place
- [[iteration-168-livefirefox-drop-does-not-wait-for-exit]] — the cleanup guarantee this check
  would be verifying held, at sweep scale
