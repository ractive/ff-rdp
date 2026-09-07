//! iter-242 Part B — the two ownership properties that need a real process to
//! prove, rather than a source scan.
//!
//! `iter_242_launch_site_ownership.rs` (an ordinary, Firefox-free test)
//! enumerates every `"launch"` invocation under `tests/live/` and asserts each
//! one is routed through `common::ff_rdp_launch_command()` and sits in a
//! function that binds an RAII owner. That is a claim about the *source*. The
//! two tests here are the claims about *behaviour*:
//!
//! - a profile created by a direct `Command`-built launch — not
//!   `common::LiveFirefox` — still names its spawning test in
//!   `.ff-rdp-owner-test`, so the `spawned by unknown test` signature that sent
//!   iteration 176 hunting through the whole suite cannot come from the live
//!   tier;
//! - `common::FirefoxGuard::drop` does not signal a PID that is already gone,
//!   which is the recycled-PID hazard iter-110 guards against in production and
//!   which iteration 151 reintroduced at test scope by removing the
//!   `ManuallyDrop` that was incidentally preventing it.

use std::time::Duration;

use crate::common::{
    FirefoxGuard, OWNER_TEST_MARKER, current_test_name, ff_rdp_launch_command, live_tests_enabled,
    pid_alive,
};

/// A port nothing is listening on, discovered by binding `:0` and releasing it.
fn free_port() -> Option<u16> {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").ok()?;
    listener.local_addr().ok().map(|a| a.port())
}

/// AC (iter-242 Part B): a profile created by a raw-`Command` launch site
/// names its spawning test.
///
/// Deliberately *not* driven through `common::LiveFirefox`: iteration 151's
/// `FF_RDP_LIVE_TEST_NAME` instrumentation only ever covered that one route,
/// which is why the ~20 direct `ff-rdp launch` call sites across the live tier
/// went on producing markers that read `spawned by unknown test`. This test
/// spawns exactly the way those sites do.
///
/// The profile is created under an isolated `$FF_RDP_HOME` so the assertion is
/// about this launch's own directory and not about whatever else is on the
/// machine — the ambiguity that broke iteration 188's first parallel sweep.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_242_marker_names_test_from_direct_launch() {
    if !live_tests_enabled() {
        eprintln!("live_242_marker_names_test_from_direct_launch: set FF_RDP_LIVE_TESTS=1");
        return;
    }

    let home = tempfile::tempdir().expect("tempdir for FF_RDP_HOME");
    let Some(port) = free_port() else {
        eprintln!("live_242_marker_names_test_from_direct_launch: no free port — skipping");
        return;
    };

    let out = ff_rdp_launch_command()
        .env("FF_RDP_HOME", home.path())
        .args(["launch", "--headless", "--debug-port", &port.to_string()])
        .output()
        .expect("spawn `ff-rdp launch`");
    // Own the PID before asserting on anything (iter-242 Theme B).
    let guard = crate::common::guard_launched_firefox(&out);
    assert!(
        out.status.success(),
        "live_242_marker_names_test_from_direct_launch: launch exited {}\n{}",
        out.status,
        crate::common::output_note(&out)
    );
    let _guard = guard.expect("a successful launch must report results.pid");

    let json: serde_json::Value =
        serde_json::from_slice(&out.stdout).expect("launch stdout is JSON");
    let profile_path = json["results"]["profile_path"]
        .as_str()
        .expect("launch reports results.profile_path")
        .to_owned();

    let marker = std::path::Path::new(&profile_path).join(OWNER_TEST_MARKER);
    let recorded = std::fs::read_to_string(&marker).unwrap_or_else(|e| {
        panic!(
            "live_242_marker_names_test_from_direct_launch: no {} in {profile_path}: {e} — a \
             profile leaked from this launch site would read `spawned by unknown test`",
            marker.display()
        )
    });
    let recorded = recorded.trim();
    assert!(
        recorded.contains("live_242_marker_names_test_from_direct_launch"),
        "the owner-test marker must name this test, got {recorded:?} (current_test_name() = {:?})",
        current_test_name()
    );
}

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
    std::thread::sleep(Duration::from_millis(50));
    assert!(
        !pid_alive(dead_pid),
        "precondition: the reaped child must read as dead"
    );

    // Dropping a guard over a dead PID must be a no-op — in particular it must
    // not sit in `kill_pid_and_wait`'s bounded wait, which would report a
    // spurious "still alive after SIGKILL" line for a process that exited
    // before the guard was even built.
    let started = std::time::Instant::now();
    drop(FirefoxGuard::new(dead_pid));
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "dropping a guard over a dead PID must not enter the kill-and-wait loop; took {:?}",
        started.elapsed()
    );

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
