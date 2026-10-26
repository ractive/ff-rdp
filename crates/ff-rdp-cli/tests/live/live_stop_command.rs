//! `ff-rdp stop` against real Firefox: it stops what ff-rdp launched (process
//! gone, profile gone), `--all` stops every such instance under the home, and
//! a Firefox started by hand on the same port is refused and keeps running.
//!
//! Every launch runs in this test thread's isolated `FF_RDP_HOME`
//! (`ff_rdp_launch_command`), and `stop` runs in the same home, so `--all`
//! can only ever see this test's own browsers.

use std::path::PathBuf;
use std::time::Duration;

use crate::common::{
    FirefoxGuard, RawFirefox, ff_rdp_launch_command, guard_launched_firefox, live_tests_enabled,
    pid_alive, wait_for_pid_exit,
};

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .expect("bind")
        .local_addr()
        .expect("addr")
        .port()
}

/// `ff-rdp launch --headless` on a fresh port; returns the guard, port and
/// managed profile path.
fn launch() -> (FirefoxGuard, u16, PathBuf) {
    let port = free_port();
    let out = ff_rdp_launch_command()
        .args(["launch", "--headless", "--debug-port", &port.to_string()])
        .output()
        .expect("run ff-rdp launch");
    let guard = guard_launched_firefox(&out);
    assert!(
        out.status.success(),
        "launch failed: {}",
        String::from_utf8_lossy(&out.stdout)
    );
    let guard = guard.expect("launch must report results.pid");
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).expect("launch JSON");
    let profile = PathBuf::from(json["results"]["profile"].as_str().expect("profile"));
    assert!(profile.is_dir(), "managed profile must exist after launch");
    (guard, port, profile)
}

fn stop(args: &[&str]) -> (bool, serde_json::Value, String) {
    let out = ff_rdp_launch_command()
        .arg("stop")
        .args(args)
        .output()
        .expect("run ff-rdp stop");
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let json = serde_json::from_str(&stdout).unwrap_or(serde_json::Value::Null);
    (out.status.success(), json, stdout)
}

/// AC: `ff-rdp stop` stops the Firefox it launched — the process exits and
/// its temporary profile is removed.
#[test]
#[ignore = "requires a live Firefox instance (FF_RDP_LIVE_TESTS=1)"]
fn live_stop_stops_launched_instance_and_removes_profile() {
    if !live_tests_enabled() {
        eprintln!("live_stop_stops_launched_instance_and_removes_profile: skipped");
        return;
    }
    let (guard, port, profile) = launch();
    let pid = guard.pid();

    let (ok, json, raw) = stop(&["--port", &port.to_string()]);
    assert!(ok, "stop must succeed: {raw}");
    assert_eq!(json["results"]["stopped"], true, "{raw}");
    assert_eq!(json["results"]["pid"], pid, "{raw}");
    assert_eq!(json["results"]["profile_removed"], true, "{raw}");
    assert!(
        wait_for_pid_exit(pid, Duration::from_secs(5)).is_some(),
        "Firefox pid {pid} must be gone after stop"
    );
    assert!(!profile.exists(), "{} must be removed", profile.display());
    guard.disarm();

    // Nothing left: a second stop reports stopped: false.
    let (ok, json, raw) = stop(&["--port", &port.to_string()]);
    assert!(ok, "{raw}");
    assert_eq!(json["results"]["stopped"], false, "{raw}");
}

/// AC: `--dry-run` reports the instance and signals nothing; `--all` then
/// stops both launched instances and removes both profiles.
#[test]
#[ignore = "requires a live Firefox instance (FF_RDP_LIVE_TESTS=1)"]
fn live_stop_all_stops_every_launched_instance() {
    if !live_tests_enabled() {
        eprintln!("live_stop_all_stops_every_launched_instance: skipped");
        return;
    }
    let (first, first_port, first_profile) = launch();
    let (second, _, second_profile) = launch();

    let (ok, json, raw) = stop(&["--dry-run", "--port", &first_port.to_string()]);
    assert!(ok, "{raw}");
    assert_eq!(json["results"]["would_stop"], true, "{raw}");
    assert_eq!(json["results"]["pid"], first.pid(), "{raw}");

    let (ok, json, raw) = stop(&["--all", "--dry-run"]);
    assert!(ok, "{raw}");
    assert_eq!(json["total"], 2, "{raw}");
    std::thread::sleep(Duration::from_millis(300));
    assert!(
        pid_alive(first.pid()) && pid_alive(second.pid()),
        "dry run must signal nothing"
    );
    assert!(first_profile.is_dir() && second_profile.is_dir());

    let (ok, json, raw) = stop(&["--all"]);
    assert!(ok, "{raw}");
    assert_eq!(json["total"], 2, "{raw}");
    for entry in json["results"].as_array().expect("array") {
        assert_eq!(entry["stopped"], true, "{raw}");
        assert_eq!(entry["profile_removed"], true, "{raw}");
    }
    for guard in [first, second] {
        assert!(
            wait_for_pid_exit(guard.pid(), Duration::from_secs(5)).is_some(),
            "pid {} must be gone",
            guard.pid()
        );
        guard.disarm();
    }
    assert!(!first_profile.exists() && !second_profile.exists());
}

/// AC: a Firefox started by hand with a raw profile on the port is refused —
/// an error naming its PID — and is still running afterwards.
#[test]
#[ignore = "requires a live Firefox instance (FF_RDP_LIVE_TESTS=1)"]
fn live_stop_refuses_a_hand_started_firefox() {
    if !live_tests_enabled() {
        eprintln!("live_stop_refuses_a_hand_started_firefox: skipped");
        return;
    }
    let raw = RawFirefox::headless_on_random_port();
    let port = raw.port().to_string();
    for args in [&["--port", &port][..], &["--dry-run", "--port", &port][..]] {
        let (ok, _, out) = stop(args);
        assert!(
            !ok,
            "stop must refuse a Firefox ff-rdp did not launch: {out}"
        );
        assert!(out.contains(&format!("PID {}", raw.pid())), "{out}");
        assert!(out.contains("did not launch"), "{out}");
    }
    let (ok, json, out) = stop(&["--all"]);
    assert!(ok, "{out}");
    assert_eq!(
        json["total"], 0,
        "--all must not list a hand-started Firefox: {out}"
    );
    std::thread::sleep(Duration::from_millis(500));
    assert!(
        pid_alive(raw.pid()),
        "REGRESSION: ff-rdp stop signalled a Firefox it did not launch"
    );
}
