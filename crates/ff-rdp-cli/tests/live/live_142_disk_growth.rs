//! Live tests for iter-142 Theme B: disk growth (temp profiles, throttle
//! state files).
//!
//! Dogfooding session 63 observed 62 temp profiles / 2.7 GB accumulate in a
//! single day, plus 5 `daemon.*.throttle.json` files for dead daemon pids.
//! Both existed because the safety nets that should reclaim them either
//! never fired for a same-day workload (`prune_orphan_profiles`'s default
//! 7-day age gate never lets a dead-owner profile go before a week has
//! passed) or never covered the file at all (`gc_stale_spawn_locks_in`
//! deliberately skips `*.throttle.json`, see `registry.rs`). iter-142 fixes
//! both: a dead-owner profile is now reclaimed immediately regardless of
//! age, and a dedicated GC sweep (wired into every `launch`) collects stale
//! throttle-state files.
//!
//! Run with:
//!   FF_RDP_LIVE_TESTS=1 cargo test-live -p ff-rdp-cli \
//!       --test live live_142_disk_growth -- --nocapture

use std::process::Command;
use std::time::Duration;

use crate::common::{ff_rdp_bin, kill_pid, live_tests_enabled};

/// Attempt to bind `:0` to discover a free port.
fn free_port() -> Option<u16> {
    let l = std::net::TcpListener::bind("127.0.0.1:0").ok()?;
    Some(l.local_addr().ok()?.port())
}

/// Launch Firefox headless via the CLI on a freshly discovered port and
/// return `(port, results)` — the `results` object of the launch envelope.
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

/// Spawn+reap a trivial child process, returning its now-dead PID.
fn spawn_and_reap_child_pid() -> u32 {
    #[cfg(unix)]
    let mut child = Command::new("true").spawn().expect("spawn `true`");
    #[cfg(windows)]
    let mut child = Command::new("cmd")
        .args(["/C", "exit", "0"])
        .spawn()
        .expect("spawn cmd exit");
    let pid = child.id();
    child.wait().expect("child exits");
    std::thread::sleep(Duration::from_millis(50));
    pid
}

/// AC: `live_142_profile_growth_bounded`
///
/// Policy under test: a temp profile whose owner PID is confirmed dead is
/// reclaimed by the very next `launch`'s opportunistic sweep — immediately,
/// not after any age threshold (`prune_orphan_profiles`, iter-142 Theme B).
///
/// 1. Launch Firefox headless (creates a managed temp profile + owner-PID
///    marker).
/// 2. Force-kill it directly (SIGKILL), bypassing `daemon stop` — the normal
///    cleanup path never runs, so the profile dir is only reclaimable via
///    the orphan sweep.
/// 3. Launch a second Firefox headless immediately (same process, no delay,
///    no artificial aging of the first profile's mtime).
/// 4. Assert instance A's profile directory is gone — proves growth is
///    bounded by "next launch", not by waiting out an age threshold.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_142_profile_growth_bounded() {
    if !live_tests_enabled() {
        return;
    }

    let Some((_port_a, results_a)) = launch_headless() else {
        eprintln!("live_142_profile_growth_bounded: Firefox not available — skipping");
        return;
    };
    let pid_a =
        u32::try_from(results_a["pid"].as_u64().expect("results.pid")).expect("pid fits u32");
    let profile_a = results_a["profile_path"]
        .as_str()
        .expect("results.profile_path")
        .to_owned();

    assert!(
        std::path::Path::new(&profile_a).exists(),
        "live_142_profile_growth_bounded: profile {profile_a} must exist right after launch"
    );

    // Force-kill instance A directly — bypasses `daemon stop`'s own cleanup,
    // so the profile dir is orphaned exactly like a crash or `kill -9` would
    // leave it.
    kill_pid(pid_a);
    std::thread::sleep(Duration::from_millis(500));

    eprintln!(
        "live_142_profile_growth_bounded: instance A (pid {pid_a}) force-killed, \
         profile {profile_a} now orphaned"
    );

    // Immediately launch a second instance — no artificial delay. If growth
    // were still bounded only by the old 7-day age gate, instance A's
    // fresh (seconds-old) profile would still be sitting there.
    let Some((port_b, results_b)) = launch_headless() else {
        eprintln!("live_142_profile_growth_bounded: second launch failed — skipping");
        return;
    };
    let pid_b =
        u32::try_from(results_b["pid"].as_u64().expect("results.pid")).expect("pid fits u32");

    assert!(
        !std::path::Path::new(&profile_a).exists(),
        "live_142_profile_growth_bounded: FAIL — orphaned profile {profile_a} \
         must be reclaimed by the very next launch's opportunistic sweep, \
         not left to accumulate for days"
    );

    eprintln!(
        "live_142_profile_growth_bounded: PASS — orphaned profile {profile_a} \
         reclaimed by the next launch (instance B on port {port_b}, pid {pid_b})"
    );

    // Clean up instance B.
    let _ = Command::new(ff_rdp_bin())
        .args([
            "--port",
            &port_b.to_string(),
            "--timeout",
            "15000",
            "daemon",
            "stop",
        ])
        .output();
    kill_pid(pid_b);
}

/// AC: `live_142_throttle_json_gc`
///
/// A `daemon.<port>.throttle.json` file whose recorded `daemon_pid` is dead
/// is collected by the next `launch`'s sweep (`gc_stale_throttle_states`,
/// wired into `commands::launch::run`); one whose `daemon_pid` is alive
/// survives. Runs against an isolated `FF_RDP_HOME` so it never touches the
/// real `~/.ff-rdp/` directory (same isolation pattern as
/// `live_123_daemon_autostart_and_registry.rs`).
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_142_throttle_json_gc() {
    if !live_tests_enabled() {
        return;
    }

    let home = tempfile::tempdir().expect("tempdir for FF_RDP_HOME");
    let registry_dir = home.path().join(".ff-rdp");
    std::fs::create_dir_all(&registry_dir).expect("create registry dir");

    let dead_pid = spawn_and_reap_child_pid();
    let live_pid = std::process::id();

    let dead_json = serde_json::json!({
        "profile": "slow-3g",
        "set_at": "2026-08-09T00:00:00Z",
        "daemon_pid": dead_pid,
    });
    let live_json = serde_json::json!({
        "profile": "slow-3g",
        "set_at": "2026-08-09T00:00:00Z",
        "daemon_pid": live_pid,
    });
    let dead_path = registry_dir.join("daemon.19100.throttle.json");
    let live_path = registry_dir.join("daemon.19101.throttle.json");
    std::fs::write(
        &dead_path,
        serde_json::to_string_pretty(&dead_json).unwrap(),
    )
    .expect("write dead throttle state");
    std::fs::write(
        &live_path,
        serde_json::to_string_pretty(&live_json).unwrap(),
    )
    .expect("write live throttle state");

    // Any `launch` sweeps the whole registry directory regardless of which
    // port it targets — trigger it with a throwaway instance under the same
    // isolated FF_RDP_HOME.
    let Some(port) = free_port() else {
        eprintln!("live_142_throttle_json_gc: no free port — skipping");
        return;
    };
    let out = Command::new(ff_rdp_bin())
        .env("FF_RDP_HOME", home.path())
        .args(["launch", "--headless", "--debug-port", &port.to_string()])
        .output()
        .expect("launch spawn failed");
    if !out.status.success() {
        eprintln!(
            "live_142_throttle_json_gc: Firefox not available — skipping (stderr={})",
            String::from_utf8_lossy(&out.stderr)
        );
        return;
    }
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).expect("launch JSON parse");
    let pid =
        u32::try_from(json["results"]["pid"].as_u64().expect("results.pid")).expect("pid fits u32");

    assert!(
        !dead_path.exists(),
        "live_142_throttle_json_gc: FAIL — dead-pid throttle state must be \
         collected by the next launch's sweep"
    );
    assert!(
        live_path.exists(),
        "live_142_throttle_json_gc: live-pid throttle state must survive the sweep"
    );

    eprintln!("live_142_throttle_json_gc: PASS — dead entry collected, live entry survived");

    // Clean up the throwaway instance.
    let _ = Command::new(ff_rdp_bin())
        .env("FF_RDP_HOME", home.path())
        .args([
            "--port",
            &port.to_string(),
            "--timeout",
            "15000",
            "daemon",
            "stop",
        ])
        .output();
    kill_pid(pid);
}
