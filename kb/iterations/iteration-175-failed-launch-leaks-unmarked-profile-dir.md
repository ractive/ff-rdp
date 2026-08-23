---
title: "Iteration 175: a launch that fails before its owner marker is written leaks an unattributable, unreclaimable profile dir"
type: iteration
date: 2026-08-17
status: done
branch: iter-175/failed-launch-leaks-unmarked-profile
depends_on:
  - iteration-171-stale-owner-pid-marker-and-pid-reuse
first_call_sites:
  - primitive: >-
      ManagedProfileGuard (armed / armed_under / disarmed / disarm) — RAII owner of a
      freshly-created managed profile dir, removed on any early return
    site: crates/ff-rdp-cli/src/commands/launch.rs (build_command, run_with_hooks)
  - primitive: >-
      LaunchHooks::locate_firefox — injected Firefox lookup so the
      post-profile-creation error paths are reachable without a real browser
    site: crates/ff-rdp-cli/src/commands/launch.rs (run_with_hooks, LaunchHooks::real)
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
tags:
  - iteration
  - profiles
  - launch
  - cleanup
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

## Findings (2026-08-23)

### Theme A — the mechanism, measured

Step 1 of `dogfood_path`, run against the real profile root
(`~/Library/Application Support/ff-rdp/profiles`) at 02:38 on 2026-08-23:

```
UNMARKED: ff-rdp-profile-0D54FEBaOOHKgdnS  contents: user.js
UNMARKED: ff-rdp-profile-3wuZ8IGZ5v1jtSs2  contents: user.js
UNMARKED: ff-rdp-profile-6U17BdTnZF2dI3a5  contents: user.js
UNMARKED: ff-rdp-profile-bI0zCMEgkvvGmT5O  contents: user.js
UNMARKED: ff-rdp-profile-coYI0iXWOLRFAzfa  contents: user.js
UNMARKED: ff-rdp-profile-jTmBdIAHnXextnsi  contents: user.js
UNMARKED: ff-rdp-profile-lstQ3NK8Pw3MLAmz  contents: user.js
UNMARKED: ff-rdp-profile-OJ8wKAOxBy0n1rLK  contents: user.js
total=8 unmarked=8
```

8 of 8, every one holding exactly `user.js` and no `.ff-rdp-owner-pid` — the same
fingerprint iteration 171 recorded for its 20. mtimes clustered at 02:25:19 (×4) and
02:27:24 (×4), 11–13 minutes old, i.e. produced by the concurrent dogfood agents running in
the same session, not by one pathological run. **Iteration 171's marker-write move did not
change this**: the marker still lands after the spawn, and these directories never reached a
spawn.

Step 3, same moment:

```
$ ff-rdp profiles prune --older-than 1h --dry-run
{"results":{"path":".../profiles","would_remove":[],"removed":[],"removed_live":[],"dry_run":true},"total":0}
```

`would_remove` empty, exactly as the plan predicted — an unmarked directory falls back to the
mtime heuristic and these were minutes old. At the 7-day default they would have sat there
for a week.

### Theme A — which error paths actually leak (read from the code, then forced)

Everything between "the profile directory exists on disk" and "`run` returns Ok" leaks. In
`launch.rs` source order:

| # | Error path | Leaks pre-175? | How it was forced |
|---|---|---|---|
| 1 | `build_command`: `fs::write(user.js)` fails | yes | read-only root (inspection) |
| 2 | `build_command`: `ensure_extension_autoinstall` fails (`--auto-consent`) | yes | inspection |
| 3 | `build_command`: `auto_consent::install` fails (no network) | yes | inspection; the existing `build_command_auto_consent_with_temp_profile_installs_extension` test already tolerates this branch offline |
| 4 | `run`: `spawn` fails | yes | `unit_175_failed_spawn_leaves_no_profile_dir` |
| 5 | `run`: Firefox exits immediately | yes | `unit_175_immediate_exit_leaves_no_profile_dir` |
| 6 | `run`: debug port never opens (`TimedOut` / `Unresolvable`) | yes | `unit_175_port_wait_timeout_leaves_no_profile_dir`, and live via `--launch-timeout 0` |
| 7 | `run`: `child.try_wait()` returns `Err` | yes | inspection (no portable way to force it) |
| 8 | the CLI process is killed anywhere in 4–6's window | yes, and **unfixable by `Drop`** | the pre-spawn marker, not the guard, is what covers this |

Paths that turn out **NOT** to leak, and why — all of them return *before* `build_command`:

- `--window-size` parse failure — parsed first, deliberately (iter-133 Theme A).
- **Port already occupied.** `dogfood_path` step 2 proposed `--debug-port <held port>` as a way
  to force a leak. That premise is **wrong** as of iteration 158: `reject_if_port_occupied`
  runs before `find_firefox`, so no directory is ever created. This is recorded rather than
  quietly dropped because it is the one enumeration entry that contradicts the plan.
- `find_firefox()` failing (no Firefox installed).
- `--replace` where `stop_prior_instance` fails.
- Any failure at all under `--profile <user-path>`: that directory is the user's, is never a
  managed `ff-rdp-profile-*` name, and never gets a marker or a guard.

### Theme B — the shape chosen, and the one rejected

**Both**, because they cover disjoint halves of the failure set and the plan's own AC 2 rules
out (ii) alone.

- **(i) RAII ownership** — `ManagedProfileGuard` in `util/profile_dir.rs`, armed in
  `build_command` the moment the directory exists and re-armed in `run` across the spawn,
  disarmed at exactly two points: `build_command`'s success return, and `run`'s success branch
  after the port probe passes. Rows 1–7 above. It routes through
  `cleanup_profile_dir_under`, so both existing safety checks (under the profile root, managed
  basename) still gate every removal — a `--profile` directory can never be reached by it.
- **(ii) Pre-spawn self-owned marker** — `build_command` writes `.ff-rdp-owner-pid` /
  `.ff-rdp-owner-start` holding the *launching CLI's own* PID before anything else can fail,
  and `run`'s existing post-spawn write replaces the pair with Firefox's. Row 8, the case
  `Drop` provably cannot reach.

**Rejected: (ii) alone.** Its failure mode is that the leak still happens — the directory
survives the failed launch and is merely *reclaimable*, on the next `launch`, by the iter-142
dead-owner rule. AC 2 asks for a forced failure to leave nothing behind, and "it will be gone
after you run the command again" is not that. Rejected too because the reclaim only fires while
the launching process is dead; two concurrent agents (the normal case here) would keep each
other's failed directories alive for as long as either is running.

**Rejected: (i) alone.** Its failure mode is the whole reason iteration 171 exists: `Drop`
does not run on SIGKILL, a CI timeout, or a live sweep interrupted mid-test, which is precisely
how the 20 + 8 directories were produced. A guard-only fix would have left that count unchanged.

One hazard the combination introduces and closes: `write_owner_pid_marker` is now an
*overwrite*. Writing Firefox's PID beside the CLI's stale start token would grade the profile
`Dead` (tokens disagree) and hand the iter-142 rule a live Firefox's profile to delete. The
function now clears the old token before writing the new PID, so the pair can never describe
two processes — regression-tested by
`unit_175_remarking_a_profile_clears_the_previous_start_token`.

`LaunchHooks` gained a `locate_firefox` hook. Without it every test past that line needs a real
Firefox installed, which is why these four error paths went four iterations with no unit
coverage at all.

### Theme C — backfill decision

**Rule:** in `prune_orphan_profiles` (the sweep every `ff-rdp launch` runs), a managed directory
that has **no owner marker** and contains **exactly one entry, `user.js`** is treated as a
provably-failed launch and removed regardless of `age_threshold`, subject to a 10-minute
`FAILED_LAUNCH_GRACE`.

- Not a new flag, and not opt-in: it describes a directory that provably cannot be in use, and
  the whole complaint is that these accumulate silently.
- The grace is a race guard, not an age gate. Post-175 the window between creating the directory
  and marking it is two syscalls; 10 minutes covers a pre-175 binary running concurrently, and
  is far inside the seconds a real Firefox takes to write `prefs.js`.
- An **empty** directory deliberately does not qualify — `build_command` writes `user.js` two
  statements after creating the directory, so an empty one is not this failure mode, and
  widening the rule past its evidence would have broken
  `unit_prune_orphan_profiles_respects_age_threshold`'s 1-hour survivor for the wrong reason.
- **`profiles prune` is deliberately left alone.** It stays a pure age query; it does not carry
  the iter-142 dead-owner rule either, so adding only this one would have made it inconsistent
  in a new direction. `--all` remains the explicit "everything, now" escape hatch. Recorded in
  its `--help`.

Observed working: after the first test run on this branch, the 8 directories above were gone
from the real profile root, reclaimed by the sweep `build_command` runs. The only survivors
were 2 directories created minutes earlier by a deliberately-neutered build, still inside the
10-minute grace.

### Verification that the tests detect the defect

With the pre-spawn marker and both guards neutered in place (fix reverted, tests kept):

```
test result: FAILED. 1 passed; 4 failed
  unit_175_failed_spawn_leaves_no_profile_dir            FAILED
  unit_175_immediate_exit_leaves_no_profile_dir          FAILED
  unit_175_port_wait_timeout_leaves_no_profile_dir       FAILED
  unit_175_build_command_marks_profile_with_own_pid_...  FAILED
  unit_175_user_profile_dir_survives_a_failed_launch     passed  (safety regression test — passes both ways by design)
```


### Closing sweeps (2026-08-23)

Two full dual-gate runs, back to back, `FF_RDP_LIVE_TESTS=1 FF_RDP_LIVE_NETWORK_TESTS=1`:

```
sweep 1: LIVE_SWEEP_SUMMARY executed=282 skipped=0 preexisting=0 vanished=0 launch_timeout=0 total=282
         272 passed / 1 failed — live_96_profile_cleanup::live_profiles_prune_removes_all_when_no_firefox_running
sweep 2: LIVE_SWEEP_SUMMARY executed=282 skipped=0 preexisting=0 vanished=0 launch_timeout=0 total=282
         272 passed / 1 failed — live_61r_eval::live_eval_on_hn
```

Different single failure each time, both green when re-run in isolation, and both filed as
[[iteration-190-live-sweep-only-failures]]. Sweep 1's failure was **self-inflicted**: the
port-6000 browser started for the `preexisting` tier was started with `ff-rdp launch`, which
creates a managed profile owned by a live PID — exactly `live_96`'s documented precondition.
Sweep 2 was re-run with a raw, unmanaged port-6000 Firefox and `live_96` passed.

`live_175_failed_launch_leaves_no_profile_dir` and
`live_175_successful_launch_keeps_its_profile_dir` executed and passed in **both** sweeps.
Orphan check afterwards (`pgrep -f 'MacOS/firefox.*ff-rdp-profile'`): 0.


## Tasks

### A. Verify
- [x] Run every step of `dogfood_path` and paste actual outputs into this plan
      (steps 1 and 3 above; step 2's premise was wrong — see the enumeration)
- [x] Enumerate which `launch` error paths leave the profile dir behind, by reading the code and
      forcing each one
- [ ] Count unmarked dirs produced by one full live sweep — **the defect-side number was never
      measured**. Two full 282-test dual-gate sweeps on this branch produced **0** unmarked
      directories (profile root afterwards: 1 directory, 0 unmarked), against 8 of 8 unmarked
      before the fix. That is the *post*-fix figure; a sweep on `main` was never run, so
      "how many does an unfixed sweep produce" is still unanswered and the box stays empty
      rather than reworded. The question the count was *for* — does this fire in automated runs
      or only interactively? — is answered by the 8 directories above, which concurrent agents
      produced in a 2-minute window doing exactly what a sweep does: automated.

### B. Fix
- [x] The chosen shape, with the rejected alternative recorded and its failure mode named
      (both shapes taken; both single-shape alternatives rejected with reasons — Theme B above)
- [x] Unit tests over the error paths (no real Firefox — `launch`'s hooks already allow stubbing)
      — 5 in `commands/launch.rs`, 8 in `util/profile_dir.rs`
- [x] A live test that fails on `main` and passes on the branch —
      `live_175_failed_launch_leaves_no_profile_dir` (`--launch-timeout 0`), plus
      `live_175_successful_launch_keeps_its_profile_dir` for the other direction

### C. Backfill
- [x] Decide and record whether existing unmarked directories are reclaimed, and under what rule

## Acceptance Criteria [4/4]

- [x] Theme A's enumeration is recorded here, including any error path that turns out NOT to leak
      — the table above, plus the five non-leaking paths and the `dogfood_path` step whose
      premise iteration 158 had already invalidated
- [x] A forced launch failure leaves no `ff-rdp-profile-*` directory behind — asserted by a test
      that fails on `main`: `unit_175_failed_spawn_leaves_no_profile_dir`,
      `unit_175_immediate_exit_leaves_no_profile_dir`,
      `unit_175_port_wait_timeout_leaves_no_profile_dir` (all three verified failing with the
      fix neutered), and live `live_175_failed_launch_leaves_no_profile_dir`
- [x] No age-gated prune behaviour is loosened for directories that are merely *old* (the 7-day
      gate stays; only provably-failed directories may be reclaimed early) —
      `unit_175_prune_does_not_loosen_the_age_gate_for_merely_old_dirs` holds a day-old
      unmarked-but-populated profile and a just-created failed one; `profiles prune` is
      untouched
- [x] `cargo fmt && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace -q`
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
