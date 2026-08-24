---
title: "Iteration 208: capture a stack the next time live_158_launch_survives_contended_bind hangs"
type: iteration
date: 2026-08-24
status: planned
branch: iter-208/live-158-contended-bind-hang-diagnosis
depends_on: [197]
first_call_sites: []
dogfood_path: |
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
tags: [iteration, testing, live-tests, flaky, carry-over]
---

# Iteration 208: the hang iteration 197 could not reproduce, made catchable next time

## Where this came from

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

## What this iteration is, given the hang has not recurred

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
   see [[iteration-209-live-sweep-windows-process-paths-untested]]) against the test binary's pid
   **before** the kill, and write it next to wherever the sweep already writes its own output.
   This must not become a second general-purpose feature: gate it to fire only for this one named
   test, so a routine timeout on an unrelated test does not start shelling out to a debugger.
3. Nothing here should widen `--phase-stall-secs` or otherwise change the watchdog's behavior —
   197 already picked that bound and justified it; this iteration only adds a side-effect on the
   way to the kill.

## Tasks

### A. Enumerate the blocking candidates [0/1]
- [ ] Every call between test start and `FF_RDP_LIVE_LAUNCH_TIMEOUT_SECS`'s bound is listed with
      file:line, and each is marked as either already bounded (cite the bound) or not

### B. Capture hook [0/2]
- [ ] A stack-sampling capture runs against the hung test binary's pid, gated to fire only when
      the timed-out name is `live_158_launch_survives_contended_bind`, and only *before*
      `kill_phase_tree` sends its signal
- [ ] The capture's output path is printed by `live-sweep` in the same `WATCHDOG` report that
      names the unreported test, so a later reader does not have to know where to look

## Acceptance Criteria [0/2]

- [ ] `live_208_capture_hook_fires_only_for_the_named_test`: given a `timed_out` set containing
      an unrelated test name, the capture hook does not run; given a set containing
      `live_158_launch_survives_contended_bind`, it does
- [ ] `cargo fmt && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace -q` clean.

## Out of scope

- Actually fixing the hang. There is no confirmed cause to fix; inventing one to close this plan
  would be worse than leaving it open (iteration 197's own conclusion).
- A general stack-capture facility for any timed-out test. Scoped to this one name until a second
  test demonstrates the same failure shape.
- The Windows side of any of this — no stack sampler is chosen here for that platform; see
  [[iteration-209-live-sweep-windows-process-paths-untested]].

## References

- [[iteration-197-live-sweep-has-no-per-test-timeout]] — where the watchdog that finally bounds
  this hang was built, and the plan row this carries over
- [[iteration-188-live-sweep-cost-and-parallelism]] — the original observation (2026-08-23, third
  sweep, `--jobs 4`)
- `crates/xtask/src/live_sweep.rs` — `kill_phase_tree`, `unreported_tests`, `slow_flagged_tests`
- `crates/ff-rdp-cli/tests/live/live_158_launch_lifecycle.rs` —
  `live_158_launch_survives_contended_bind`
