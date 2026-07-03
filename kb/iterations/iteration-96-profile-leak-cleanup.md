---
title: "Iteration 96: temp-profile leak — daemon stop cleans up, launch prunes orphans, profiles subcommand"
type: iteration
date: 2026-06-02
status: planned
branch: iter-96/profile-leak-cleanup
depends_on:
  - iteration-95-session-60-followups
firefox_refs: []
kb_refs:
  - kb/dogfooding/dogfooding-session-60.md
first_call_sites:
  - primitive: daemon stop removes record.profile_dir after SIGKILL escalation
    site: crates/ff-rdp-cli/src/daemon/client.rs
  - primitive: launch prunes ff-rdp-profile-* dirs older than N days before creating its own
    site: crates/ff-rdp-cli/src/commands/launch.rs
  - primitive: ff-rdp profiles {list,prune} subcommand
    site: crates/ff-rdp-cli/src/commands/profiles.rs
dogfood_script: iteration-96-profile-leak-cleanup.dogfood.sh
tags:
  - iteration
  - daemon
  - launch
  - cleanup
  - leak
  - disk
---

# Iteration 96 — temp-profile leak

`ff-rdp launch` (without `--profile`) creates a fresh `~/Library/Application Support/ff-rdp/profiles/ff-rdp-profile-<16 random bytes>/` and calls `.keep()` on the `TempDir` — explicitly persisting the directory past the `launch` process's exit. The code comment at `crates/ff-rdp-cli/src/commands/launch.rs:244-245` claims *"the existing cleanup path on process exit remains in effect"* — **but that path does not exist anywhere in the codebase**. `daemon stop` doesn't remove the profile dir. No background sweeper runs. Every launch leaks one ~28 MB directory forever.

Discovered 2026-06-02: a single user's `~/Library/Application Support/ff-rdp/profiles/` had **1,715 directories totalling 48 GB**, accumulated over ~8 days of normal development + dogfooding + ralph-loop. Cleaned up manually in this session; this iter ensures it doesn't refill.

## Themes

### A. `daemon stop` removes `record.profile_dir` after SIGKILL escalation (closes the common-path leak)

The daemon record already persists `profile_dir` (`crates/ff-rdp-cli/src/daemon_record.rs:55`). After the iter-95 SIGTERM→SIGKILL→killpg ladder completes successfully, remove the profile directory if and only if it's under the secure-profile-root parent (never `--profile <user-path>`). Common case — graceful `ff-rdp daemon stop` — now reclaims its own profile.

### B. `ff-rdp launch` prunes orphan `ff-rdp-profile-*` siblings older than N days (catches crashes, kill -9, reboots)

Before `tempdir_in(&profile_root)`, walk `profile_root`, list entries matching `ff-rdp-profile-*`, and `remove_dir_all` any whose `mtime` is older than `FF_RDP_PROFILE_PRUNE_DAYS` (default 7). Bounded work: stops scanning after the first N old entries per launch so a 1700-dir backlog doesn't pause the next launch by 30s. The active profile (created by the current process) is never a candidate — it doesn't exist on disk yet at this point.

### C. `ff-rdp profiles {list,prune}` subcommand + `doctor` size check

Explicit knob for the user (or for `doctor`) to surface and reclaim. Subcommands:
- `ff-rdp profiles list [--format text]` — show profile_root path, count, total size, oldest mtime.
- `ff-rdp profiles prune [--older-than 7d] [--all] [--dry-run]` — explicit prune, default 7 days, `--all` deletes everything not held by a running Firefox, `--dry-run` lists what would go.

And add a `doctor` check `profile_disk_usage` that emits `warn` when `profiles/` exceeds 1 GB or 100 entries (the empirical "you have a problem" thresholds from session-60-era data).

## Pre-fix repro

- A: `pre_fix_repro_daemon_stop_removes_active_profile`
  — launch → daemon stop → assert the spawned profile directory no longer exists.
- B: `pre_fix_repro_launch_prunes_stale_orphan_profiles`
  — pre-seed `profile_root` with 3 fake `ff-rdp-profile-aaaa…` dirs whose mtime is 8 days old, then `launch`; assert the 3 stale dirs are gone, the new one is present.
- C: `pre_fix_repro_profiles_subcommand_prune_dry_run_lists_orphans`
  — seed orphans, run `profiles prune --dry-run`, assert it lists them without deleting.

## Hard rule

Three themes. Each ships its own pre-fix repro + unit test. **No theme cross-talk** — A touches the daemon stop path (already touched by iter-94/95), B touches launch's pre-tempdir setup, C is an entirely new module + clap subcommand. If any theme breaks the others, the fix belongs in a separate iter.

## Tasks

### Theme A — daemon stop removes active profile [4/4] [pre_fix_repro_test: pre_fix_repro_daemon_stop_removes_active_profile]

- [x] In `crates/ff-rdp-cli/src/daemon/client.rs`, after the existing
      SIGTERM→SIGKILL→killpg ladder reports success (port freed AND
      process gone), call `cleanup_profile_dir(&record.profile_dir)`.
- [x] Add `cleanup_profile_dir` helper in
      `crates/ff-rdp-cli/src/util/profile_dir.rs` that only removes the
      path if (a) the path is under
      `secure_profile_root()` and (b) the basename matches
      `^ff-rdp-profile-[A-Za-z0-9]{16}$`. **Never** remove a directory
      the user passed via `--profile`. Log the removal at debug level;
      `daemon stop` JSON output gains a `profile_removed: true|false`
      field.
- [x] Land `pre_fix_repro_daemon_stop_removes_active_profile` as a
      live test (`FF_RDP_LIVE_TESTS=1`): launch → capture
      `profile_path` from the launch JSON → daemon stop → assert
      `!profile_path.exists()` AND launch JSON's `profile_path`
      equals daemon stop JSON's removed path.
- [x] `unit_cleanup_profile_dir_refuses_path_outside_profile_root`:
      pass `/tmp/some-user-dir`; assert no removal and no error
      (silent skip + log).

### Theme B — launch prunes orphan profiles [4/4] [pre_fix_repro_test: pre_fix_repro_launch_prunes_stale_orphan_profiles]

- [x] In `crates/ff-rdp-cli/src/commands/launch.rs`, before
      `tempdir_in(&profile_root)`, call a new
      `prune_orphan_profiles(&profile_root, age_threshold)` helper in
      `util/profile_dir.rs`. The helper iterates `read_dir`, filters
      to `ff-rdp-profile-*` basenames, checks `metadata.modified()`
      against `Utc::now() - age_threshold`, and `remove_dir_all` the
      stale ones. Errors are warn-not-fail (never block a launch).
- [x] Bound the work: stop after pruning `FF_RDP_PROFILE_PRUNE_MAX`
      (default 50) entries per launch so a 1700-dir backlog doesn't
      add 30s to a hot launch. Subsequent launches will pick up the
      next batch.
- [x] Land `pre_fix_repro_launch_prunes_stale_orphan_profiles` as a
      unit test that pre-seeds the profile root with three dated
      directories (using `filetime::set_file_mtime`) and asserts they
      vanish after `prune_orphan_profiles` runs.
- [x] `unit_prune_orphan_profiles_respects_age_threshold`: seed
      `aaaa` (8 days old) + `bbbb` (1 hour old); prune at 7-day
      threshold; assert `aaaa` gone, `bbbb` survives.

### Theme C — `profiles` subcommand + `doctor` check [5/5] [pre_fix_repro_test: pre_fix_repro_profiles_subcommand_prune_dry_run_lists_orphans]

- [x] Add `crates/ff-rdp-cli/src/commands/profiles.rs` with two
      subcommands: `list` (path, count, total size, oldest mtime) and
      `prune` (delete with `--older-than`, `--all`, `--dry-run`).
      Wire into `cli/args.rs` clap derive.
- [x] Implement size aggregation via `walkdir` (already a dep).
      `--dry-run` prints exactly what `--all` (or `--older-than`)
      would remove, without removing it.
- [x] In `commands/doctor.rs`, add a `profile_disk_usage` check:
      walk `profile_root`, count entries + total size. Emit `warn`
      if entries > 100 OR total size > 1 GB, with hint
      `ff-rdp profiles prune` and the current count/size in the
      detail. `ok` otherwise. Never `fail`.
- [x] Land `pre_fix_repro_profiles_subcommand_prune_dry_run_lists_orphans`:
      seed profiles, `profiles prune --dry-run`, assert JSON
      `would_remove` array contains the seeded basenames AND
      directories still on disk.
- [x] `unit_doctor_profile_disk_usage_warns_above_threshold`: mock
      a `profile_root` with 101 entries; doctor check returns `warn`.

## Acceptance Criteria [11/11]

- [x] `pre_fix_repro_daemon_stop_removes_active_profile`: live;
      profile dir gone after stop.
- [x] `unit_cleanup_profile_dir_refuses_path_outside_profile_root`:
      external paths are never removed.
- [x] `live_daemon_stop_profile_path_matches_launch_json`:
      `launch.results.profile_path` == `daemon stop.results.profile_removed_path`.
- [x] `pre_fix_repro_launch_prunes_stale_orphan_profiles`: seeded
      stale dirs vanish on next launch.
- [x] `unit_prune_orphan_profiles_respects_age_threshold`: only
      mtime-older-than-threshold entries are removed.
- [x] `unit_prune_orphan_profiles_bounded_by_max`: seed 60 stale
      dirs; assert at most 50 removed per call (the default
      `FF_RDP_PROFILE_PRUNE_MAX`).
- [x] `pre_fix_repro_profiles_subcommand_prune_dry_run_lists_orphans`:
      dry-run lists without deleting.
- [x] `unit_profiles_list_aggregates_size_and_count_correctly`:
      seed known sizes; assert sums match.
- [x] `unit_doctor_profile_disk_usage_warns_above_threshold`:
      threshold trips.
- [x] `live_profiles_prune_removes_all_when_no_firefox_running`:
      seed orphans, no Firefox running, `profiles prune --all`,
      assert zero entries remain.
- [x] `dogfood_script_full_run_iter_96`: `.dogfood.sh` drives `prune_orphan_profiles`,
      `cleanup_profile_dir`, `run_prune`, and the doctor `profile_disk_usage`
      check end to end; exits 0 and writes `/tmp/ff-rdp-iter-96-dogfood-ok`
      (verified 2026-07-03, all themes PASS).

## Out of scope

- **Active-Firefox detection for `--all`.** A naive implementation
  would check `lsof` against each profile path before removing, but
  cross-platform that gets messy. For this iter, `--all` is documented
  as "removes everything matching `ff-rdp-profile-*`; do not run while
  Firefox is using one of these profiles". The `daemon stop` cleanup
  in Theme A handles the common case where Firefox *is* running.
- **Profile snapshot/restore** for debugging — out of scope; if you
  want to keep a specific session's state, use `--profile <path>`.
- **Migration of the existing 1715-dir backlog** at upgrade time.
  Theme B's pruning handles it organically over a few launches;
  users who want it gone immediately run `ff-rdp profiles prune --all`.
- **Linux/Windows path differences** — `secure_profile_root` already
  abstracts the per-OS state directory; this iter inherits that
  abstraction without changes.

## References

- [[dogfooding-session-60]]
- [[iteration-95-session-60-followups]] (the SIGKILL ladder this
  builds on)
- `crates/ff-rdp-cli/src/commands/launch.rs:244-264` (the
  `.keep()` call + the comment claiming a cleanup path that does
  not exist)
- `crates/ff-rdp-cli/src/util/profile_dir.rs` (where the new
  helpers belong)
- `crates/ff-rdp-cli/src/daemon_record.rs:55` (`profile_dir` field
  Theme A consumes)
