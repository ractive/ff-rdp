//! Live tests for iteration 146 — live suite reliability.
//!
//! ## Theme A — the live-test harness's own teardown
//!
//! `LiveFirefox`'s `Drop` was already reliable (verified live before this
//! iteration: a throwaway probe test that panics mid-test leaves zero
//! surviving processes). The actual leak iter-146 found in a
//! full sequential sweep traced to `live_96_profile_cleanup.rs`'s
//! `launch_headless()`, which used to launch Firefox via a bare `Command`
//! with **no** RAII guard at all — see the fix and doc comment there. The
//! tests below pin the harness-wide guarantee so it can't regress silently:
//! every `LiveFirefox` must leave no surviving process once its guard drops,
//! even through a panic.
//!
//! # Running
//!
//!   FF_RDP_LIVE_TESTS=1 cargo test -p ff-rdp-cli --test live live_146 -- --nocapture

use std::panic::AssertUnwindSafe;

use crate::common::{LiveFirefox, live_tests_enabled, pid_alive};

/// Poll until `pid_alive(pid)` is `false` or `timeout` elapses, returning
/// the final liveness.
///
/// `kill_pid`'s `SIGKILL` is asynchronous — the kernel needs a moment to
/// actually reap the process, so a liveness probe taken immediately after
/// `Drop` can still observe "alive" for a few milliseconds. Every assertion
/// in this file that a Firefox PID is gone polls through this helper rather
/// than checking once, so it verifies the guard's eventual guarantee
/// (bounded and small — 2 s is generous headroom) instead of racing its own
/// probe against the kernel.
fn wait_until_dead(pid: u32, timeout: std::time::Duration) -> bool {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        if !pid_alive(pid) {
            return true;
        }
        if std::time::Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
}

/// AC: `live_146_no_orphan_firefox_after_suite` — after a small sequential
/// run of `LiveFirefox` launches — the shape a live suite takes —
/// zero of the Firefox processes they started are still alive once every
/// guard has dropped. Mirrors the dogfood_path's "zero ff-rdp-owned Firefox
/// processes remain" bar at a scale this test can run unattended.
#[test]
#[ignore = "requires Firefox — FF_RDP_LIVE_TESTS=1"]
fn live_146_no_orphan_firefox_after_suite() {
    const INSTANCES: usize = 3;

    if !live_tests_enabled() {
        return;
    }

    let mut pids = Vec::with_capacity(INSTANCES);
    for _ in 0..INSTANCES {
        let ff = LiveFirefox::headless_on_random_port();
        pids.push(ff.pid());
        // `ff` drops at the end of this iteration, killing this instance
        // before the next one launches — modeling a sequential suite run.
    }

    for pid in &pids {
        assert!(
            wait_until_dead(*pid, std::time::Duration::from_secs(2)),
            "live_146_no_orphan_firefox_after_suite: Firefox pid {pid} is still alive after \
             its LiveFirefox guard dropped"
        );
    }

    eprintln!(
        "live_146_no_orphan_firefox_after_suite: PASS — {}/{INSTANCES} sequential launches \
         left no survivor",
        pids.len()
    );
}

/// AC: `live_146_harness_teardown_kills_firefox_on_panic` — a test that
/// launches Firefox and then panics still leaves zero surviving processes
/// once its `LiveFirefox` guard drops (dropped as part of the panic's unwind,
/// exactly as `cargo test`'s own per-test harness does).
#[test]
#[ignore = "requires Firefox — FF_RDP_LIVE_TESTS=1"]
fn live_146_harness_teardown_kills_firefox_on_panic() {
    if !live_tests_enabled() {
        return;
    }

    let pid_cell = std::cell::Cell::new(None::<u32>);
    let outcome = std::panic::catch_unwind(AssertUnwindSafe(|| {
        // iter-158 Theme D: `headless_on_random_port` panics rather than
        // returning `None`. That panic is caught here just like the
        // intentional one below, so the `pid_cell`-is-empty arm underneath
        // means "Firefox never launched" and is reported as a failure
        // instead of a skip.
        let ff = LiveFirefox::headless_on_random_port();
        pid_cell.set(Some(ff.pid()));
        // `ff` is moved into and dies inside this closure's unwind — the
        // scenario under test: a live test that panics mid-assertion with
        // its Firefox still in scope.
        panic!("iter-146 probe: intentional panic with Firefox running");
    }));

    let pid = pid_cell.get().expect(
        "live_146_harness_teardown_kills_firefox_on_panic: Firefox never launched, so the \
         teardown guarantee under test was never exercised",
    );
    assert!(outcome.is_err(), "the probe closure always panics");
    assert!(
        wait_until_dead(pid, std::time::Duration::from_secs(2)),
        "live_146_harness_teardown_kills_firefox_on_panic: Firefox pid {pid} survived a panic"
    );
    eprintln!("live_146_harness_teardown_kills_firefox_on_panic: PASS — pid {pid} is gone");
}
