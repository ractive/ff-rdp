//! Live tests for iter-153: `launch --replace` must emit exactly one
//! top-level JSON envelope, and `results.pid` must always name the newly
//! launched instance — never the prior instance that was stopped to make
//! room for it.
//!
//! ## The topology that reproduces the defect
//!
//! `launch --replace`'s internal stop-before-relaunch step
//! (`stop_prior_instance`) has three fallback paths (iter-90): a
//! `DaemonRecord` match, a proxy-daemon registry match, and a raw
//! port-owner kill. The double-envelope defect lived specifically in the
//! *registry* path: `stop_prior_instance` used to call `run_daemon_stop`
//! there, and `run_daemon_stop` prints its own top-level envelope — from
//! *inside* `launch`'s own command run.
//!
//! [`LiveFirefox`] launches Firefox via `ff-rdp launch` against the REAL
//! `$HOME`, so no `DaemonRecord` exists for its port under the isolated
//! `FF_RDP_HOME` these tests use — the `DaemonRecord` path is a guaranteed
//! miss. An `eval` call inside that isolated home then auto-starts a
//! registry-tracked proxy daemon for the port, so `launch --replace` is
//! forced into the registry path — reproduced against real Firefox and
//! confirmed against a pre-fix build during iter-153 development (see
//! [[iteration-153-launch-replace-double-envelope]]): stdout was
//! `{"results":{"stopped":true,"pid":<prior>},"total":1}` immediately
//! followed by the launch envelope, and `serde_json` parsing of the full
//! buffer failed with "trailing characters".
//!
//! Run with:
//!   FF_RDP_LIVE_TESTS=1 cargo test-live -p ff-rdp-cli \
//!       --test live live_153 -- --nocapture

use std::process::Command;

use crate::common::{FirefoxGuard, LiveFirefox, ff_rdp_bin, live_tests_enabled, pid_alive};

/// Run `ff-rdp --host 127.0.0.1 --port <port> <args...>` inside an isolated
/// `FF_RDP_HOME` and return `(exit_success, raw_stdout_bytes)`.
fn run_raw(home: &std::path::Path, port: u16, args: &[&str]) -> (bool, Vec<u8>) {
    let mut full: Vec<String> = vec![
        "--host".into(),
        "127.0.0.1".into(),
        "--port".into(),
        port.to_string(),
        "--timeout".into(),
        "10000".into(),
    ];
    full.extend(args.iter().map(|s| (*s).to_owned()));
    let out = Command::new(ff_rdp_bin())
        .env("FF_RDP_HOME", home)
        .args(&full)
        .output()
        .expect("live_153: failed to spawn ff-rdp");
    (out.status.success(), out.stdout)
}

/// Build the topology described in the module doc: a `LiveFirefox` with no
/// `DaemonRecord` under the isolated `home`, plus a registry-tracked proxy
/// daemon autostarted inside that `home` — forcing `stop_prior_instance`
/// into the registry fallback path where the double-envelope defect lived.
///
/// Panics (never returns a skip) when Firefox or the autostart is
/// unavailable — iter-158 Theme D. This suite is the reason: on 2026-08-13
/// `live_153_replace_emits_single_envelope` was the one real product defect a
/// full sweep found, and it only surfaced because it happened to fail loudly.
/// Every sibling that skipped instead reported `ok`.
fn setup_registry_topology() -> (LiveFirefox, tempfile::TempDir) {
    let ff = LiveFirefox::headless_on_random_port();
    let home = tempfile::tempdir().expect("live_153: tempdir for FF_RDP_HOME");
    // `eval` routes through `resolve_connection_target`, which auto-starts a
    // registry-tracked proxy daemon inside `home` for `ff.port()` when none
    // is already running there.
    let (ok, stdout) = run_raw(home.path(), ff.port(), &["eval", "1"]);
    assert!(
        ok,
        "setup_registry_topology: the `eval` daemon autostart failed for port {}\nstdout={}",
        ff.port(),
        String::from_utf8_lossy(&stdout)
    );
    (ff, home)
}

/// AC `live_153_replace_emits_single_envelope`: stdout of `launch --replace`
/// against a prior instance *with* a daemon record (here: a registry-tracked
/// proxy daemon, the topology that used to trigger the nested print) parses
/// as exactly one JSON document — no trailing data.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_153_replace_emits_single_envelope() {
    if !live_tests_enabled() {
        eprintln!("live_153_replace_emits_single_envelope: set FF_RDP_LIVE_TESTS=1 to run");
        return;
    }
    let (ff, home) = setup_registry_topology();
    let port = ff.port();

    let (ok, stdout) = run_raw(
        home.path(),
        port,
        &[
            "launch",
            "--headless",
            "--debug-port",
            &port.to_string(),
            "--replace",
        ],
    );

    // Bind a reap guard from whatever pid can be parsed BEFORE any assertion
    // (mirrors the pattern in live_86 / live_123) so a panic below still
    // unwinds through the kill and this test never leaks a replacement.
    let guard: Option<FirefoxGuard> = serde_json::from_slice::<serde_json::Value>(&stdout)
        .ok()
        .and_then(|j| j["results"]["pid"].as_u64())
        .and_then(|p| u32::try_from(p).ok())
        .map(FirefoxGuard::new);

    assert!(
        ok,
        "live_153_replace_emits_single_envelope: FAIL — launch --replace returned non-zero\n\
         stdout={}",
        String::from_utf8_lossy(&stdout)
    );

    // The actual regression check: parse the WHOLE buffer as a single JSON
    // document and fail on trailing data. A substring grep (e.g. checking
    // stdout contains `"results"`) would pass on both the buggy two-envelope
    // output and the fixed single-envelope output — only a full-buffer parse
    // that rejects trailing bytes catches the defect (iter-153 Theme C).
    let mut de = serde_json::Deserializer::from_slice(&stdout);
    let _value: serde_json::Value = serde::Deserialize::deserialize(&mut de).unwrap_or_else(|e| {
        panic!(
            "live_153_replace_emits_single_envelope: FAIL — stdout is not valid JSON: {e}\n\
             stdout={}",
            String::from_utf8_lossy(&stdout)
        )
    });
    de.end().unwrap_or_else(|e| {
        panic!(
            "live_153_replace_emits_single_envelope: FAIL — stdout carries trailing data after \
             the first JSON document (the double-envelope defect): {e}\nstdout={}",
            String::from_utf8_lossy(&stdout)
        )
    });

    eprintln!(
        "live_153_replace_emits_single_envelope: PASS — stdout parses as exactly one JSON \
         document, replacement pid={:?}",
        guard.map(|g| g.pid())
    );
}

/// AC `live_153_replace_reports_launched_pid`: `results.pid` of that envelope
/// is the PID of the newly launched Firefox — alive immediately after the
/// command — and not the stopped one.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_153_replace_reports_launched_pid() {
    if !live_tests_enabled() {
        eprintln!("live_153_replace_reports_launched_pid: set FF_RDP_LIVE_TESTS=1 to run");
        return;
    }
    let (ff, home) = setup_registry_topology();
    let port = ff.port();
    let prior_pid = ff.pid();

    let (ok, stdout) = run_raw(
        home.path(),
        port,
        &[
            "launch",
            "--headless",
            "--debug-port",
            &port.to_string(),
            "--replace",
        ],
    );

    let json: serde_json::Value = serde_json::from_slice(&stdout).unwrap_or_else(|e| {
        panic!(
            "live_153_replace_reports_launched_pid: FAIL — stdout did not parse as a single \
             JSON document: {e}\nstdout={}",
            String::from_utf8_lossy(&stdout)
        )
    });

    let launched_pid = json["results"]["pid"]
        .as_u64()
        .and_then(|p| u32::try_from(p).ok());
    // Bind the guard before any assertion so a panic still unwinds through
    // the kill.
    let guard = launched_pid.map(FirefoxGuard::new);

    assert!(
        ok,
        "live_153_replace_reports_launched_pid: FAIL — launch --replace returned non-zero: \
         {json}"
    );
    let launched_pid = launched_pid
        .expect("live_153_replace_reports_launched_pid: results.pid missing from envelope");

    assert_ne!(
        launched_pid, prior_pid,
        "live_153_replace_reports_launched_pid: FAIL — results.pid ({launched_pid}) equals the \
         STOPPED prior instance's pid ({prior_pid}); it must name the newly launched instance"
    );
    assert!(
        pid_alive(launched_pid),
        "live_153_replace_reports_launched_pid: FAIL — results.pid ({launched_pid}) is not \
         alive immediately after launch --replace reported success"
    );

    eprintln!(
        "live_153_replace_reports_launched_pid: PASS — results.pid={launched_pid} is alive and \
         distinct from the stopped prior instance (pid {prior_pid}), guard={:?}",
        guard.map(|g| g.pid())
    );
}

/// AC `live_153_replace_reports_stopped_instance`: the stopped instance's PID
/// is still discoverable in the chosen shape (`meta.replaced`) — nothing is
/// silently dropped by folding the stop outcome into the launch envelope.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_153_replace_reports_stopped_instance() {
    if !live_tests_enabled() {
        eprintln!("live_153_replace_reports_stopped_instance: set FF_RDP_LIVE_TESTS=1 to run");
        return;
    }
    let (ff, home) = setup_registry_topology();
    let port = ff.port();
    let prior_pid = ff.pid();

    let (ok, stdout) = run_raw(
        home.path(),
        port,
        &[
            "launch",
            "--headless",
            "--debug-port",
            &port.to_string(),
            "--replace",
        ],
    );

    let json: serde_json::Value = serde_json::from_slice(&stdout).unwrap_or_else(|e| {
        panic!(
            "live_153_replace_reports_stopped_instance: FAIL — stdout did not parse as a \
             single JSON document: {e}\nstdout={}",
            String::from_utf8_lossy(&stdout)
        )
    });
    let guard = json["results"]["pid"]
        .as_u64()
        .and_then(|p| u32::try_from(p).ok())
        .map(FirefoxGuard::new);

    assert!(
        ok,
        "live_153_replace_reports_stopped_instance: FAIL — launch --replace returned non-zero: \
         {json}"
    );

    let replaced = &json["meta"]["replaced"];
    assert!(
        replaced.is_object(),
        "live_153_replace_reports_stopped_instance: FAIL — meta.replaced is missing; the \
         stopped instance's outcome was silently dropped: {json}"
    );
    assert_eq!(
        replaced["stopped"].as_bool(),
        Some(true),
        "live_153_replace_reports_stopped_instance: FAIL — meta.replaced.stopped must be true: \
         {json}"
    );
    let reported_stopped_pid = replaced["pid"].as_u64().and_then(|p| u32::try_from(p).ok());
    assert_eq!(
        reported_stopped_pid,
        Some(prior_pid),
        "live_153_replace_reports_stopped_instance: FAIL — meta.replaced.pid must be the prior \
         instance's pid ({prior_pid}), got {reported_stopped_pid:?}: {json}"
    );

    eprintln!(
        "live_153_replace_reports_stopped_instance: PASS — meta.replaced={{stopped: true, pid: \
         {prior_pid}}}, guard={:?}",
        guard.map(|g| g.pid())
    );
}
