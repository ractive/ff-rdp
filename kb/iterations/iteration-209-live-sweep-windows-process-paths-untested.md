---
title: "Iteration 209: live-sweep's Windows process-tree paths compile but have never run"
type: iteration
date: 2026-08-24
status: planned
branch: iter-209/live-sweep-windows-process-paths
depends_on: [197]
first_call_sites: []
dogfood_path: |
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
tags: [iteration, testing, live-tests, windows, tooling, xtask, carry-over]
---

# Iteration 209: three Windows-only branches nothing has ever exercised

## Where this came from

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

## Themes

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

## Tasks

### A. Kill-path proof [0/1]
- [ ] A `#[cfg(windows)]` test spawns a process with a real child it did not directly launch (the
      Windows analogue of the Unix grandchild fixture) and asserts `kill_phase_tree` — via
      `taskkill /F /T` — leaves neither alive; runs on `windows-latest` CI

### B. Listing-format proof [0/1]
- [ ] A `#[cfg(windows)]` test spawns a real short-lived process with a distinguishing command
      line, calls `process_listing()` for real, and asserts `managed_firefox_pids` finds it —
      proving the PowerShell one-liner's actual output shape, not a hand-written fixture

### C. Scope decision [0/1]
- [ ] This plan states, in its Outcome section, whether A and B passed as first-run and whether
      any further Windows-specific coverage is warranted or the module is accepted as
      never-exercised-in-CI by design

## Acceptance Criteria [0/2]

- [ ] `windows_live_sweep_kill_phase_tree_reaches_a_real_grandchild` (Theme A) passes on
      `windows-latest` CI, not just locally
- [ ] `windows_live_sweep_process_listing_matches_a_real_process` (Theme B) passes on
      `windows-latest` CI
- [ ] `cargo fmt && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace -q` clean.

## Out of scope

- Running the live Firefox test tier on Windows CI at all — that is a much larger undertaking
  (headless Firefox availability on `windows-latest`, path/profile handling differences) than
  proving these three process-management primitives work, and iteration 197 did not claim it.
- Fixing anything in `kill_phase_tree` / `process_listing` / `kill_pid_hard` speculatively. If A
  or B fails, that is this iteration's actual finding and the fix belongs here, backed by the
  failing test — not guessed at up front.

## References

- [[iteration-197-live-sweep-has-no-per-test-timeout]] — where these three branches were added,
  and the carry-over row this plan resolves
- `crates/xtask/src/live_sweep.rs` — `kill_phase_tree`, `process_listing`, `kill_pid_hard`,
  `managed_firefox_pids`, `argv0`
- `.github/workflows/ci.yml` — `test (windows-latest)` job, the runner this plan's new tests
  must actually execute on
