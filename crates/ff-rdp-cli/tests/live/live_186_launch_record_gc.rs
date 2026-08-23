//! Live tests for iter-186: `~/.ff-rdp/launch-record.<port>.json` leaked one
//! file per launch.
//!
//! Measured on the dev machine 2026-08-23: 4040 records / 17 MB, spanning ten
//! days of ordinary test and dogfood traffic. iter-142 split what had been a
//! single global `launch-record.json` into one file per port (fixing a real
//! clobber between concurrent agents) and, in doing so, turned a file that
//! could only be overwritten into a family of files that could only grow.
//!
//! Two reclamation paths existed and neither covered it:
//! `daemon_record::remove_in` runs only on a *clean* `daemon stop` — the live
//! suite kills daemons routinely — and `daemon_record::read_in` removes a
//! dead-pid record only as a side effect of reading *that same port* again.
//! Ports come from an ephemeral `bind(:0)`, so that port essentially never
//! recurs: the lazy reaper was not slow, it never fired at all.
//!
//! The fix mirrors what iter-142 itself did for `daemon.<port>.throttle.json`
//! (see `live_142_disk_growth.rs`): a directory sweep
//! (`daemon_record::gc_stale_launch_records`) wired into every `launch`.
//!
//! Run with:
//!   FF_RDP_LIVE_TESTS=1 cargo test-live -p ff-rdp-cli \
//!       --test live live_186 -- --nocapture

use std::path::Path;
use std::process::Command;
use std::time::Duration;

use crate::common::{
    FirefoxGuard, ff_rdp_bin, ff_rdp_launch_command, kill_pid, live_tests_enabled,
};

/// Attempt to bind `:0` to discover a free port.
fn free_port() -> Option<u16> {
    let l = std::net::TcpListener::bind("127.0.0.1:0").ok()?;
    Some(l.local_addr().ok()?.port())
}

/// Spawn a trivial child, wait for it, and return its now-dead pid — a pid
/// that provably existed and provably does not any more (same helper shape as
/// `live_142_disk_growth.rs`).
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

/// Plant a `launch-record.<port>.json` for `pid` in `dir`.
fn plant_record(dir: &Path, port: u16, pid: u32) -> std::path::PathBuf {
    let json = serde_json::json!({
        "pid": pid,
        "port": port,
        "headless": true,
        "launched_at": "2026-08-12T00:00:00Z",
        "profile_dir": "/tmp/ff-rdp-planted",
    });
    let path = dir.join(format!("launch-record.{port}.json"));
    std::fs::write(&path, serde_json::to_string_pretty(&json).unwrap())
        .expect("write planted launch record");
    path
}

/// Count `launch-record.<PORT>.json` files in `dir`.
fn record_count(dir: &Path) -> usize {
    std::fs::read_dir(dir)
        .expect("read_dir")
        .flatten()
        .filter(|e| {
            e.file_name()
                .into_string()
                .ok()
                .and_then(|n| {
                    n.strip_prefix("launch-record.")
                        .and_then(|r| r.strip_suffix(".json"))
                        .and_then(|p| p.parse::<u16>().ok())
                })
                .is_some()
        })
        .count()
}

/// Stop the throwaway instance started on `port` under `home`, then make sure
/// the process is gone.
fn stop_instance(home: &Path, port: u16, pid: u32) {
    let _ = Command::new(ff_rdp_bin())
        .env("FF_RDP_HOME", home)
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

/// Launch a throwaway headless Firefox under an isolated `FF_RDP_HOME` and
/// return `(guard, port, pid)`. Panics on any launch failure (iter-158 Theme
/// D). The guard is built the instant the pid is known so every assertion
/// downstream is guard-protected (iter-146/151).
fn launch_under_home(home: &Path, test_name: &str) -> Option<(FirefoxGuard, u16, u32)> {
    let Some(port) = free_port() else {
        eprintln!("{test_name}: no free port — skipping");
        return None;
    };
    let out = ff_rdp_launch_command()
        .env("FF_RDP_HOME", home)
        .env(crate::common::SPAWNING_TEST_ENV, test_name)
        .args(["launch", "--headless", "--debug-port", &port.to_string()])
        .output()
        .expect("launch spawn failed");
    assert!(
        out.status.success(),
        "{test_name}: `ff-rdp launch --headless --debug-port {port}` exited {}\n  stdout: {}\n  stderr: {}",
        out.status,
        String::from_utf8_lossy(&out.stdout).trim(),
        String::from_utf8_lossy(&out.stderr).trim(),
    );
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).expect("launch JSON parse");
    let pid =
        u32::try_from(json["results"]["pid"].as_u64().expect("results.pid")).expect("pid fits u32");
    Some((FirefoxGuard::new(pid), port, pid))
}

/// AC: `live_186_launch_record_gc`
///
/// AC 2 of the plan, end to end: a launch-record whose pid is dead is
/// collected by the next `launch`'s sweep, and **a live daemon's record
/// survives a sweep that runs while it is up** — including the record the
/// launch under test writes for itself.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_186_launch_record_gc_collects_dead_spares_live() {
    if !live_tests_enabled() {
        return;
    }

    let home = tempfile::tempdir().expect("tempdir for FF_RDP_HOME");
    let record_dir = home.path().join(".ff-rdp");
    std::fs::create_dir_all(&record_dir).expect("create record dir");

    let dead_pid = spawn_and_reap_child_pid();
    let dead_path = plant_record(&record_dir, 19100, dead_pid);
    // This test process is unambiguously alive for the whole run — it stands
    // in for a concurrent daemon whose record must not be touched.
    let live_path = plant_record(&record_dir, 19101, std::process::id());

    let Some((_guard, port, pid)) =
        launch_under_home(home.path(), "live_186_launch_record_gc_collects_dead_spares_live")
    else {
        return;
    };

    assert!(
        !dead_path.exists(),
        "live_186: FAIL — a launch record whose pid is dead must be collected \
         by the next launch's sweep (this is the 4040-file leak)"
    );
    assert!(
        live_path.exists(),
        "live_186: FAIL — a live process's launch record must survive a sweep \
         that runs while it is up"
    );
    assert!(
        record_dir
            .join(format!("launch-record.{port}.json"))
            .exists(),
        "live_186: FAIL — the launching instance's own record must exist after \
         its launch; the sweep runs before the record is written and must \
         never race it away"
    );

    eprintln!("live_186: PASS — dead record collected, live records survived");

    stop_instance(home.path(), port, pid);
}

/// AC: `live_186_launch_record_growth_bounded`
///
/// AC 1 of the plan: `ls ~/.ff-rdp/launch-record.*.json | wc -l` does not grow
/// across repeated launches. Three real launches on three ephemeral ports —
/// every one a distinct port, which is exactly the condition under which
/// `read_in`'s lazy reaping collects nothing — must leave at most one record
/// per still-running instance, not one per launch.
///
/// Each instance is stopped before the next launch, so the steady state is a
/// single record: the one the most recent launch wrote for itself.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_186_launch_record_growth_bounded() {
    if !live_tests_enabled() {
        return;
    }

    let home = tempfile::tempdir().expect("tempdir for FF_RDP_HOME");
    let record_dir = home.path().join(".ff-rdp");
    std::fs::create_dir_all(&record_dir).expect("create record dir");

    const LAUNCHES: usize = 3;
    for i in 0..LAUNCHES {
        let Some((guard, port, pid)) =
            launch_under_home(home.path(), "live_186_launch_record_growth_bounded")
        else {
            return;
        };
        // Kill without a clean `daemon stop` on all but the last pass: the
        // killed case is precisely the one `remove_in` never covers, so the
        // record is left behind for the *next* launch's sweep to reclaim.
        stop_instance(home.path(), port, pid);
        drop(guard);

        let count = record_count(&record_dir);
        assert!(
            count <= 2,
            "live_186: FAIL — after {} launch(es) the record directory holds {count} \
             launch-record files; unbounded growth is the defect under test",
            i + 1
        );
    }

    // One more launch to sweep whatever the final stop left behind, then
    // assert the steady state directly.
    let Some((_guard, port, pid)) =
        launch_under_home(home.path(), "live_186_launch_record_growth_bounded")
    else {
        return;
    };
    let count = record_count(&record_dir);
    assert!(
        count <= 2,
        "live_186: FAIL — after {} launches the record directory holds {count} files; \
         it must stay bounded, not grow with the launch count",
        LAUNCHES + 1
    );
    eprintln!(
        "live_186: PASS — {} launches left {count} launch-record file(s)",
        LAUNCHES + 1
    );

    stop_instance(home.path(), port, pid);
}
