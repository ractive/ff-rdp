---
title: "Iteration 168: LiveFirefox::drop signals but never waits, so a killed Firefox still reads as alive"
type: iteration
date: 2026-08-16
status: planned
branch: iter-168/livefirefox-drop-waits-for-exit
depends_on: []
first_call_sites: []
dogfood_path: |
  # This is a TEST-HARNESS defect, so the repro path is the live suite under
  # contention, not an ff-rdp command. It does not reproduce on an idle
  # machine — that is the whole point, and it is why a single-test re-run
  # "proves" nothing.

  # 1. Confirm the two tests pass in isolation, so a later failure is
  #    attributable to ordering/contention and not to either test.
  FF_RDP_LIVE_TESTS=1 cargo test -p ff-rdp-cli --test live -q -- --ignored \
    live_128_meta_route --test-threads=1
  FF_RDP_LIVE_TESTS=1 cargo test -p ff-rdp-cli --test live -q -- --ignored \
    live_profiles_prune_removes_all_when_no_firefox_running --test-threads=1

  # 2. Run them adjacent, in that order, in ONE process — live_128 drops its
  #    LiveFirefox, live_96 then enumerates owner-PID markers.
  FF_RDP_LIVE_TESTS=1 cargo test -p ff-rdp-cli --test live -q -- --ignored \
    --test-threads=1 live_128_meta_route \
    live_profiles_prune_removes_all_when_no_firefox_running
  # → EXPECTED (predicted from reading the code on 2026-08-16, NOT yet
  #   reproduced deliberately): intermittently, live_96's precondition fires
  #   with "precondition violated" naming live_128_meta_route as the owner
  #   test. Observed once for real in iter-165's dual-gate sweep (pid 59653).
  #   RUN THIS FIRST and record how many runs out of N it takes.

  # 3. If step 2 will not reproduce, add load — the iter-158 sweep saw it at
  #    load average 18.6. Any CPU burner will do; record what you used.

  # 4. Direct observation of the race, independent of both tests: after
  #    kill_pid returns, poll pid_alive and record how long it stays true.
  #    A non-zero window is the defect; the exact number sizes the fix.
tags: [iteration, testing, live-tests, harness]
---

# Iteration 168: `LiveFirefox::drop` signals but never waits for exit

Carry-over from [[iteration-165-eval-scope-leak-contradicts-help]]. The finding was written into
PR #203's live-sweep section and correctly diagnosed as suite hygiene, but it was described in
prose rather than filed, so neither that iteration's carry-over sweep nor its review pass gave it
a disposition. This plan is that disposition.

## What was observed

iter-165's dual-gate sweep reported 260 passed / 2 failed. One failure was
`live_96_profile_cleanup::live_profiles_prune_removes_all_when_no_firefox_running`, whose
precondition fired because a Firefox owned by `live_128_meta_route` (pid 59653) still held an
`ff-rdp` profile directory. It passed on an idle isolated re-run.

**The precondition is not the bug — it is the detector, and it worked.** iter-146 Theme B
deliberately replaced a `daemon status` skip-check with this loud failure so the condition could
not stay invisible, and iter-151 Theme A added `OWNER_TEST_MARKER` so the message names the
culprit test instead of leaving the reader to bisect the suite. Both paid off here: the failure
message named `live_128_meta_route` directly. Do not "fix" this by softening the precondition
back into a skip.

## The mechanism (read from the code, not yet measured)

`live_128_meta_route` holds a `LiveFirefox` RAII guard, so on any exit path — including its two
early `return`s and an assertion unwind — `Drop` runs:

```rust
// crates/ff-rdp-cli/tests/common/mod.rs:638
impl Drop for LiveFirefox {
    fn drop(&mut self) {
        kill_pid(self.firefox_pid);
    }
}
```

`kill_pid` sends `SIGKILL` and returns immediately (`mod.rs:248`); it does not wait for the
process to actually go away, and the test process is not Firefox's parent, so it never reaps it
either. `live_96`'s precondition asks the complementary question:

```rust
// crates/ff-rdp-cli/tests/common/mod.rs:189
let rc = unsafe { libc::kill(pid.cast_signed(), 0) };
```

`kill(pid, 0)` succeeds for a process that has been signalled but has not yet been scheduled to
die or been reaped. So between `Drop` returning and the kernel finishing the kill there is a
window in which the profile's owner PID still reads as alive, and `prune --all` correctly refuses
to delete a profile it believes is in use.

The window is negligible on an idle machine and grows with load — which is exactly why this
surfaced in a contended sweep and vanished on the isolated re-run. It is the same shape as the
defect iter-164 fixed in `LiveFirefox::with_daemon`: a signal-and-hope where a bounded poll
belongs. Every `LiveFirefox` user inherits it, not just `live_128_meta_route`; that test is
simply the one that happens to run before `live_96`.

## Why 146 and 151 did not already cover this

This is the third iteration to touch leaked live Firefox processes, and it is **not** a repeat of
the first two. Both predecessors fixed *guard coverage* — whether an RAII guard existed and
survived an assertion unwind. [[iteration-146-live-suite-reliability]] Theme A fixed the missing
guard in `live_96_profile_cleanup.rs`; [[iteration-151-residual-live-firefox-leak]] audited the
rest of the suite and removed four `ManuallyDrop` suppressions plus one bare-`Command` launch,
proving the fix live against a `catch_unwind`'d panic.

Neither touched what `Drop` *does once it runs*. `live_128_meta_route` has a correct guard, on
every exit path, and it fires — the guard is not the problem. The problem is that firing it is
not the same as the process being gone, and every fix so far has implicitly assumed it was. So
the predecessors' audits were sound and their conclusions still hold; this is a different failure
at a layer neither examined.

## Themes

- **A — Measure the window before changing anything.** Run the `dogfood_path`. Record how long
  `pid_alive` stays true after `kill_pid` returns, idle and under load, and how many adjacent
  runs it takes to reproduce the precondition failure. If the window is unmeasurable and the
  ordering repro will not fire even under load, the mechanism above is wrong — say so in this
  plan and close the iteration `obsolete` rather than hardening against an imagined race.
- **B — Make `Drop` wait, bounded.** `kill_pid` then poll `pid_alive` until it goes false or a
  bounded timeout expires. The timeout must not hang a test run: pick it from Theme A's measured
  window with headroom, make it overridable by env var the way iter-164 did for the daemon
  budget, and decide explicitly what a timeout does — a loud `eprintln!` naming the pid and the
  owner test is the minimum, since a silent give-up recreates today's behaviour exactly.
- **C — Decide whether the profile directory is also `Drop`'s job.** Waiting for exit fixes the
  liveness read, but the profile dir is still left behind for `prune` to collect. Check whether
  that is intended (a deliberate artifact for post-mortem inspection) or merely unhandled, and
  record which. Removing it may be out of scope; deciding is not.

## Tasks

### A. Measure
- [ ] Run every step of `dogfood_path` and paste actual outputs into this plan
- [ ] Record the post-`kill_pid` `pid_alive` window, idle and under load
- [ ] Record how many adjacent `live_128` → `live_96` runs it takes to reproduce

### B. Fix the harness
- [ ] `LiveFirefox::drop` waits for the pid to actually disappear, bounded, with an env override
- [ ] Timeout path emits a loud diagnostic naming the pid and the owning test
- [ ] Unit tests: a pid that exits promptly returns fast; an already-dead pid does not block;
      the timeout path is exercised without a real Firefox

### C. Profile-directory ownership
- [ ] Record in this plan whether leaving the profile dir behind is deliberate, and the evidence

## Acceptance Criteria [0/4]

- [ ] `unit_168_drop_waits_for_pid_exit`: the wait helper returns promptly for a process that
      exits, returns immediately for an already-dead pid, and hits its bounded timeout for a pid
      that never dies — all without launching Firefox
- [ ] `live_168_adjacent_tests_leave_no_live_owner`: after a `LiveFirefox` is dropped,
      `live_owned_profile_dirs` reports no entry owned by that pid, asserted against a real
      launched Firefox
- [ ] The Theme A measurements are recorded in this plan, including the reproduce rate — or, if
      it did not reproduce, that fact and the decision that follows from it
- [ ] `cargo fmt && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace -q` clean,
      plus a dual-gate live sweep whose `live_96` and `live_128` both pass in the same run

## Design notes

The fix belongs in `Drop`, not in `live_96`'s precondition and not in `live_128`. The precondition
is the detector and must stay loud. `live_128` is one of many `LiveFirefox` users and is not
special — fixing it alone would leave the same race for every other test that drops a guard before
a profile-scanning test runs.

PID reuse is a known hazard of any `kill(pid, 0)` liveness check: once the pid is freed the OS may
reassign it, and the marker would then name an unrelated process. Waiting for exit makes the
window smaller, not zero. Out of scope to fix here, but if Theme A trips over it, record it.

Not every `LiveFirefox` drop happens on a passing path — the guard also drops during an assertion
unwind. The wait must therefore be cheap and non-panicking; a `Drop` that panics while unwinding
aborts the process and would turn one failing test into a suiteless run.

## Out of scope

- Softening `live_profiles_prune_removes_all_when_no_firefox_running`'s precondition back into a
  skip. iter-146 Theme B removed that on purpose; re-adding it would re-hide this class of defect.
- Reworking `ff-rdp launch`'s profile lifecycle in the product. This is a test-harness iteration;
  any product-side finding gets filed, not fixed here.
- Parallelising the live suite. The race is visible under `--test-threads=1` and would only get
  worse with concurrency; that is a separate question.

## References

- [[iteration-165-eval-scope-leak-contradicts-help]] — the sweep that surfaced this, and the
  carry-over row it did not get
- [[iteration-164-two-failures-the-158-sweep-uncovered]] — the same signal-and-hope → bounded-poll
  fix applied to `LiveFirefox::with_daemon`'s daemon readiness wait
- [[iteration-146-live-suite-reliability]] — Theme B, which made this precondition fail loudly
  instead of skipping, and Theme A, which fixed guard *coverage* in `live_96_profile_cleanup.rs`
- [[iteration-151-residual-live-firefox-leak]] — Theme A, the `OWNER_TEST_MARKER` that named the
  culprit here, and the audit that migrated guard coverage to the rest of the suite
