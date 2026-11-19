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
    eprintln!(
        "DIAG launch pid={} launcher_pid={} port={port}",
        json["results"]["pid"], json["results"]["launcher_pid"]
    );
    let profile = PathBuf::from(json["results"]["profile"].as_str().expect("profile"));
    assert!(profile.is_dir(), "managed profile must exist after launch");
    (guard, port, profile)
}

/// TEMP iter-304 diagnostic: the state of every firefox.exe after `stop`.
fn diag_after_stop(label: &str) {
    #[cfg(windows)]
    for delay in [0u64, 1000, 3000] {
        std::thread::sleep(Duration::from_millis(delay));
        let table = std::process::Command::new("powershell")
            .args([
                "-NoProfile",
                "-Command",
                "Get-CimInstance Win32_Process -Filter \"Name='firefox.exe'\" | \
                 ForEach-Object { \"$($_.ProcessId) ppid=$($_.ParentProcessId) contentproc=$($_.CommandLine -like '*-contentproc*')\" }",
            ])
            .output()
            .map_or_else(|_| String::new(), |o| String::from_utf8_lossy(&o.stdout).into_owned());
        let states: Vec<String> = table
            .lines()
            .filter_map(|l| l.split_whitespace().next()?.parse::<u32>().ok())
            .map(crate::common::pid_diag)
            .collect();
        eprintln!("DIAG {label} +{delay}ms table:\n{table}states: {states:?}");
    }
    #[cfg(not(windows))]
    let _ = label;
}

fn stop(args: &[&str]) -> (bool, serde_json::Value, String) {
    let out = ff_rdp_launch_command()
        .arg("stop")
        .args(args)
        .output()
        .expect("run ff-rdp stop");
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    eprintln!("DIAG stop {args:?} -> {}", stdout.trim());
    diag_after_stop(&format!("{args:?}"));
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
    assert_eq!(json["results"]["profile_would_be_removed"], true, "{raw}");

    let (ok, json, raw) = stop(&["--all", "--dry-run"]);
    assert!(ok, "{raw}");
    assert_eq!(json["total"], 2, "{raw}");
    for entry in json["results"].as_array().expect("array") {
        assert_eq!(entry["profile_would_be_removed"], true, "{raw}");
    }
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
    let browser = browser_pid(&raw);
    for args in [&["--port", &port][..], &["--dry-run", "--port", &port][..]] {
        let (ok, _, out) = stop(args);
        assert!(
            !ok,
            "stop must refuse a Firefox ff-rdp did not launch: {out}"
        );
        assert!(out.contains(&format!("PID {browser}")), "{out}");
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
        pid_alive(raw.pid()) && pid_alive(browser),
        "REGRESSION: ff-rdp stop signalled a Firefox it did not launch"
    );
}

/// The PID holding the hand-started Firefox's debug port — the process
/// `stop` looks up and names.
///
/// On Linux and macOS that is the spawned PID. On Windows the spawned
/// `firefox.exe` is usually Firefox's *launcher process*, which starts the
/// browser as a child and stays alive as its parent; the browser holds the
/// port (iter-299). GitHub's Windows runner runs that launcher, so the refusal
/// there names the child (run 44577054335: listener 3196, not the spawned
/// PID). The listener is read from `netstat` and must be the spawned PID or a
/// process whose parent it is.
fn browser_pid(raw: &RawFirefox) -> u32 {
    #[cfg(windows)]
    {
        let out = std::process::Command::new("netstat")
            .args(["-ano", "-p", "tcp"])
            .output()
            .expect("run netstat");
        // OEM code page, localized state column: a listener is the row whose
        // foreign address has port 0; the PID is the last column.
        let text = String::from_utf8_lossy(&out.stdout);
        let port_of = |addr: &str| {
            addr.rsplit_once(':')
                .and_then(|(_, p)| p.parse::<u16>().ok())
        };
        let listener = text
            .lines()
            .find_map(|line| {
                let fields: Vec<_> = line.split_whitespace().collect();
                (fields.len() >= 5
                    && fields[0].eq_ignore_ascii_case("TCP")
                    && port_of(fields[1]) == Some(raw.port())
                    && port_of(fields[2]) == Some(0))
                .then(|| fields.last().and_then(|p| p.parse::<u32>().ok()))
                .flatten()
            })
            .unwrap_or_else(|| panic!("no listener on {} in netstat:\n{text}", raw.port()));
        if listener != raw.pid() {
            let parent = parent_pid(listener);
            assert_eq!(
                parent,
                Some(raw.pid()),
                "port {} is held by PID {listener}, neither the spawned Firefox {} nor its child",
                raw.port(),
                raw.pid()
            );
        }
        listener
    }
    #[cfg(not(windows))]
    {
        raw.pid()
    }
}

/// `pid`'s parent PID, from `Win32_Process` (a `wmic`-free PowerShell query).
#[cfg(windows)]
fn parent_pid(pid: u32) -> Option<u32> {
    let out = std::process::Command::new("powershell")
        .args([
            "-NoProfile",
            "-Command",
            &format!("(Get-CimInstance Win32_Process -Filter 'ProcessId={pid}').ParentProcessId"),
        ])
        .output()
        .ok()?;
    String::from_utf8_lossy(&out.stdout).trim().parse().ok()
}
