//! Live tests for iteration 151 — a residual live-suite Firefox leak that
//! survived iteration 146.
//!
//! See `kb/iterations/iteration-151-residual-live-firefox-leak.md` for the
//! full investigation. Summary of what this file locks in:
//!
//! Theme A ("name the leaking test") used to live here: it asserted a
//! `LiveFirefox`-spawned profile carried an `.ff-rdp-owner-test` marker naming
//! the spawning test. That marker is gone as of the 2026-10-02 reset phase 4b
//! second pass — every live test now launches into its own thread-isolated
//! `FF_RDP_HOME` (`tests/common/mod.rs`'s `ISOLATED_LIVE_HOME`), which removes
//! the cross-test bisection problem the marker existed to solve, rather than
//! solving it with a label.
//!
//! ## Theme B — the confirmed leak source(s)
//!
//! Two real, still-open instances of the exact "no RAII guard across an
//! assertion" bug class iteration 146 fixed in
//! `live_96_profile_cleanup.rs`'s `launch_headless`:
//!
//! 1. Four lifecycle tests (since deleted with the feature they tested) each
//!    wrapped their `LiveFirefox` guard in `std::mem::ManuallyDrop`
//!    immediately after spawning, so the command under test alone was
//!    responsible for killing Firefox — but every assertion between that
//!    point and the final liveness check ran with **no** guard at all. A
//!    failure in any of them panicked with Firefox still alive and nothing
//!    left to reap it. Fixed by removing the `ManuallyDrop` suppression — the guard now
//!    stays a normal binding for the rest of each function, so its `Drop`
//!    is a harmless no-op on the happy path and a real safety net on panic.
//! 2. `live_142_disk_growth.rs`'s `launch_headless` launched Firefox via a
//!    bare `Command` with no guard whatsoever and relied entirely on a
//!    *manual*, later `kill_pid` call for cleanup — this is the exact
//!    pre-146 `live_96_profile_cleanup.rs` shape, just never migrated when
//!    146 fixed that file. Fixed by adding `common::FirefoxGuard`, a small
//!    RAII wrapper over a raw PID (this file's launches need a custom
//!    `FF_RDP_HOME` env var that `common::LiveFirefox` doesn't expose, so it
//!    can't reuse that type outright).
//! 3. `launch --replace` starts a REPLACEMENT Firefox after reaping the prior
//!    instance, and a `LiveFirefox` guard owns only the PID it launched
//!    itself — so `live_86_perf_field_fixes.rs`'s
//!    `live_launch_replace_handles_stuck_prior` (and a since-deleted
//!    port-scoping test) each orphaned one Firefox on *every* run, happy path included. This class
//!    was missed by the original Theme B audit (which looked only for
//!    discarded guards, not for processes nothing ever owned) and is the
//!    better arithmetic fit for the measured ~1-orphan-per-100-tests rate
//!    than the intermittent `ManuallyDrop` panics of mechanism 1. Fixed by
//!    binding a `common::FirefoxGuard` over the replacement's `results.pid`
//!    before any assertion at both sites.
//!
//! [`live_151_root_cause_documented`] reproduces mechanism 1 live: it drives
//! both the pre-fix (`ManuallyDrop`) and fixed (normal binding) shapes
//! against a real Firefox process inside a `catch_unwind`'d panic and
//! asserts on actual PID liveness afterward — proof, not a hypothesis.
//!
//! Theme C ("whole-suite guarantee, testable in chunks") used to live here
//! too: `live_151_chunk_a_leaves_no_orphans` / `_b` nested a full chunk run as
//! a child process and scanned the real per-user profile root for survivors,
//! identified via Theme A's now-deleted marker. Removed in the same pass as
//! Theme A rather than only re-pointed: once every live test launches into
//! its own thread-isolated `FF_RDP_HOME`, nothing a nested child process
//! spawns lands under the real root any more, so a scan of that root would
//! report zero survivors whether or not anything actually leaked — a check
//! that cannot fail is not a check. It was never wired into CI (opt-in via
//! `FF_RDP_LIVE_SUITE_CHECK=1`, never set by any workflow) and is not
//! replaced here; a real whole-suite leak guarantee would need a shared,
//! scannable root threaded through the nested process tree, which is a
//! redesign out of scope for this pass.
//!
//! # Running
//!
//!   FF_RDP_LIVE_TESTS=1 cargo test-live -p ff-rdp-cli --test live live_151 -- --nocapture

use std::panic::AssertUnwindSafe;
use std::time::Duration;

use crate::common::{LiveFirefox, kill_pid, live_tests_enabled, pid_alive};

/// Poll until `pid_alive(pid)` is `false` or `timeout` elapses. Mirrors
/// `live_146_suite_reliability.rs`'s identical helper — `kill_pid`'s
/// `SIGKILL` is asynchronous, so a liveness probe taken immediately after a
/// kill can still observe "alive" for a few milliseconds.
fn wait_until_dead(pid: u32, timeout: Duration) -> bool {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        if !pid_alive(pid) {
            return true;
        }
        if std::time::Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

// ---------------------------------------------------------------------------
// Theme B — mechanism, proven live
// ---------------------------------------------------------------------------

/// AC: `live_151_root_cause_documented` — reproduces, live, the exact
/// mechanism this iteration's Theme B fixed: a `LiveFirefox` guard
/// suppressed via `ManuallyDrop` (the pre-fix pattern this iteration removed
/// from four lifecycle tests) leaves Firefox
/// alive when a panic strikes before cleanup runs; the fixed pattern (the
/// guard stays a normal binding) does not.
///
/// This drives both shapes against a real Firefox process and asserts on
/// actual PID liveness afterward — the same "prove it on the wire" bar
/// every `pre_fix_repro_*` test in this suite holds itself to, not a
/// restated hypothesis.
#[test]
#[ignore = "requires Firefox — FF_RDP_LIVE_TESTS=1"]
fn live_151_root_cause_documented() {
    if !live_tests_enabled() {
        return;
    }

    // --- Pre-fix shape: guard suppressed via ManuallyDrop before a panic ---
    let ff = LiveFirefox::headless_on_random_port();
    let leaked_pid = ff.pid();
    let outcome = std::panic::catch_unwind(AssertUnwindSafe(|| {
        // The exact pre-fix idiom: suppress Drop, then panic before any
        // cleanup code runs — modeling a failed stop assertion.
        let _keep = std::mem::ManuallyDrop::new(ff);
        panic!("live_151 probe: simulated assertion failure before cleanup (pre-fix shape)");
    }));
    assert!(outcome.is_err(), "probe closure must panic");

    // Give the (nonexistent) cleanup a moment it will never get.
    std::thread::sleep(Duration::from_millis(300));
    assert!(
        pid_alive(leaked_pid),
        "live_151_root_cause_documented: pid {leaked_pid} is already dead — expected it to \
         still be alive, proving the ManuallyDrop pattern leaks Firefox when a panic strikes \
         before cleanup runs (the pre-151 bug this iteration fixes)"
    );
    eprintln!(
        "live_151_root_cause_documented: confirmed — ManuallyDrop pattern leaked pid \
         {leaked_pid}; cleaning it up manually now"
    );
    kill_pid(leaked_pid);
    assert!(
        wait_until_dead(leaked_pid, Duration::from_secs(2)),
        "live_151_root_cause_documented: manual cleanup of the deliberately-leaked pid \
         {leaked_pid} failed — the test harness itself is broken"
    );

    // --- Fixed shape: guard stays a normal binding across the same panic ---
    let ff2 = LiveFirefox::headless_on_random_port();
    let protected_pid = ff2.pid();
    let outcome2 = std::panic::catch_unwind(AssertUnwindSafe(|| {
        // `ff2` is moved into the closure as a normal binding — the fixed
        // pattern. It drops (killing Firefox) as part of the panic's
        // unwind, exactly like every other live test in this suite.
        let _ff2 = ff2;
        panic!("live_151 probe: simulated assertion failure before cleanup (fixed shape)");
    }));
    assert!(outcome2.is_err(), "probe closure must panic");
    assert!(
        wait_until_dead(protected_pid, Duration::from_secs(2)),
        "live_151_root_cause_documented: FAIL — pid {protected_pid} survived a panic even \
         with the guard kept as a normal binding (the iter-151 fix regressed)"
    );

    eprintln!(
        "live_151_root_cause_documented: PASS — mechanism confirmed live: ManuallyDrop \
         leaked pid {leaked_pid}, the fixed pattern reaped pid {protected_pid} through the \
         same panic"
    );
}
