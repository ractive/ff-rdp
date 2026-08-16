---
title: "Iteration 168: LiveFirefox::drop signals but never waits, so a killed Firefox still reads as alive"
type: iteration
date: 2026-08-16
status: done
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
tags:
  - iteration
  - testing
  - live-tests
  - harness
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

> **Superseded in part by the Theme A measurements below — kept verbatim as the hypothesis this
> iteration set out to test.** Three claims in this section did not survive contact with a
> measurement: the window is *not* negligible on an idle machine (16–27 ms, consistently), it does
> *not* grow with load (an 8× load increase left it unchanged), and `live_128_meta_route` does
> *not* "happen to run before `live_96`" in any meaningful sense — 176 tests separate them. See
> A1–A3.

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

## Theme A — what was actually measured (2026-08-16)

Machine: macOS 25.5.0 (Darwin, Apple silicon), Firefox headless via `ff-rdp launch`, debug build.

### A1 — the post-`SIGKILL` liveness window (`dogfood_path` step 4)

Measured **before** any code change, with a standalone `rustc`-compiled probe that launches
Firefox via `ff-rdp launch --headless`, `SIGKILL`s the reported pid, and spin-polls `kill(pid, 0)`
until it reports `ESRCH`.

```text
already-reaped child pid 9948: pid_alive=false        # control: a reaped pid reads dead at once

# load averages 6.50 7.08 9.32
run 0: pid  9956 kill_pid returned in 16.584µs; pid_alive stayed true for 26.785ms
run 1: pid 10040 kill_pid returned in 30.084µs; pid_alive stayed true for 21.460209ms
run 2: pid 10094 kill_pid returned in 21.709µs; pid_alive stayed true for 19.028584ms
run 3: pid 10152 kill_pid returned in 15.209µs; pid_alive stayed true for 22.450375ms
run 4: pid 10205 kill_pid returned in 19.959µs; pid_alive stayed true for 21.893334ms
n=5 min=19.028584ms max=26.785ms mean=22.3235ms

# load averages 28.62 → 54.01 (12 CPU burners added)
run 0: pid 10946 kill_pid returned in 19.791µs; pid_alive stayed true for 18.376166ms
run 1: pid 11005 kill_pid returned in 37.208µs; pid_alive stayed true for 24.859958ms
run 2: pid 11087 kill_pid returned in 28.125µs; pid_alive stayed true for 16.093875ms
run 3: pid 11136 kill_pid returned in 35.375µs; pid_alive stayed true for 16.73125ms
run 4: pid 11191 kill_pid returned in 46.75µs; pid_alive stayed true for 17.809458ms
n=5 min=16.093875ms max=24.859958ms mean=18.774141ms
```

**The window is real, unconditional and 1000× wider than the `kill` call itself: `kill_pid`
returns in ~20 µs, the pid keeps reading as alive for ~16–27 ms.** 10/10 launches. It did **not**
grow with CPU load — the plan predicted it would, and that prediction is wrong: an 8× load
increase moved the mean *down* (22.3 ms → 18.8 ms, i.e. inside run-to-run noise). Whatever sets
this window, it is not CPU contention.

### A2 — the ordering repro (`dogfood_path` steps 1–3): **did not reproduce, 0/10**

```text
# pristine origin/main worktree (9bbf539), 16 CPU burners, adjacent pair per run
run 1: pass (load: 106.53 123.11 74.55)
run 2: pass (load: 154.56 132.88 80.04)
…
run 10: pass (load: 268.75 208.96 129.20)
BASELINE REPRO SUMMARY: 0 / 10 runs tripped the precondition (burners=16)
```

Load averages 106→268 — far past the 18.6 at which iter-158's sweep saw trouble — and the
precondition never fired. (An earlier 8-run attempt in the working tree is discarded as
contaminated: `cargo test` recompiles `common/mod.rs` on every invocation, so this iteration's own
edits leaked into runs 4–8. The 10 runs above are from a separate `git worktree` on `origin/main`.)

### A3 — why A2 could never have reproduced, and what that means for the premise

The plan's mechanism assumed `live_128_meta_route` and `live_96`'s prune test run adjacently.
They do not. In the live binary's `--test-threads=1` execution order:

```text
$ cargo test -p ff-rdp-cli --test live -- --ignored --list
 25: live_128_network_output_fidelity::live_128_meta_route
201: live_96_profile_cleanup::live_profiles_prune_removes_all_when_no_firefox_running
(260 tests total)
```

**176 tests — minutes — separate them.** A 27 ms window cannot survive that by four orders of
magnitude. `live-sweep` does not close the gap either: it shells out per phase with
`--test-threads=1 --exact`, so the wall-clock distance is if anything larger.

So: the window this iteration fixes is real and now closed, but **it is not what caused iter-165's
`live_96` failure**. That failure remains unexplained by this plan's mechanism. The leading
remaining hypothesis is the one this plan's own Design notes flag as out of scope — **a stale
`.ff-rdp-owner-pid` marker plus PID reuse**: `LiveFirefox::drop` leaves the profile directory on
disk (Theme C below), so its owner-PID marker outlives the process for the rest of the sweep, and
any later process assigned that pid makes the dead profile read as live-owned. That is filed as
[[iteration-171-stale-owner-pid-marker-and-pid-reuse]]; it is **not** fixed here.

The plan's stated `obsolete` condition — window unmeasurable **and** repro will not fire — is not
met: the window measured cleanly at 16–27 ms, 10/10. The iteration proceeds on that basis, with
the causal claim narrowed to what the evidence supports.

### A4 — PID reuse (Design-notes hazard, tripped over as predicted)

Recorded per the plan's instruction. Not fixed here; see A3 and iteration 169.

## Theme C — is leaving the profile directory behind deliberate?

**Decision: it is unhandled, not deliberate — and it is now load-bearing rather than cosmetic.**

Evidence:
- `LiveFirefox`'s own doc comment says so in as many words: *"the temporary profile created by
  `ff-rdp launch` is left for the OS to reap (deferred to a future cleanup pass — see iter-61o
  notes)"*. A deliberate post-mortem artifact would not be labelled "deferred".
- The product ships two collectors for these directories — `profiles prune` and `daemon stop`'s
  `profile_removed_path` — and `live_96_profile_cleanup` tests both. The harness relies on the
  product to clean up after it; nothing in the harness owns the lifecycle.
- Nothing reads a left-behind profile after the fact: no test, no doc, no tooling.

**Not removed in this iteration** (the plan permits deferring removal: *"Removing it may be out of
scope; deciding is not"*). Reason to defer rather than fold in: A3 promotes the stale marker from
housekeeping to prime suspect, so the change now needs its own measurement and its own sweep
rather than a late unvalidated addition to this one. Filed as
[[iteration-171-stale-owner-pid-marker-and-pid-reuse]].

## Tasks

### A. Measure
- [x] Run every step of `dogfood_path` and paste actual outputs into this plan
- [x] Record the post-`kill_pid` `pid_alive` window, idle and under load — 16–27 ms, 10/10, and
      it does **not** grow with load (see A1)
- [x] Record how many adjacent `live_128` → `live_96` runs it takes to reproduce — it does not:
      0/10 on pristine `main` at load 106→268, because the two tests are 176 tests apart (A2/A3)

### B. Fix the harness
- [x] `LiveFirefox::drop` waits for the pid to actually disappear, bounded, with an env override
      (`kill_pid_and_wait`, `FF_RDP_TEST_KILL_WAIT_TIMEOUT_MS`, default 5 s ≈ 185× the measured
      worst case). `FirefoxGuard::drop` and `try_launch`'s abandoned-launch path get the same
      treatment; `RawFirefox::drop` already blocks in `Child::wait`, which is strictly stronger.
- [x] Timeout path emits a loud diagnostic naming the pid and the owning test — written through
      `std::io::stderr()` rather than `eprintln!`, which panics on write failure and would abort
      the process when `Drop` runs on an unwinding thread
- [x] Unit tests: a pid that exits promptly returns fast; an already-dead pid does not block;
      the timeout path is exercised without a real Firefox

### C. Profile-directory ownership
- [x] Record in this plan whether leaving the profile dir behind is deliberate, and the evidence

## Live sweep (2026-08-16)

```text
FF_RDP_LIVE_TESTS=1 FF_RDP_LIVE_NETWORK_TESTS=1 cargo run -p xtask -- live-sweep
LIVE_SWEEP_SUMMARY executed=270 skipped=0 preexisting=0 total=270
→ 260 passed / 10 failed
```

With a hand-started `firefox -no-remote -headless --start-debugger-server 6000`, so the
`ff-rdp-core` tier executed rather than being classified `preexisting`. Corpus grew from iter-165's
262 to 270 (this iteration's `live_168_*`, plus the tests iters 166/167 merged in the meantime), so
this is a **larger** corpus, not a shrunken one.

`live_96_profile_cleanup::live_profiles_prune_removes_all_when_no_firefox_running` **passed** —
the failure this iteration was filed against did not recur. `live_168_adjacent_tests_leave_no_live_owner`
passed. All ten failures are enumerated with dispositions in the PR's Carry-over table; every one
of them passes on re-run, so none is a deterministic regression:

| test | failure | disposition |
|---|---|---|
| `live_128_meta_route` | daemon autostart read an **empty** registry file (`EOF while parsing a value at line 1 column 0`), burned its 20 s budget, fell back to `route: "direct"` | [[iteration-172-daemon-registry-torn-read-on-autostart]] |
| `live_138_navigate_reports_200` | `status: null`, `status_reason: "no_status_reported"` under the daemon | [[iteration-169-navigate-status-delivery-and-nav-verb-parity]] |
| `live_138_navigate_reports_404` | same | [[iteration-169-navigate-status-delivery-and-nav-verb-parity]] |
| 7 × `ff-rdp-core` live tests | `ConnectionRefused` on port 6000 — the hand-started browser did not survive the 831 s CLI tier; 7/7 pass against a fresh one | [[iteration-173-live-sweep-port-6000-firefox-does-not-survive]] |

## Acceptance Criteria [3/4]

- [x] `unit_168_drop_waits_for_pid_exit`: the wait helper returns promptly for a process that
      exits, returns immediately for an already-dead pid, and hits its bounded timeout for a pid
      that never dies — all without launching Firefox
      (`crates/ff-rdp-cli/tests/iter_168_harness_kill_wait.rs`)
- [x] `live_168_adjacent_tests_leave_no_live_owner`: after a `LiveFirefox` is dropped,
      `live_owned_profile_dirs` reports no entry owned by that pid, asserted against a real
      launched Firefox. Verified as a real detector, not a tautology: **fails 3/3 on a pristine
      `origin/main` worktree** (`pid 33543/33596/33649 still reads as alive the instant
      LiveFirefox::drop returned`) and passes on this branch.
- [x] The Theme A measurements are recorded in this plan, including the reproduce rate — or, if
      it did not reproduce, that fact and the decision that follows from it (see A1–A4; the repro
      rate is 0/10 and the decision is in A3)
- [ ] `cargo fmt && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace -q` clean,
      plus a dual-gate live sweep whose `live_96` and `live_128` both pass in the same run
      — **left unticked: the second clause did not hold.** fmt/clippy/`cargo test --workspace -q`
      are clean and the dual-gate sweep ran (`executed=270 skipped=0 preexisting=0`), and `live_96`
      passed, but `live_128_meta_route` failed in that same run — on the daemon-registry torn read
      filed as [[iteration-172-daemon-registry-torn-read-on-autostart]], which has nothing to do
      with this iteration's change and passes on re-run. The AC is not reworded to say "for an
      unrelated reason": as written it is unmet, and an honest empty box is the only signal a later
      reader gets.

**Not claimed, deliberately:** that this iteration fixes the `live_96` precondition failure
observed in iter-165. See A3 — the measured window is four orders of magnitude too small to
explain it, and no AC above asserts otherwise.

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
