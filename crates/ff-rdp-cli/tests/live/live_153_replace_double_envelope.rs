//! Live tests for iter-153: `launch --replace` must emit exactly one
//! top-level JSON envelope, and `results.pid` must always name the newly
//! launched instance — never the prior instance that was stopped to make
//! room for it.
//!
//! ## Prospective paired contract (2026-09-24, iteration282)
//!
//! The three positive cases retain every original153 success assertion. They
//! now use one fresh private configured home, archive and remove only their
//! own real launch-record lookup, and retain the real owner marker/start token.
//! A real eval/daemon and positive browser ownership make registry fallback
//! eligible. This explicitly models a missing launch record.
//!
//! Historical default-root browser plus override registry is supported by188.
//! Two different override roots do not supply the same ownership proof. The
//! retained closing1 and native1 failures stay failed; the separate negative
//! case preserves that topology and requires refusal plus browser survival.
//! Neither fixture expands production ownership or transfers a marker.

use crate::common::{ff_rdp_launch_command, live_tests_enabled, pid_alive, recorded_launch_output};
#[path = "support/replace_fixture.rs"]
mod fixture;
use fixture::ProcessGuard as FirefoxGuard;

/// Run `ff-rdp --host 127.0.0.1 --port <port> <args...>` inside an isolated
/// `FF_RDP_HOME` and return `(exit_success, raw_stdout_bytes)`.
fn run_raw(home: &std::path::Path, port: u16, attempt: u8, args: &[&str]) -> (bool, Vec<u8>) {
    let mut full: Vec<String> = vec![
        "--host".into(),
        "127.0.0.1".into(),
        "--port".into(),
        port.to_string(),
        "--timeout".into(),
        "10000".into(),
    ];
    full.extend(args.iter().map(|s| (*s).to_owned()));
    // One writer per retained home; preserve exact piped output before any
    // assertion, including stderr and nonzero status on failed replacement.
    let out = recorded_launch_output(
        ff_rdp_launch_command().env("FF_RDP_HOME", home).args(&full),
        &home.join("commands.attempts.jsonl"),
        attempt,
        port,
    )
    .expect("live_153: failed to run and record ff-rdp");
    (out.status.success(), out.stdout)
}

/// Real owned registry fallback, with hard setup failures. Archive only this
/// fixture's launch-record lookup after eval, leaving its ownership markers
/// untouched. This is the explicitly adopted missing-record arrangement.
fn setup_registry_topology() -> (fixture::Browser, std::path::PathBuf) {
    let home = fixture::home();
    let mut ff = fixture::launch(&home);
    let (ok, stdout) = run_raw(home.as_path(), ff.port(), 0, &["eval", "1"]);
    assert!(
        ok,
        "setup_registry_topology: the `eval` daemon autostart failed for port {}\nstdout={}",
        ff.port(),
        String::from_utf8_lossy(&stdout)
    );
    fixture::establish_registry(&mut ff, &home, &stdout, true);
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
        home.as_path(),
        port,
        1,
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
        .map(|pid| FirefoxGuard::new(pid, &home));

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

    ff.assert_stopped();
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
        home.as_path(),
        port,
        1,
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
    let guard = launched_pid.map(|pid| FirefoxGuard::new(pid, &home));

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

    ff.assert_stopped();
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
        home.as_path(),
        port,
        1,
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
        .map(|pid| FirefoxGuard::new(pid, &home));

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

    ff.assert_stopped();
    eprintln!(
        "live_153_replace_reports_stopped_instance: PASS — meta.replaced={{stopped: true, pid: \
         {prior_pid}}}, guard={:?}",
        guard.map(|g| g.pid())
    );
}

/// Exact two-override topology: proxy authority is not Firefox ownership.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_153_replace_refuses_unproven_outer_override() {
    if !live_tests_enabled() {
        eprintln!(
            "live_153_replace_refuses_unproven_outer_override: set FF_RDP_LIVE_TESTS=1 to run"
        );
        return;
    }
    let outer = fixture::home();
    let mut ff = fixture::launch(&outer);
    let home = fixture::home();
    let port = ff.port();
    let (ok, stdout) = run_raw(&home, port, 0, &["eval", "1"]);
    assert!(
        ok,
        "negative setup eval must really succeed: {}",
        String::from_utf8_lossy(&stdout)
    );
    fixture::establish_registry(&mut ff, &home, &stdout, false);
    let (ok, stdout) = run_raw(
        &home,
        port,
        1,
        &[
            "launch",
            "--headless",
            "--debug-port",
            &port.to_string(),
            "--replace",
        ],
    );
    let _unexpected = fixture::guard_launched_firefox(&stdout, &home);
    let error: serde_json::Value = serde_json::from_slice(&stdout)
        .expect("refusal must be one complete JSON envelope, without trailing data");
    assert!(
        !ok,
        "unproven outer override must refuse replacement: {error}"
    );
    assert_eq!(
        error["error_type"], "User",
        "complete ownership error: {error}"
    );
    let message = error["error"].as_str().expect("ownership error message");
    assert!(
        message.contains("ownership")
            && message.contains("Refusing to stop")
            && !message.contains("stopped Firefox"),
        "truthful browser-ownership refusal: {error}"
    );
    // Assert survival and untouched evidence before the fixture owner's
    // separately birth-validated cleanup. Proxy shutdown is a distinct action.
    ff.assert_survives("after-refusal");
}
