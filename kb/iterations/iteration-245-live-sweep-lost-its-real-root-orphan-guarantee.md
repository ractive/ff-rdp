---
title: "Iteration 245: live-sweep harness: real-root orphan guarantee, live_158 hang stack capture, Windows process paths"
type: iteration
date: 2026-08-23
status: done
branch: iter-245/live-sweep-orphan-guarantee
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

#### A. Design the check [2/2]
- [x] Decide the marker-reading approach (duplicate vs. expose a helper) and record the trade-off
- [x] Decide whether a finding fails the sweep, warns, or both — and update the
      `LIVE_SWEEP_SUMMARY` line's documented shape if it changes

#### B. Implement and prove it catches something [2/2]
- [x] The check runs after phase 1, scans the real root only (never a `$FF_RDP_HOME`-isolated one —
      scanning those would be meaningless, they are always empty when their owning test exits
      cleanly)
- [x] A reproduction: deliberately leave a live-owned profile in the real root, run `live-sweep`,
      confirm the check names it (per this plan's `dogfood_path`); clean up, re-run, confirm clean

### Acceptance Criteria [2/2]

- [x] `live-sweep` run against a real root with one deliberately-left live-owned profile reports it
      by name (directory + PID), not silently
- [x] `live-sweep` run against a clean real root reports no finding, and existing accounting
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

#### A. Enumerate the blocking candidates [1/1]
- [x] Every call between test start and `FF_RDP_LIVE_LAUNCH_TIMEOUT_SECS`'s bound is listed with
      file:line, and each is marked as either already bounded (cite the bound) or not

#### B. Capture hook [2/2]
- [x] A stack-sampling capture runs against the hung test binary's pid, gated to fire only when
      the timed-out name is `live_158_launch_survives_contended_bind`, and only *before*
      `kill_phase_tree` sends its signal
- [x] The capture's output path is printed by `live-sweep` in the same `WATCHDOG` report that
      names the unreported test, so a later reader does not have to know where to look

### Acceptance Criteria [1/1]

- [x] `live_208_capture_hook_fires_only_for_the_named_test`: given a `timed_out` set containing
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

#### A. Kill-path proof [1/1]
- [x] A `#[cfg(windows)]` test spawns a process with a real child it did not directly launch (the
      Windows analogue of the Unix grandchild fixture) and asserts `kill_phase_tree` — via
      `taskkill /F /T` — leaves neither alive; runs on `windows-latest` CI

#### B. Listing-format proof [1/1]
- [x] A `#[cfg(windows)]` test spawns a real short-lived process with a distinguishing command
      line, calls `process_listing()` for real, and asserts `managed_firefox_pids` finds it —
      proving the PowerShell one-liner's actual output shape, not a hand-written fixture

#### C. Scope decision [1/1]
- [x] This plan states, in its Outcome section, whether A and B passed as first-run and whether
      any further Windows-specific coverage is warranted or the module is accepted as
      never-exercised-in-CI by design

### Acceptance Criteria [2/2]

- [x] `windows_live_sweep_kill_phase_tree_reaches_a_real_grandchild` (Theme A) passes on
      `windows-latest` CI, not just locally
- [x] `windows_live_sweep_process_listing_matches_a_real_process` (Theme B) passes on
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

## Closing acceptance criterion (covers all parts) [1/1]

- [x] `cargo fmt && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace -q` clean.

## Outcome — Part A: the real-root orphan check

### Design decisions (Part A, task A)

**1. Duplicate the marker format in `xtask`; do not expose a helper from `ff-rdp-cli`.**
`crates/xtask/src/live_sweep.rs` now carries its own `MANAGED_PROFILE_PREFIX`,
`OWNER_PID_MARKER` and `OWNER_TEST_MARKER` constants plus a ~30-line reader
(`scan_owned_profiles`). The alternative — a narrow `pub` read-only helper on `ff-rdp-cli` —
was rejected on build cost, not on taste: `xtask` has no dependency on `ff-rdp-cli` today,
and adding one would make **every** `cargo run -p xtask -- check-*` invocation (the discipline
gates that run on every iteration, and in CI) build the whole CLI and its dependency tree
first, to read three file names. The live-test harness already carries exactly this
duplication for exactly this reason — `crates/ff-rdp-cli/tests/common/mod.rs` says so in
`OWNER_PID_MARKER`'s own doc comment — so this is the established shape rather than a new one.
Failure mode if a copy goes stale: the check finds nothing (a marker rename would make
`scan_owned_profiles` see an unmarked directory), never a false accusation.

**2. A finding fails the sweep — but only the attributable kind — and it gets its own summary
line.** Three sub-decisions, each with its reason:

- **`leaked` fails the sweep.** A live test's Firefox still holding a profile after its tier's
  phase 1 is the exact regression iteration 146 made loud, and iteration 146's own postmortem
  is that a *quiet* signal let real leaks run for weeks. A warning inside a 40-minute log is a
  quiet signal.
- **`unattributed` never fails it.** A live-owned managed profile with no `.ff-rdp-owner-test`
  marker cannot be tied to the live tier at all — the likeliest owner is the developer's own
  `ff-rdp launch` in another terminal, which is explicitly none of the sweep's business (the
  plan's question 2). It is printed as a note and counted separately.
- **A new `LIVE_SWEEP_PROFILES leaked=N unattributed=U root=<path>` line, not an eighth field
  on `LIVE_SWEEP_SUMMARY`.** Every field of that line counts a *test* and `total=T` conserves
  them; several readers depend on that invariant. A leaked profile is not a test, so folding it
  in would need a permanent explanation of its relationship to `total`. `LIVE_SWEEP_SUMMARY`'s
  documented shape is therefore **unchanged** (AC2's "existing accounting is unchanged" is
  satisfied literally, not just numerically).

**3. What separates "this sweep's leak" from "somebody else's browser" (the plan's question 2)
is the owner-test marker, not the clock.** `FF_RDP_LIVE_TEST_NAME` is set only by the live
harness (`tests/common/mod.rs::ff_rdp_launch_command`), so a profile carrying that marker was
launched by a live test and a profile without one was not. The pre-sweep snapshot
(`preexisting_unmarked_names`) is a second, weaker signal used only for the *unmarked* case:
an interactive browser that was already open is excused by name, one that appears mid-sweep is
noted. A marked profile is deliberately **not** excused by the snapshot — a live test's browser
alive before the sweep even starts is a leak somebody should hear about, not a state to
normalise, and it is what the `dogfood_path` below reproduces. The residual false positive is a
*second concurrent live sweep on the same machine*; that configuration is already unsupported
for older reasons (`reap_managed_firefox` kills managed browsers machine-wide, and the
`preexisting` tier assumes one client on port 6000), and the failure message says so in as many
words rather than pretending it cannot happen.

### Where the check runs

After **each target's phase 1**, inside the target loop in `live_sweep::run` — the vantage point
the plan identified, since at that instant every self-launching test in that target has either
cleaned up (`daemon stop` / `LiveFirefox::drop`) or already been counted as a failure. Anything
reported is folded into the excused set so a later target cannot report the same directory
twice. `--dry-run` skips it (nothing ran, so there is nothing to have leaked).

## Outcome — Part B: the blocking-call enumeration and the capture hook

### Every blocking call between test start and the 30 s port-wait bound (Part B, task A)

The bound in question is `launch`'s own `DEFAULT_PORT_WAIT` (30 s,
`crates/ff-rdp-cli/src/commands/launch.rs:444`, overridable by `--launch-timeout` /
`FF_RDP_LAUNCH_TIMEOUT_SECS`). Note that this test does **not** go through `LiveFirefox`, so the
harness's `FF_RDP_LIVE_LAUNCH_TIMEOUT_SECS` (`tests/common/mod.rs:134`) never applies to it —
the plan's phrasing assumed it did.

| Call | Site | Bounded? |
| --- | --- | --- |
| `ff_rdp_launch_command_for(..).output()` ×4 threads | `tests/live/live_158_launch_lifecycle.rs:89` | **No.** Waits for child exit *and* EOF on both pipes, so it inherits whatever bounds `ff-rdp launch`. A pipe-inheritance hang is **refuted** here: Firefox is spawned with `Stdio::null()` stdout and a `piped()` stderr owned by `ff-rdp` (`launch.rs:402-404`), so the launched browser never holds the test's pipes. |
| `h.join()` | `live_158_launch_lifecycle.rs:102` | **No** — inherits the row above. |
| `gc_stale_spawn_locks` / `gc_legacy_spawn_lock` / `gc_stale_throttle_states` / `gc_stale_launch_records` | `launch.rs:812,813,814,821` | No explicit bound, but filesystem-only: no lock is waited on, no subprocess spawned. |
| `(hooks.is_port_in_use)(port)` | `launch.rs:840` → `port_owner.rs:42-51` | **Yes** — `TcpStream::connect_timeout(200 ms)`. |
| `identify_running_instance` → `port_owner::find_listener` → `lsof -nP -iTCP:<port> … .output()` | `launch.rs:844` → `port_owner.rs:56-62` | **No — prime suspect.** Reached only once the port probe says something is listening, which is precisely the contended case. `lsof` walks every fd of every process; with four launches racing and ~200 browsers on the machine it is the one unbounded call on the hot path that *scales with load*, which fits a hang that happens once in three whole-tier sweeps and never in isolation. |
| `reject_if_port_occupied` → the same `lsof` | `launch.rs:855`/`:669` → `port_owner.rs:56` | **No** — same call, other branch. |
| `(hooks.locate_firefox)()` → `which_binary(..).output()` | `launch.rs:859` → `launch.rs:77` | **No.** A `which`/`where` subprocess with no timeout; fast in practice, unbounded in principle. |
| `(hooks.spawn)(&mut cmd)` | `launch.rs:880` | Non-blocking (`posix_spawn`/`fork`). |
| `write_owner_pid_marker` / `write_owner_test_marker` | `launch.rs:902`, `:915` | Filesystem writes (atomic rename); no wait primitive. |
| `std::thread::sleep(500 ms)` | `launch.rs:919` | **Yes** — fixed 500 ms. |
| `child.try_wait()` | `launch.rs:921` | **Yes** — non-blocking by construction. |
| `stderr.read_to_string(..)` | `launch.rs:926` | **No — second suspect.** Reads Firefox's stderr pipe to EOF; a grandchild content process that inherited that pipe keeps it open indefinitely. Reached only on the "Firefox exited immediately" path, which makes it a poorer fit for the observed shape (four live Firefoxes were still running) but it is a genuine unbounded wait. |
| `(hooks.probe_port)` → `wait_for_port` | `launch.rs:943` → `launch.rs:694-733` | **Yes** — `resolve_port_wait_bound` (flag → env → `DEFAULT_PORT_WAIT` 30 s), per-address `connect_timeout`, 200 ms poll interval, hard deadline. |

Output as the plan asked: a list of suspects with `file:line`, **not** a fix. Nothing here is
measured — the hang has not recurred since 2026-08-23 — and inventing a fix for an unreproduced
hang is what iteration 197 already declined to do.

### The capture hook (Part B, task B)

`run_phase` takes a `PhaseWatch` (phase 1 only; phase 2 executes nothing and cannot hang in a
test body). When the watchdog fires, it computes `unreported_tests` from the output captured so
far and, **only** if `capture_hook_should_fire` matches `live_158_launch_survives_contended_bind`
on the final `::` segment, samples the stalled process tree *before* `kill_phase_tree` signals
it: `descendant_pids` from a `ps -eo pid=,ppid=,args=` snapshot (so the target is the test binary
`cargo` spawned, not `cargo`), then `sample <pid> 5 -f <out>` on macOS or
`gdb -p <pid> -batch -ex 'thread apply all bt'` on Linux. Files land in
`target/live-sweep/live_158_launch_survives_contended_bind-<epoch>-pid<n>.txt` and their paths
are printed in the same `WATCHDOG` report that names the unreported test (AC2). Windows has no
sampler wired up on purpose — see Part C's scope decision. Every step is best-effort: a missing
sampler, an unreadable process table or an empty output file all resolve to "no capture", never
to a delayed kill or an error.

## Outcome — Part C: the Windows process-tree paths

### Scope decision (Theme C)

**Keep both tests in the ordinary `cargo test --workspace` run on `windows-latest`; do not add a
Windows `live-sweep` job.** The two tests cost a few seconds inside a job that already runs (CI's
`test (windows-latest)`, 10-minute budget) and they convert the three never-executed Windows
branches from an assumption into a checked one. What is *not* warranted is running the live tier
itself on Windows: that needs headless Firefox on the runner plus profile/path work, which
iteration 197 did not claim and this plan explicitly puts out of scope. So the module's Windows
support is: `kill_phase_tree` and `process_listing` are proved against real processes by CI;
`kill_pid_hard` (`taskkill /F /PID`) remains exercised only indirectly, through
`windows_live_sweep_kill_phase_tree_reaches_a_real_grandchild`'s cleanup path and
`reap_managed_firefox`, and that is accepted rather than given a third test — it is a one-line
spelling of the same `taskkill` the tree test proves is present and functional.

### Theme A/B first-run result (Part C, Theme C)

Both passed on their **first** `windows-latest` CI run, PR #245, run 34109828660:

```
test live_sweep::tests::windows_live_sweep_process_listing_matches_a_real_process ... ok
test live_sweep::tests::windows_live_sweep_kill_phase_tree_reaches_a_real_grandchild ... ok
```

So the three Windows branches iteration 197 wrote blind are correct as written — `taskkill /F /T`
does reach a process the phase never launched directly, and `Get-CimInstance Win32_Process`'s
real output is the `<pid> <command line>` shape `managed_firefox_pids` parses. No fix was needed,
which is the outcome the plan explicitly refused to guess at up front.

## Outcome — what the check found on its own first real sweep

The first dual-gate sweep with Part A in place reported exactly one leak:

```
live-sweep: LEAKED PROFILE after -p ff-rdp-cli --test live — ff-rdp-profile-VpRz1nZkhn6wILgF
  (pid 84108, spawned by live_target_destroyed::live_target_destroyed_invalidates_registry) …
```

That test **passed**, and its browser was gone by the time anyone looked. The cause is not a
product leak but a defect in the check itself: `LiveFirefox::drop` signals its Firefox and returns
without waiting for it to exit ([[iteration-168-livefirefox-drop-does-not-wait-for-exit]]), and the
check ran the instant `run_phase` returned — so the last tests' browsers were still in the process
table, *dying*, with their markers intact.

Fixed here, not deferred: `settle_owned_profiles` re-scans `ORPHAN_SETTLE_ATTEMPTS` (5) times at
`ORPHAN_SETTLE_DELAY` (2 s) and reports only what is still live-owned, matched on directory **and**
PID, on every scan. A clean root pays exactly one scan and no sleep, so the cost lands only on a
run that has something to report. Four unit tests cover both directions
(`iter_245_a_browser_still_exiting_is_not_reported_as_a_leak`,
`iter_245_a_browser_that_survives_every_scan_is_still_a_leak`,
`iter_245_a_clean_root_is_scanned_once`,
`iter_245_still_owned_matches_on_pid_not_just_directory`), and the confirming sweep below reports
`leaked=0` with the same tier that produced the false positive.

**This is the guarantee working as designed on its first outing** — it found something real (a
browser alive after its tier finished), and following it to the bottom produced a correction rather
than a shrug. A warning-only design would have produced a line nobody read.

**A second defect, caught in PR review rather than by the sweep itself:** `real_profile_root()`
copied the product resolver's `$FF_RDP_HOME` precedence but not its empty-string filter — an
exported-but-empty override would have resolved to a *relative* `ff-rdp/profiles` under the
sweep's cwd instead of falling through to `state_dir`/`data_local_dir`, the same edge case
`util::home_override()` exists to guard against. Fixed on the branch: the env read is now split
into a pure `resolve_real_profile_root`, with `iter_245_real_profile_root_filters_an_empty_home_override`
covering both the fallthrough and the genuinely-set-override precedence.

## Live sweep (closing record)

Two dual-gate sweeps, macOS, `--jobs 6`, a raw `firefox --start-debugger-server 6000 --headless`
up for the `preexisting` tier. Both quoted with their gates, per the `iteration-close` skill.

**Sweep 1** — before the settle-loop fix (`FF_RDP_LIVE_TESTS=1 FF_RDP_LIVE_NETWORK_TESTS=1`):

```
LIVE_SWEEP_SUMMARY executed=328 skipped=0 preexisting=0 vanished=0 launch_timeout=0 timed_out=0 total=328
LIVE_SWEEP_PROFILES leaked=1 unattributed=0 root=~/Library/Application Support/ff-rdp/profiles
ff-rdp-cli tier: 304 passed / 15 failed (449.63 s); ff-rdp-core tiers: 9 passed / 0 failed
```

**Sweep 2** — the branch as it stands (`FF_RDP_LIVE_TESTS=1 FF_RDP_LIVE_NETWORK_TESTS=1`):

```
LIVE_SWEEP_SUMMARY executed=328 skipped=0 preexisting=0 vanished=0 launch_timeout=0 timed_out=0 total=328
LIVE_SWEEP_PROFILES leaked=0 unattributed=0 root=~/Library/Application Support/ff-rdp/profiles
ff-rdp-cli tier: 307 passed / 12 failed (521.27 s); ff-rdp-core tiers: 9 passed / 0 failed
live-sweep: real profile root … holds no live-owned profile this sweep left behind
  (checked after -p ff-rdp-cli --test live)
```

Reconciliation, both runs: `passed + failed = 328 = executed`, so no test went missing.

**Declared contamination:** sweep 2 ran while the host desktop was at load average 400+ on a
10-core machine (Finder, Chrome, Teams and ~90 WebKit content processes belonging to the operator,
not the sweep). Sweep 1 is the cleaner of the two and its failure list is the one to compare
against.

`executed=328` vs the 319 the CLI tier reports is the four `ff-rdp-core` targets (1 + 3 + 3 + 2),
not a missing verdict.

## Carry-over

| # | Row | Disposition |
| --- | --- | --- |
| 1 | 7 `--full-page` screenshot failures in both sweeps, all `TypeError: WindowGlobalParent.drawSnapshot: Argument 4 can't be converted to a dictionary` (`live_61l`, `live_61r_screenshot`, `live_92` ×2, `live_135`, `live_144`, `live_screenshot_shim`) | **fold** — already exactly [[iteration-257-firefox-155-drawsnapshot-dictionary-arg]], which is filed and pending. No edit needed. |
| 2 | 8 load-shaped live failures whose set is **not stable between two runs on the same commit** (`live_109` block/unblock daemon timeout, `live_145` frame targets, `live_159` CMP, `live_169` direct *and* daemon, `live_174` daemon, `live_212` ref click, `live_237` ×2) | **fold** into [[iteration-246-sweep-load-misclassification]] — added as a dated addendum with the per-run table and the two rows that deserve to be read as more than noise. |
| 3 | `live_navigate_default_fast::live_navigate_elapsed_matches_wall` — failed in **both** sweeps, in the same direction: reported `elapsed_ms` ~900 ms *smaller* than measured wall | **fold** into [[iteration-246-sweep-load-misclassification]], flagged there as possibly a real honesty regression (iter-122 Theme B) rather than load: load lengthens the wall clock, it does not shorten ff-rdp's own measurement. Re-run in isolation before writing off. |
| 4 | `live_target_destroyed_invalidates_registry` reported as a leaked profile by the new check | **closed in this PR** — it was the check's own false positive; `settle_owned_profiles` and four unit tests, above. |
| 5 | Six dead-owner `ff-rdp-profile-*` directories left in the real root after the sweeps (no live owner, so not reported by this check) | **fold** — this is `daemon stop`'s removal race, already [[iteration-260-live-owner-removal-race-in-daemon-stop]]. Named here because the check makes the *live* half visible while the dead half stays that plan's business. |
| 6 | `live_158_launch_survives_contended_bind` still has not hung, so the capture hook has never fired for real | **no plan, with a stated reason** — there is nothing measured to act on; the hook exists precisely so the next occurrence is diagnosable. If a sweep reports `timed_out` naming it and the capture produces a stack, *that* stack needs its own plan (iteration 197's own disposition, unchanged). |
| 7 | `kill_pid_hard`'s Windows branch (`taskkill /F /PID`) is still only exercised indirectly | **no plan, with a stated reason** — Part C Theme C's scope decision, above. It is a one-line spelling of the `taskkill` the tree test proves is present and functional; if a Windows reap is ever observed failing, that is when it earns a test. |
