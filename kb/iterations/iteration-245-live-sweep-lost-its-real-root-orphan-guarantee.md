---
title: "Iteration 245: live-sweep harness: real-root orphan guarantee, live_158 hang stack capture, Windows process paths"
type: iteration
date: 2026-08-23
status: planned
branch: iter-245/live-sweep-real-root-orphan-guarantee
depends_on: [kb/iterations/iteration-188-live-sweep-cost-and-parallelism.md, kb/iterations/iteration-146-live-suite-reliability.md, 197]
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
  # --- from iteration 247 ---
  # This iteration has nothing to run today — the hang it is waiting for has not
  # recurred since 197 built a bound around it. What it dogfoods is the capture
  # path, in isolation, so it is ready the next time the watchdog reports
  # `timed_out` naming this test:
  #   1. Run the tier long enough under load to make a slow bind contend
  #      (mirrors the shape iteration 188's third sweep hit):
  FF_RDP_LIVE_TESTS=1 cargo run -p xtask -- live-sweep --jobs 6
  #   2. If `LIVE_SWEEP_SUMMARY … timed_out=N …` names
  #      live_158_launch_survives_contended_bind, the capture script this
  #      iteration adds must have already written a `sample`/`lldb` (or `perf
  #      record` on Linux) snapshot of the still-alive test binary BEFORE
  #      live-sweep's watchdog SIGKILLs its process group — expected: a
  #      non-empty stack file under the path the script prints, naming every
  #      thread `live_158_launch_survives_contended_bind` had running.
  # --- from iteration 248 ---
  # Everything below must be run ON a Windows machine (or a Windows CI runner) —
  # the whole point is that none of it has ever executed there before.
  # 1. The reaper's process listing. Confirm `Get-CimInstance Win32_Process`
  #    really renders as `<pid> <command line>` with no header/trailing rows
  #    that `managed_firefox_pids` would misparse:
  powershell -NoProfile -Command "Get-CimInstance Win32_Process | ForEach-Object { \"$($_.ProcessId) $($_.CommandLine)\" }" | Select-Object -First 5
  #    expected: each line starts with digits, a space, then a full command line —
  #    exactly what `managed_firefox_pids` expects and what
  #    `iter_197_argv0_handles_a_quoted_windows_path` only exercises as a fixture.
  # 2. The watchdog's kill path against a real process tree:
  cargo test -p xtask --lib live_sweep:: -- --include-ignored
  #    expected: the new `#[cfg(windows)]` grandchild-kill test (Theme A) passes,
  #    proving `taskkill /F /T` actually reaches what `cargo` spawned on this
  #    platform the way the Unix process-group kill does today.
  # 3. End to end, forced:
  cargo run -p xtask -- live-sweep --dry-run
  #    (a full forced-timeout run needs Firefox on the runner, which iteration
  #    197's PR notes CI does not have on windows-latest; do at minimum 1-2 with
  #    whatever live tests can run there, and say plainly if none can.)
tags: [iteration, testing, live-tests, tooling, xtask, carry-over, flaky, windows]
---

# Iteration 245: live-sweep harness: real-root orphan guarantee, live_158 hang stack capture, Windows process paths

> **Renumbered 202 → 245 on 2026-09-06** so the pending queue runs as one contiguous sweep (DEC-051). Older PRs, commits and sweep logs cite it as iteration 202.

> **Merged 2026-09-06 (DEC-051 addendum):** absorbs [[iteration-247-live-158-contended-bind-hang-diagnosis]] as Part B and [[iteration-248-live-sweep-windows-process-paths-untested]] as Part C. One branch, one PR, one carry-over sweep for all parts; Part C's ACs are judged on windows-latest CI for that PR.

## Part A: restore the whole-run guarantee that a live sweep leaves no owned profile in the real per-user root

### Where this came from

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

### Why this needs a sweep-level check, not a test-level one

The old test could assert the real-root property because, pre-188, it ran serially and could
assume "no sibling test is using the real root right now." Post-188 that assumption is false for
any *single* test — but it is still true and checkable at exactly one point: **after phase 1 of
`live-sweep` completes**, every self-launching test has finished (successfully cleaned up via
`daemon stop`/`Drop`, per [[iteration-146-live-suite-reliability]] and
[[iteration-168-livefirefox-drop-does-not-wait-for-exit]]) or is accounted for in the failure set.
That is the sweep's own vantage point, which `crates/xtask/src/live_sweep.rs` already has and no
individual test can reconstruct.

### The question to answer

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

### Tasks

#### A. Design the check [0/2]
- [ ] Decide the marker-reading approach (duplicate vs. expose a helper) and record the trade-off
- [ ] Decide whether a finding fails the sweep, warns, or both — and update the
      `LIVE_SWEEP_SUMMARY` line's documented shape if it changes

#### B. Implement and prove it catches something [0/2]
- [ ] The check runs after phase 1, scans the real root only (never a `$FF_RDP_HOME`-isolated one —
      scanning those would be meaningless, they are always empty when their owning test exits
      cleanly)
- [ ] A reproduction: deliberately leave a live-owned profile in the real root, run `live-sweep`,
      confirm the check names it (per this plan's `dogfood_path`); clean up, re-run, confirm clean

### Acceptance Criteria [0/2]

- [ ] `live-sweep` run against a real root with one deliberately-left live-owned profile reports it
      by name (directory + PID), not silently
- [ ] `live-sweep` run against a clean real root reports no finding, and existing accounting
      (`executed`/`skipped`/`preexisting`/`vanished`/`launch_timeout`/`total`) is unchanged by this
      addition

### Out of scope

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

### References

- [[iteration-188-live-sweep-cost-and-parallelism]] — where the isolated test that used to stand in
  for this guarantee was deleted, in PR review, as dead weight
- [[iteration-146-live-suite-reliability]] — Theme B, why the precondition was made loud in the
  first place
- [[iteration-168-livefirefox-drop-does-not-wait-for-exit]] — the cleanup guarantee this check
  would be verifying held, at sweep scale

## Part B: capture a stack the next time live_158_launch_survives_contended_bind hangs (absorbed from iteration 247)

> **Renumbered 208 → 247 on 2026-09-06** so the pending queue runs as one contiguous sweep (DEC-051). Older PRs, commits and sweep logs cite it as iteration 208.

### Where this came from

Carry-over from [[iteration-197-live-sweep-has-no-per-test-timeout]], task A ("reproduce the hang
and identify which of the four launches blocks, and on what"), left unticked deliberately. Twelve
reproduction attempts on 2026-08-24 — 8 runs of
`live_158_launch_lifecycle::live_158_launch_survives_contended_bind` in isolation (2.05-3.29 s,
8/8 green, four live pids each) and 4 runs of the 21-test `launch` subset at `--test-threads=6`
(10.30-12.52 s, 4/4 green, zero orphaned Firefox afterwards) — did not reproduce it. The hang
observed on 2026-08-23 (iteration 188's third sweep, 276 of 277 CLI-tier tests reported, then
silence for 20+ minutes) is a rare, load-dependent event: one occurrence in three whole-tier
sweeps, never reproduced on demand since.

197's disposition on this row was explicit: "if a sweep reports `timed_out` naming this test
again, it needs its own plan, with the captured `sample`/`lldb` stack of the test binary the
watchdog killed." That is what recurring would look like now — 197 built exactly the detector
(`timed_out=N`, the test named) that used to be a silent, unbounded freeze. What is still missing
is the capture step: today a recurrence gets killed and counted, but nothing takes a stack of it
first, so the *why* is destroyed by the same watchdog that finally makes the hang visible.

### What this iteration is, given the hang has not recurred

Not a diagnosis — there is nothing measured to diagnose. It is the instrumentation that makes the
*next* occurrence diagnosable instead of merely counted, plus everything that can be established
about the four-launch shape without a live repro:

1. Read `live_158_launch_survives_contended_bind` and `commands::launch::build_command` closely
   enough to enumerate every blocking call ahead of `FF_RDP_LIVE_LAUNCH_TIMEOUT_SECS`'s 30 s bound
   — a pre-spawn occupancy check against a port an orphan still holds, or a `Command::output()`
   whose child never closes its pipes are iteration 197's own candidates; there may be others.
   This is a code-reading exercise, not a live one, and its output is a short list of suspects
   with file:line, not a fix.
2. A capture hook: when `live-sweep`'s watchdog is about to kill a phase's process group
   (`kill_phase_tree`, `crates/xtask/src/live_sweep.rs`), and the phase went silent while a name in
   `slow_flagged_tests`/`unreported_tests` matches `live_158_launch_survives_contended_bind`
   specifically, run a platform stack sampler (`sample <pid> 5` on macOS, `gdb -p <pid> -batch -ex
   'thread apply all bt'` or a `/proc/<pid>/stack`-based dump on Linux — Windows is out of scope,
   see [[iteration-248-live-sweep-windows-process-paths-untested]]) against the test binary's pid
   **before** the kill, and write it next to wherever the sweep already writes its own output.
   This must not become a second general-purpose feature: gate it to fire only for this one named
   test, so a routine timeout on an unrelated test does not start shelling out to a debugger.
3. Nothing here should widen `--phase-stall-secs` or otherwise change the watchdog's behavior —
   197 already picked that bound and justified it; this iteration only adds a side-effect on the
   way to the kill.

### Tasks

#### A. Enumerate the blocking candidates [0/1]
- [ ] Every call between test start and `FF_RDP_LIVE_LAUNCH_TIMEOUT_SECS`'s bound is listed with
      file:line, and each is marked as either already bounded (cite the bound) or not

#### B. Capture hook [0/2]
- [ ] A stack-sampling capture runs against the hung test binary's pid, gated to fire only when
      the timed-out name is `live_158_launch_survives_contended_bind`, and only *before*
      `kill_phase_tree` sends its signal
- [ ] The capture's output path is printed by `live-sweep` in the same `WATCHDOG` report that
      names the unreported test, so a later reader does not have to know where to look

### Acceptance Criteria [0/1]

- [ ] `live_208_capture_hook_fires_only_for_the_named_test`: given a `timed_out` set containing
      an unrelated test name, the capture hook does not run; given a set containing
      `live_158_launch_survives_contended_bind`, it does

### Out of scope

- Actually fixing the hang. There is no confirmed cause to fix; inventing one to close this plan
  would be worse than leaving it open (iteration 197's own conclusion).
- A general stack-capture facility for any timed-out test. Scoped to this one name until a second
  test demonstrates the same failure shape.
- The Windows side of any of this — no stack sampler is chosen here for that platform; see
  [[iteration-248-live-sweep-windows-process-paths-untested]] — now Part C of this plan

### References

- [[iteration-197-live-sweep-has-no-per-test-timeout]] — where the watchdog that finally bounds
  this hang was built, and the plan row this carries over
- [[iteration-188-live-sweep-cost-and-parallelism]] — the original observation (2026-08-23, third
  sweep, `--jobs 4`)
- `crates/xtask/src/live_sweep.rs` — `kill_phase_tree`, `unreported_tests`, `slow_flagged_tests`
- `crates/ff-rdp-cli/tests/live/live_158_launch_lifecycle.rs` —
  `live_158_launch_survives_contended_bind`

## Part C: live-sweep's Windows process-tree paths compile but have never run (absorbed from iteration 248)

> **Renumbered 209 → 248 on 2026-09-06** so the pending queue runs as one contiguous sweep (DEC-051). Older PRs, commits and sweep logs cite it as iteration 209.

### Where this came from

Carry-over from [[iteration-197-live-sweep-has-no-per-test-timeout]]'s closing sweep. That
iteration built a watchdog and a reaper for `xtask live-sweep`'s process-management (killing a
hung phase's process tree, listing and reaping orphaned ff-rdp-managed Firefox processes), and
gave each mechanism a Windows-specific implementation alongside the Unix one:

- `kill_phase_tree`'s non-unix branch: `taskkill /F /T /PID <pid>` (`crates/xtask/src/live_sweep.rs`)
- `process_listing`'s non-unix branch: `powershell -Command "Get-CimInstance Win32_Process | …"`
- `kill_pid_hard`'s non-unix branch: `taskkill /F /PID <pid>`

None of the three has ever run. `live-sweep` is not invoked on Windows by CI (the `windows-latest`
job runs `cargo test --workspace`, which exercises the *parsing* half of this file —
`managed_firefox_pids`, `argv0`, `is_firefox_executable` — against string fixtures, including
`iter_197_argv0_handles_a_quoted_windows_path` for the quoted-path case — but nothing invokes
`taskkill` or `Get-CimInstance` for real) or by anyone doing iteration work today, since no
contributor's live-sweep runs happen on Windows.

197's own review found a real defect in the equivalent Unix code (`kill -KILL -<pgid>` being
parsed as an option by GNU `kill`, only caught because `ubuntu-latest` actually *ran* it) that
"macOS could not have found" — its words. The same asymmetry applies here in reverse: nothing on
Unix has told anyone whether the Windows spellings are right, because nothing has run them.

### Themes

- **A — Prove the kill path reaches a real process tree.** A `#[cfg(windows)]` mirror of
  `iter_197_watchdog_kill_reaches_the_grandchild`, run on `windows-latest` CI, so this stops being
  an assumption.
- **B — Prove the listing format matches what the parser expects.** Confirm
  `Get-CimInstance Win32_Process`'s real one-line-per-process shape is what
  `managed_firefox_pids` is written against — on a real Windows process, not only the fixture
  string in `iter_197_argv0_handles_a_quoted_windows_path`.
- **C — Decide whether any of this belongs in CI at all**, given `live-sweep` itself is not run on
  Windows. If B and A both check out but the module is otherwise dead weight on this platform, say
  so in this plan rather than adding permanent CI cost for a path nobody exercises end to end.

### Tasks

#### A. Kill-path proof [0/1]
- [ ] A `#[cfg(windows)]` test spawns a process with a real child it did not directly launch (the
      Windows analogue of the Unix grandchild fixture) and asserts `kill_phase_tree` — via
      `taskkill /F /T` — leaves neither alive; runs on `windows-latest` CI

#### B. Listing-format proof [0/1]
- [ ] A `#[cfg(windows)]` test spawns a real short-lived process with a distinguishing command
      line, calls `process_listing()` for real, and asserts `managed_firefox_pids` finds it —
      proving the PowerShell one-liner's actual output shape, not a hand-written fixture

#### C. Scope decision [0/1]
- [ ] This plan states, in its Outcome section, whether A and B passed as first-run and whether
      any further Windows-specific coverage is warranted or the module is accepted as
      never-exercised-in-CI by design

### Acceptance Criteria [0/2]

- [ ] `windows_live_sweep_kill_phase_tree_reaches_a_real_grandchild` (Theme A) passes on
      `windows-latest` CI, not just locally
- [ ] `windows_live_sweep_process_listing_matches_a_real_process` (Theme B) passes on
      `windows-latest` CI

### Out of scope

- Running the live Firefox test tier on Windows CI at all — that is a much larger undertaking
  (headless Firefox availability on `windows-latest`, path/profile handling differences) than
  proving these three process-management primitives work, and iteration 197 did not claim it.
- Fixing anything in `kill_phase_tree` / `process_listing` / `kill_pid_hard` speculatively. If A
  or B fails, that is this iteration's actual finding and the fix belongs here, backed by the
  failing test — not guessed at up front.

### References

- [[iteration-197-live-sweep-has-no-per-test-timeout]] — where these three branches were added,
  and the carry-over row this plan resolves
- `crates/xtask/src/live_sweep.rs` — `kill_phase_tree`, `process_listing`, `kill_pid_hard`,
  `managed_firefox_pids`, `argv0`
- `.github/workflows/ci.yml` — `test (windows-latest)` job, the runner this plan's new tests
  must actually execute on

## Closing acceptance criterion (covers all parts) [0/1]

- [ ] `cargo fmt && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace -q` clean.
