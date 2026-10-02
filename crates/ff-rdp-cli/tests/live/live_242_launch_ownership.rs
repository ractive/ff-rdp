//! iter-242 Part B — an ownership property that needs a real process to
//! prove, rather than a source scan.
//!
//! `iter_242_launch_site_ownership.rs` (an ordinary, Firefox-free test)
//! enumerates every `"launch"` invocation under `tests/live/` and asserts each
//! one is routed through `common::ff_rdp_launch_command()` and sits in a
//! function that binds an RAII owner. That is a claim about the *source*. The
//! test here is the claim about *behaviour*: `common::FirefoxGuard::drop` does
//! not signal a PID that is already gone, which is the recycled-PID hazard
//! iter-110 guards against in production and which iteration 151 reintroduced
//! at test scope by removing the `ManuallyDrop` that was incidentally
//! preventing it.
//!
//! A sibling test used to live here proving that a profile created by a
//! direct `Command`-built launch site still names its spawning test in
//! `.ff-rdp-owner-test`. That marker (and the `FF_RDP_LIVE_TEST_NAME` env var
//! that fed it) is gone as of the 2026-10-02 reset phase 4b second pass: every
//! live test now launches into its own thread-isolated `FF_RDP_HOME` (see
//! `common::ISOLATED_LIVE_HOME`), so a leaked profile can no longer land
//! anywhere a sibling test — or a human — would need to bisect to find, which
//! was the only reason the attribution marker existed.

use std::time::Duration;

use crate::common::{FirefoxGuard, pid_alive};

/// AC (iter-242 Part B Theme D): a guard whose PID was already reaped does not
/// signal it on drop.
///
/// Needs no Firefox — a spawned-and-reaped child is a dead PID like any other
/// — but lives here because `FirefoxGuard` is a `tests/live` helper. Proving a
/// *negative* about a signal is done the only way it can be at this level: the
/// guard is given a PID that is already dead, and dropping it must neither
/// panic (a panic in `Drop` during unwind aborts the whole test binary) nor
/// leave the process table changed. The stronger statement — `disarm()` — is
/// asserted alongside, because it does not depend on losing the race against
/// PID reuse to be correct.
// allow-ungated-live: needs no Firefox — it spawns and reaps a trivial child to
// get a dead PID, so it is a fast, network-free probe of a `tests/live` helper.
// Gating it behind FF_RDP_LIVE_TESTS would mean the guard's Drop contract is
// only ever checked on a machine that has Firefox, which is the opposite of
// where a regression here would hurt most.
#[test]
fn live_242_guard_drop_skips_dead_pid() {
    #[cfg(unix)]
    let mut child = std::process::Command::new("true")
        .spawn()
        .expect("spawn `true`");
    #[cfg(windows)]
    let mut child = std::process::Command::new("cmd")
        .args(["/C", "exit", "0"])
        .spawn()
        .expect("spawn cmd exit");
    let dead_pid = child.id();
    child.wait().expect("child exits");

    // Windows keeps the process *object* alive while any handle to it remains,
    // so `OpenProcess` — and therefore `pid_alive` — can still succeed for a
    // PID whose process has exited, sometimes for seconds under a CI job
    // object. Poll rather than assume, and if the OS never agrees the PID is
    // dead, say so and skip the half of this test that needs it instead of
    // asserting something about a process the OS still considers present.
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while pid_alive(dead_pid) && std::time::Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
    }

    if pid_alive(dead_pid) {
        eprintln!(
            "live_242_guard_drop_skips_dead_pid: pid {dead_pid} still reads as alive after \
             exiting — the OS has not released the process object, so the dead-PID half is \
             not observable here; asserting the disarm half only"
        );
    } else {
        // Dropping a guard over a dead PID must be a no-op — in particular it
        // must not sit in `kill_pid_and_wait`'s bounded wait, which would
        // report a spurious "still alive after SIGKILL" line for a process that
        // exited before the guard was even built.
        let started = std::time::Instant::now();
        drop(FirefoxGuard::new(dead_pid));
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "dropping a guard over a dead PID must not enter the kill-and-wait loop; took {:?}",
            started.elapsed()
        );
    }

    // A disarmed guard hands its PID back and forgets it, so `Drop` has
    // nothing to signal even while the process is still alive.
    let guard = FirefoxGuard::new(std::process::id());
    assert_eq!(guard.pid(), std::process::id());
    assert_eq!(guard.disarm(), std::process::id());
    assert!(
        pid_alive(std::process::id()),
        "the disarmed guard must not have signalled this very process"
    );
}
