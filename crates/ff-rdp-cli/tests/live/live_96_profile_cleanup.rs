//! Live tests for iter-96 Theme A (`daemon stop` profile cleanup) and
//! Theme C (`ff-rdp profiles prune` manual cleanup).
//!
//! Every `ff-rdp launch` (without `--profile`) creates a fresh profile dir
//! under `secure_profile_root()` and never removed it — see
//! `kb/iterations/iteration-96-profile-leak-cleanup.md`. Theme A's tests
//! assert the fix: `daemon stop` removes the directory once the
//! SIGTERM→SIGKILL→killpg escalation ladder confirms Firefox is gone and the
//! port is free. Theme C's test asserts the manual escape hatch: `ff-rdp
//! profiles prune --all` removes every managed orphan directory on demand.
//!
//! Run with:
//!   FF_RDP_LIVE_TESTS=1 cargo test-live -p ff-rdp-cli \
//!       --test live live_96_profile_cleanup -- --nocapture

use std::process::Command;
use std::time::Duration;

use crate::common::ff_rdp_bin;
use crate::common::live_tests_enabled;
use crate::common::{LiveFirefox, pid_alive};

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
fn launch_headless() -> Option<(LiveFirefox, serde_json::Value)> {
    let (ff, envelope) = LiveFirefox::headless_on_random_port_with_args(&[])?;
    // `headless_on_random_port_with_args` returns the *whole* launch JSON
    // envelope (`{"results": {...}, "total": 1, "meta": {...}}`); callers
    // here want just the `results` object, matching this helper's
    // pre-iter-146 return shape so the test bodies below didn't need to
    // change their `launch_results["profile_path"]`-style indexing.
    let results = envelope.get("results")?.clone();
    Some((ff, results))
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

    let Some((ff, launch_results)) = launch_headless() else {
        eprintln!(
            "pre_fix_repro_daemon_stop_removes_active_profile: Firefox not available — skipping"
        );
        return;
    };
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
         stderr={}",
        String::from_utf8_lossy(&stop_out.stderr)
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

    let Some((ff, launch_results)) = launch_headless() else {
        eprintln!(
            "live_daemon_stop_profile_path_matches_launch_json: Firefox not available — skipping"
        );
        return;
    };
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
         stderr={}",
        String::from_utf8_lossy(&stop_out.stderr)
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

/// Path to the owner-PID marker written inside every ff-rdp-managed profile
/// dir (mirrors the product's private `util::profile_dir::OWNER_PID_MARKER`,
/// unreachable from an integration-test binary — see
/// `write_owner_pid_marker`/`read_owner_pid_marker` there).
const OWNER_PID_MARKER: &str = ".ff-rdp-owner-pid";

/// Path to the owner-test marker (iter-151 Theme A) — mirrors the product's
/// private `util::profile_dir::OWNER_TEST_MARKER`, same duplication
/// rationale as [`OWNER_PID_MARKER`] above.
const OWNER_TEST_MARKER: &str = ".ff-rdp-owner-test";

/// Scan `root` for `ff-rdp-profile-*` directories whose owner-PID marker
/// names a still-alive process, returning `(dir, pid, spawning_test)` pairs.
/// `spawning_test` is `None` for a profile with no owner-test marker (a
/// normal interactive `ff-rdp launch`, or a pre-iter-151 profile).
///
/// iter-146 Theme B: unlike a `daemon status` check (the precondition this
/// replaces), this also catches a Firefox instance launched via `ff-rdp
/// launch` that never triggered daemon autostart — the exact gap the old
/// precondition's own doc comment acknowledged and that made
/// `live_profiles_prune_removes_all_when_no_firefox_running` order-dependent:
/// it passed in isolation but failed late in a full sequential suite run
/// with an opaque `left: 1 / right: 0`, because `prune --all` **correctly**
/// refused to delete a profile some earlier test's Firefox still owned.
///
/// iter-151 Theme A: also reads [`OWNER_TEST_MARKER`] so the precondition
/// message below names the culprit test directly instead of leaving the
/// reader to bisect the full live suite.
fn live_owned_profile_dirs(root: &str) -> Vec<(std::path::PathBuf, u32, Option<String>)> {
    let Ok(entries) = std::fs::read_dir(root) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter(|e| {
            e.file_name()
                .to_str()
                .is_some_and(|n| n.starts_with("ff-rdp-profile-"))
        })
        .filter_map(|e| {
            let marker = e.path().join(OWNER_PID_MARKER);
            let pid: u32 = std::fs::read_to_string(&marker).ok()?.trim().parse().ok()?;
            if !pid_alive(pid) {
                return None;
            }
            let test_name = std::fs::read_to_string(e.path().join(OWNER_TEST_MARKER))
                .ok()
                .map(|s| s.trim().to_owned())
                .filter(|s| !s.is_empty());
            Some((e.path(), pid, test_name))
        })
        .collect()
}

/// AC: `live_profiles_prune_removes_all_when_no_firefox_running`
///
/// Seeds orphan `ff-rdp-profile-*` directories directly under the real
/// profile root (discovered via `ff-rdp profiles list`'s `results.path`),
/// then runs `ff-rdp profiles prune --all` and asserts zero
/// `ff-rdp-profile-*` entries remain under the root afterwards.
///
/// Requires no *running* ff-rdp-managed Firefox instance — `--all` removes
/// every managed directory regardless of age, which would rip the profile
/// out from under a live session. iter-146 Theme B: the precondition is now
/// an explicit, named-PID assertion (`live_owned_profile_dirs`) rather than
/// a `daemon status` skip check — the latter went silently blind to a
/// directly-launched Firefox that never triggered daemon autostart, which
/// is exactly the gap iter-146 Theme A's own leak exercised. This test still
/// never kills anything itself; a live owner means the test environment
/// isn't clean, which is worth failing loudly on rather than skipping quietly
/// or reporting a bare `left: 1 / right: 0` at the very end.
#[test]
#[ignore = "touches the real per-user profile root — set FF_RDP_LIVE_TESTS=1"]
fn live_profiles_prune_removes_all_when_no_firefox_running() {
    if !live_tests_enabled() {
        return;
    }

    let list_out = Command::new(ff_rdp_bin())
        .args(["profiles", "list"])
        .output()
        .expect(
            "live_profiles_prune_removes_all_when_no_firefox_running: profiles list spawn failed",
        );
    assert!(
        list_out.status.success(),
        "live_profiles_prune_removes_all_when_no_firefox_running: profiles list must succeed — \
         stderr={}",
        String::from_utf8_lossy(&list_out.stderr)
    );
    let list_json: serde_json::Value = serde_json::from_slice(&list_out.stdout).expect(
        "live_profiles_prune_removes_all_when_no_firefox_running: profiles list stdout is not valid JSON",
    );
    let root = list_json["results"]["path"]
        .as_str()
        .expect(
            "live_profiles_prune_removes_all_when_no_firefox_running: \
             profiles list JSON must expose results.path",
        )
        .to_owned();

    // iter-146 Theme B: explicit, named precondition — see
    // `live_owned_profile_dirs`'s doc comment for why this replaces the old
    // `daemon status`-only skip check.
    let live_owners = live_owned_profile_dirs(&root);
    assert!(
        live_owners.is_empty(),
        "live_profiles_prune_removes_all_when_no_firefox_running: precondition violated — \
         {} ff-rdp-managed profile dir(s) under {root} are still owned by a live process, so \
         `prune --all` would rip a profile out from under it: {}. Rerun once these have \
         exited (or in an isolated environment).",
        live_owners.len(),
        live_owners
            .iter()
            .map(|(dir, pid, test_name)| {
                let who = test_name.as_deref().unwrap_or("unknown test");
                format!("{} (pid {pid}, spawned by {who})", dir.display())
            })
            .collect::<Vec<_>>()
            .join(", ")
    );

    // Seed a handful of orphan managed profile dirs directly on disk.
    let seeded: Vec<std::path::PathBuf> = (0..3)
        .map(|i| {
            let dir = std::path::Path::new(&root).join(format!("ff-rdp-profile-{i:016}"));
            std::fs::create_dir_all(&dir).expect(
                "live_profiles_prune_removes_all_when_no_firefox_running: seed orphan profile dir",
            );
            dir
        })
        .collect();

    let prune_out = Command::new(ff_rdp_bin())
        .args(["profiles", "prune", "--all"])
        .output()
        .expect(
            "live_profiles_prune_removes_all_when_no_firefox_running: profiles prune spawn failed",
        );
    assert!(
        prune_out.status.success(),
        "live_profiles_prune_removes_all_when_no_firefox_running: profiles prune --all must \
         succeed — stderr={}",
        String::from_utf8_lossy(&prune_out.stderr)
    );

    for dir in &seeded {
        assert!(
            !dir.exists(),
            "live_profiles_prune_removes_all_when_no_firefox_running: {} should have been removed",
            dir.display()
        );
    }

    let remaining: Vec<std::path::PathBuf> = std::fs::read_dir(&root).map_or_else(
        |_| Vec::new(),
        |entries| {
            entries
                .flatten()
                .filter(|e| {
                    e.file_name()
                        .to_str()
                        .is_some_and(|n| n.starts_with("ff-rdp-profile-"))
                })
                .map(|e| e.path())
                .collect()
        },
    );
    // iter-146 Theme B: name what's left (and whether it has a live owner)
    // instead of a bare count — this precondition-checked test now failing
    // here means `prune --all` itself is broken, not stale suite state.
    assert!(
        remaining.is_empty(),
        "live_profiles_prune_removes_all_when_no_firefox_running: expected zero \
         ff-rdp-profile-* dirs under {root} after prune --all, found {}: {:?} (live owners \
         among them: {:?})",
        remaining.len(),
        remaining,
        live_owned_profile_dirs(&root)
    );

    eprintln!("live_profiles_prune_removes_all_when_no_firefox_running: PASS — root={root}");
}
