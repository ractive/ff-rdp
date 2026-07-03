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
//!       --test live_96_profile_cleanup -- --nocapture

#[path = "common/mod.rs"]
mod common;

use std::process::Command;
use std::time::Duration;

use common::ff_rdp_bin;

fn live_tests_enabled() -> bool {
    std::env::var("FF_RDP_LIVE_TESTS").as_deref() == Ok("1")
}

/// Attempt to bind `:0` to discover a free port.
fn free_port() -> Option<u16> {
    let l = std::net::TcpListener::bind("127.0.0.1:0").ok()?;
    Some(l.local_addr().ok()?.port())
}

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

/// Launch Firefox headless via the CLI on a freshly discovered port and
/// return `(port, results)` where `results` is the `results` object of the
/// launch JSON envelope. Returns `None` if the launch fails.
fn launch_headless() -> Option<(u16, serde_json::Value)> {
    let port = free_port()?;
    let out = Command::new(ff_rdp_bin())
        .args(["launch", "--headless", "--debug-port", &port.to_string()])
        .output()
        .ok()?;
    if !out.status.success() {
        eprintln!(
            "launch_headless: launch failed — stderr={}",
            String::from_utf8_lossy(&out.stderr)
        );
        return None;
    }
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).ok()?;
    let results = json.get("results")?.clone();
    Some((port, results))
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

    let Some((port, launch_results)) = launch_headless() else {
        eprintln!(
            "pre_fix_repro_daemon_stop_removes_active_profile: Firefox not available — skipping"
        );
        return;
    };

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

    let Some((port, launch_results)) = launch_headless() else {
        eprintln!(
            "live_daemon_stop_profile_path_matches_launch_json: Firefox not available — skipping"
        );
        return;
    };

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

/// AC: `live_profiles_prune_removes_all_when_no_firefox_running`
///
/// Seeds orphan `ff-rdp-profile-*` directories directly under the real
/// profile root (discovered via `ff-rdp profiles list`'s `results.path`),
/// then runs `ff-rdp profiles prune --all` and asserts zero
/// `ff-rdp-profile-*` entries remain under the root afterwards.
///
/// Requires no *running* ff-rdp-managed Firefox instance — `--all` removes
/// every managed directory regardless of age, which would rip the profile
/// out from under a live session. This test never kills anything; it just
/// skips (rather than force-stopping a session) when `ff-rdp daemon status`
/// reports one is active. This is a best-effort check: a Firefox instance
/// launched via `ff-rdp launch` that never triggered daemon auto-start
/// (no other command has run against it yet) wouldn't be visible to
/// `daemon status` — see the iter-96 Theme C plan for the acknowledged gap.
#[test]
#[ignore = "touches the real per-user profile root — set FF_RDP_LIVE_TESTS=1"]
fn live_profiles_prune_removes_all_when_no_firefox_running() {
    if !live_tests_enabled() {
        return;
    }

    let status_out = Command::new(ff_rdp_bin())
        .args(["daemon", "status"])
        .output();
    if let Ok(out) = status_out
        && out.status.success()
        && let Ok(json) = serde_json::from_slice::<serde_json::Value>(&out.stdout)
        && json["results"]["running"].as_bool() == Some(true)
    {
        eprintln!(
            "live_profiles_prune_removes_all_when_no_firefox_running: \
             a daemon is running — skipping to avoid pruning a live session's profile"
        );
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

    let remaining = std::fs::read_dir(&root).map_or(0, |entries| {
        entries
            .flatten()
            .filter(|e| {
                e.file_name()
                    .to_str()
                    .is_some_and(|n| n.starts_with("ff-rdp-profile-"))
            })
            .count()
    });
    assert_eq!(
        remaining, 0,
        "live_profiles_prune_removes_all_when_no_firefox_running: expected zero \
         ff-rdp-profile-* dirs under {root} after prune --all, found {remaining}"
    );

    eprintln!("live_profiles_prune_removes_all_when_no_firefox_running: PASS — root={root}");
}
