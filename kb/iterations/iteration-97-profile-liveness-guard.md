---
title: "Iteration 97: owner-PID liveness guard for profile pruning"
type: iteration
date: 2026-07-03
status: planned
branch: iter-97/profile-liveness-guard
depends_on:
  - iteration-96-profile-leak-cleanup
firefox_refs: []
kb_refs:
  - kb/iterations/iteration-96-profile-leak-cleanup.md
first_call_sites:
  - primitive: launch writes .ff-rdp-owner-pid into the managed profile dir after spawn
    site: crates/ff-rdp-cli/src/commands/launch.rs
  - primitive: profile_is_owned_by_live_process guard consulted by both prune paths
    site: crates/ff-rdp-cli/src/util/profile_dir.rs
dogfood_script: iteration-97-profile-liveness-guard.dogfood.sh
tags:
  - iteration
  - launch
  - cleanup
  - safety
---

# Iteration 97 — owner-PID liveness guard for profile pruning

Iter-96's review (PR #133, local reviewer, High finding) established that
every age-gated pruning path — `launch`'s automatic orphan sweep and
`profiles prune --older-than` — judges staleness from mtimes alone. Iter-96
hardened the signal to "newest top-level file mtime" (a live Firefox rewrites
file contents constantly), which closes the practical window, but it is still
a *heuristic*: a headless Firefox that is completely idle past the threshold
(no pref writes, no sqlite checkpoints, no cert/telemetry churn for 7+ days)
could in principle still have its live profile deleted out from under it.

This iteration replaces the heuristic with a positive ownership signal: the
PID of the Firefox process that owns the profile, checked for liveness before
any automatic deletion.

## Themes

### A. `launch` records the owner PID in the profile

Immediately after spawning Firefox, `launch` writes `.ff-rdp-owner-pid`
(plain-text PID, newline-terminated) into the managed profile directory it
created. Only managed (`ff-rdp-profile-*`) dirs get the marker — never a
`--profile <user-path>` dir.

### B. Prune paths skip profiles whose owner PID is alive

New helper `profile_is_owned_by_live_process(&Path) -> bool` in
`util/profile_dir.rs`: reads `.ff-rdp-owner-pid`, returns `true` iff it
parses and `process::is_process_alive(pid)` (the daemon-registry primitive)
holds. Consulted by `prune_orphan_profiles` AND `select_prune_targets`
*before* the mtime heuristics; a live owner always wins. A missing or
unparsable marker falls back to the iter-96 mtime heuristics (pre-97 profiles
have no marker). PID-reuse false-positives err toward *keeping* a directory —
the safe direction; the mtime heuristic still reclaims it eventually once the
recycled PID dies.

### C. `--all` keeps its documented sharp edge, but warns

`profiles prune --all` remains "no age gate", but when it is about to remove
a profile whose owner PID is alive it prints a warning line per directory
(and still removes it — `--all` is the explicit escape hatch, documented as
such in iter-96). `daemon stop`'s `cleanup_profile_dir` is unaffected: it
only ever removes the profile of the Firefox it just confirmed dead.

## Pre-fix repro

- `pre_fix_repro_prune_deletes_profile_with_live_owner_pid` — seed a managed
  profile dir with fully back-dated mtimes (dir + all files) AND an
  `.ff-rdp-owner-pid` naming the *current test process* PID; run
  `prune_orphan_profiles` at a 7-day threshold; on pre-fix code assert the
  dir is deleted (demonstrating the heuristic gap), post-fix assert it
  survives.

## Tasks

### Theme A — owner-PID marker [0/2]

- [ ] In `crates/ff-rdp-cli/src/commands/launch.rs`, after a successful
      spawn of Firefox with a managed temp profile, write
      `.ff-rdp-owner-pid` (the child PID) into the profile dir.
      Warn-not-fail: a write failure must never fail the launch.
- [ ] `unit_owner_pid_marker_written_only_for_managed_profiles`: a
      `--profile <user-path>` launch never receives a marker.

### Theme B — liveness guard in prune paths [0/3]

- [ ] Add `profile_is_owned_by_live_process` to `util/profile_dir.rs`;
      consult it first in `prune_orphan_profiles` and
      `commands::profiles::select_prune_targets`.
- [ ] Land `pre_fix_repro_prune_deletes_profile_with_live_owner_pid`
      (see Pre-fix repro).
- [ ] `unit_prune_skips_live_owner_but_reclaims_dead_owner`: marker with
      the test's own PID → skipped; marker with a known-dead PID (spawn
      and wait a child) → pruned when mtimes are stale.

### Theme C — `--all` warning [0/2]

- [ ] `profiles prune --all` logs a warning per live-owner directory it
      removes; JSON output gains `removed_live: [basenames]`.
- [ ] `unit_prune_all_reports_live_owner_dirs`: seeded live-marker dir
      appears in `removed_live` and is removed.

## Acceptance Criteria [0/5]

- [ ] `pre_fix_repro_prune_deletes_profile_with_live_owner_pid`: post-fix,
      a live-owner profile survives the launch sweep at any age.
- [ ] `unit_owner_pid_marker_written_only_for_managed_profiles`: user
      `--profile` dirs never receive a marker.
- [ ] `unit_prune_skips_live_owner_but_reclaims_dead_owner`: dead-PID
      markers do not block reclamation.
- [ ] `unit_prune_all_reports_live_owner_dirs`: `--all` surfaces
      live-owner removals in `removed_live`.
- [ ] `dogfood_script_full_run_iter_97`: `.dogfood.sh` launches Firefox,
      verifies its profile survives a forced prune sweep while running and
      is reclaimed after `daemon stop`; exits 0.

## Out of scope

- **Locking-based detection** (`parent.lock` / flock probing) — platform
  divergent (Windows exclusive locks vs Unix symlink/fcntl), superseded by
  the PID marker which ff-rdp fully controls.
- **Cross-host markers** — profile roots are per-user, per-host state dirs;
  a PID from another host is out of the threat model.

## References

- [[iteration-96-profile-leak-cleanup]] — the mtime-heuristic hardening this
  replaces with a positive signal; PR #133 review finding (High).
- `crates/ff-rdp-cli/src/util/profile_dir.rs` (`latest_profile_activity`,
  the iter-96 heuristic that becomes the fallback).
