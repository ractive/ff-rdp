//! Live tests for iter-96 Theme A (`daemon stop` profile cleanup).
//!
//! Every `ff-rdp launch` (without `--profile`) creates a fresh profile dir
//! under `secure_profile_root()` and never removed it — see
//! `kb/iterations/iteration-96-profile-leak-cleanup.md`. These tests assert
//! the fix: `daemon stop` removes the directory once the
//! SIGTERM→SIGKILL→killpg escalation ladder confirms Firefox is gone and the
//! port is free.
//!
//! Theme C's test (`ff-rdp profiles prune --all` removes every managed
//! orphan directory on demand) used to live here too, seeded under the real
//! per-user profile root with a named-PID precondition guarding against a
//! dirty environment. iter-188 Theme C made the live tier run concurrently,
//! which that global precondition could no longer satisfy, so the test was
//! moved onto an isolated per-test `$FF_RDP_HOME` — at which point it no
//! longer needed Firefox at all (the precondition can never fire in a root
//! nothing else touches) and became a strict duplicate of
//! `tests/e2e/profiles.rs::profiles_prune_is_scoped_to_ff_rdp_home`, which
//! asserts the identical seed/prune/assert-removed sequence without paying
//! for a live-tier slot. It was deleted here in favour of that e2e test
//! (found in review of iteration 188's PR). What it does **not** replace:
//! the whole-suite guarantee that a completed live-sweep run leaves no
//! live-owned managed profile behind in the *real* per-user root — that
//! guarantee currently has no test anywhere and is filed as
//! `kb/iterations/iteration-202-live-sweep-lost-its-real-root-orphan-guarantee.md`.
//!
//! Run with:
//!   FF_RDP_LIVE_TESTS=1 cargo test-live -p ff-rdp-cli \
//!       --test live live_96_profile_cleanup -- --nocapture

use std::process::Command;
use std::time::Duration;

use crate::common::LiveFirefox;
use crate::common::ff_rdp_bin;
use crate::common::live_tests_enabled;

/// Poll until the path at `path` no longer exists, or `timeout` elapses.
fn wait_path_gone(path: &str, timeout: Duration) -> bool {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        if !std::path::Path::new(path).exists() {
            return true;
        }
        if std::time::Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// Launch Firefox headless and return `(LiveFirefox, results)` where
/// `results` is the `results` object of the `launch` JSON envelope.
/// Returns `None` if the launch fails.
///
/// iter-146 Theme A: this used to be a bare `Command::new(ff_rdp_bin())`
/// launch returning only `(port, results)` — no RAII guard at all. Every
/// other live suite kills its Firefox via `LiveFirefox`'s `Drop` (robust
/// even through a panic — verified live in iter-146), but this file relied
/// entirely on `daemon stop` succeeding to clean up, with **no fallback**.
/// If `daemon stop` ever failed (or an assertion between launch and `daemon
/// stop` panicked — e.g. under the CPU contention a full sequential suite
/// run creates), Firefox leaked with nothing to reap it: the exact PID/args
/// shape of the four orphaned Firefox instances iter-146 found alive after
/// a live sweep (`firefox -no-remote --start-debugger-server … --headless
/// --profile …/ff-rdp-profile-…`) matches precisely what this helper
/// produces. Returning a `LiveFirefox` here makes `daemon stop`'s own
/// cleanup (still asserted below) belt, and the guard's `Drop` suspenders —
/// on any panic after this call, Firefox dies anyway.
fn launch_headless() -> (LiveFirefox, serde_json::Value) {
    let (ff, envelope) = LiveFirefox::headless_on_random_port_with_args(&[]);
    // `headless_on_random_port_with_args` returns the *whole* launch JSON
    // envelope (`{"results": {...}, "total": 1, "meta": {...}}`); callers
    // here want just the `results` object, matching this helper's
    // pre-iter-146 return shape so the test bodies below didn't need to
    // change their `launch_results["profile_path"]`-style indexing.
    let results = envelope
        .get("results")
        .expect("launch envelope has a `results` object")
        .clone();
    (ff, results)
}

/// AC: `pre_fix_repro_daemon_stop_removes_active_profile`
///
/// launch → capture `profile_path` from the launch JSON → `daemon stop` →
/// assert the profile directory no longer exists on disk.
///
/// Pre-fix (no cleanup wired up): the directory survives `daemon stop`
/// forever — this is the leak iter-96 Theme A closes.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn pre_fix_repro_daemon_stop_removes_active_profile() {
    if !live_tests_enabled() {
        return;
    }

    let (ff, launch_results) = launch_headless();
    // iter-146 Theme A: `ff` stays in scope for the rest of the test as a
    // Drop-based safety net — see `launch_headless`'s doc comment. On the
    // happy path below, `daemon stop` already kills Firefox and this
    // guard's `Drop` is a harmless no-op (the PID is already dead).
    let port = ff.port();

    let profile_path = launch_results["profile_path"]
        .as_str()
        .expect(
            "pre_fix_repro_daemon_stop_removes_active_profile: \
             launch JSON must expose results.profile_path",
        )
        .to_owned();

    assert!(
        std::path::Path::new(&profile_path).exists(),
        "pre_fix_repro_daemon_stop_removes_active_profile: profile dir {profile_path} \
         should exist right after launch"
    );

    let stop_out = Command::new(ff_rdp_bin())
        .args([
            "--host",
            "127.0.0.1",
            "--port",
            &port.to_string(),
            "daemon",
            "stop",
        ])
        .output()
        .expect("pre_fix_repro_daemon_stop_removes_active_profile: daemon stop spawn failed");

    assert!(
        stop_out.status.success(),
        "pre_fix_repro_daemon_stop_removes_active_profile: daemon stop returned non-zero — \
         {}",
        crate::common::output_note(&stop_out)
    );

    let removed = wait_path_gone(&profile_path, Duration::from_secs(5));
    assert!(
        removed,
        "pre_fix_repro_daemon_stop_removes_active_profile: FAIL — profile dir {profile_path} \
         still exists after daemon stop (iter-96 Theme A regression)"
    );

    eprintln!(
        "pre_fix_repro_daemon_stop_removes_active_profile: PASS — \
         {profile_path} removed after daemon stop"
    );
}

/// AC: `live_daemon_stop_profile_path_matches_launch_json`
///
/// `launch`'s `results.profile_path` must equal `daemon stop`'s
/// `results.profile_removed_path`, and `profile_removed` must be `true`.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_daemon_stop_profile_path_matches_launch_json() {
    if !live_tests_enabled() {
        return;
    }

    let (ff, launch_results) = launch_headless();
    // iter-146 Theme A: safety net — see `launch_headless`'s doc comment.
    let port = ff.port();

    let launch_profile_path = launch_results["profile_path"]
        .as_str()
        .expect(
            "live_daemon_stop_profile_path_matches_launch_json: \
             launch JSON must expose results.profile_path",
        )
        .to_owned();

    let stop_out = Command::new(ff_rdp_bin())
        .args([
            "--host",
            "127.0.0.1",
            "--port",
            &port.to_string(),
            "daemon",
            "stop",
        ])
        .output()
        .expect("live_daemon_stop_profile_path_matches_launch_json: daemon stop spawn failed");

    assert!(
        stop_out.status.success(),
        "live_daemon_stop_profile_path_matches_launch_json: daemon stop returned non-zero — \
         {}",
        crate::common::output_note(&stop_out)
    );

    let stop_json: serde_json::Value = serde_json::from_slice(&stop_out.stdout)
        .expect("live_daemon_stop_profile_path_matches_launch_json: stdout is not valid JSON");

    assert_eq!(
        stop_json["results"]["profile_removed"].as_bool(),
        Some(true),
        "live_daemon_stop_profile_path_matches_launch_json: profile_removed must be true — \
         got {}",
        stop_json["results"]
    );

    let stop_profile_removed_path = stop_json["results"]["profile_removed_path"]
        .as_str()
        .expect(
            "live_daemon_stop_profile_path_matches_launch_json: \
             profile_removed_path must be a string",
        )
        .to_owned();

    assert_eq!(
        launch_profile_path, stop_profile_removed_path,
        "live_daemon_stop_profile_path_matches_launch_json: launch profile_path must equal \
         daemon stop's profile_removed_path"
    );

    eprintln!("live_daemon_stop_profile_path_matches_launch_json: PASS — {launch_profile_path}");
}
