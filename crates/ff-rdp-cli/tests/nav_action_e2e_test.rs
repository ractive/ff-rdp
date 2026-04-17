mod support;

use serde_json::json;
use support::{MockRdpServer, load_fixture};

fn ff_rdp_bin() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_BIN_EXE_ff-rdp"))
}

fn base_args(port: u16) -> Vec<String> {
    vec![
        "--host".to_owned(),
        "127.0.0.1".to_owned(),
        "--port".to_owned(),
        port.to_string(),
        "--no-daemon".to_owned(),
    ]
}

fn nav_action_server(method: &str) -> MockRdpServer {
    MockRdpServer::new()
        .on("listTabs", load_fixture("list_tabs_response.json"))
        .on("getTarget", load_fixture("get_target_response.json"))
        .on(method, load_fixture("reload_response.json"))
}

#[test]
fn reload_outputs_json_envelope() {
    let server = nav_action_server("reload");
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.push("reload".to_owned());

    let output = std::process::Command::new(ff_rdp_bin())
        .args(&args)
        .output()
        .expect("failed to spawn ff-rdp");

    handle.join().unwrap();

    assert!(
        output.status.success(),
        "expected success, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("stdout must be valid JSON");

    assert_eq!(json["results"]["action"], "reload");
}

// ---------------------------------------------------------------------------
// reload --wait-idle
// ---------------------------------------------------------------------------

/// Build a mock server that:
/// 1. Responds to listTabs, getTarget, getWatcher, watchResources
/// 2. After watchResources, pushes a network event batch (simulating page reload traffic)
/// 3. Closes the connection after sending followups so the idle loop gets EOF
///    and returns cleanly (simulates the "idle" condition).
fn reload_wait_idle_server(network_events: Vec<serde_json::Value>) -> MockRdpServer {
    MockRdpServer::new()
        .on("listTabs", load_fixture("list_tabs_response.json"))
        .on("getTarget", load_fixture("get_target_response.json"))
        .on("getWatcher", load_fixture("get_watcher_response.json"))
        .on_with_followups(
            "watchResources",
            load_fixture("watch_resources_response.json"),
            network_events,
        )
        // No reload handler needed — the raw reload send gets an "unknownMethod"
        // error back from the mock, which is harmlessly ignored by the idle loop.
        .on(
            "unwatchResources",
            load_fixture("unwatch_resources_response.json"),
        )
        .close_after_followups()
}

#[test]
fn reload_wait_idle_observes_network_events() {
    let network_event = json!({
        "type": "resources-available-array",
        "from": "server1.conn0.watcher4",
        "array": [
            ["network-event", [
                {
                    "resourceType": "network-event",
                    "actor": "server1.conn0.netActor1",
                    "startedDateTime": "2026-01-01T00:00:00.000Z",
                    "url": "https://example.com/style.css",
                    "method": "GET",
                    "isXHR": false,
                    "cause": {"type": "stylesheet"},
                    "fromCache": false,
                    "private": false
                },
                {
                    "resourceType": "network-event",
                    "actor": "server1.conn0.netActor2",
                    "startedDateTime": "2026-01-01T00:00:00.010Z",
                    "url": "https://example.com/app.js",
                    "method": "GET",
                    "isXHR": false,
                    "cause": {"type": "script"},
                    "fromCache": false,
                    "private": false
                }
            ]]
        ]
    });

    let server = reload_wait_idle_server(vec![network_event]);
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend([
        "reload".to_owned(),
        "--wait-idle".to_owned(),
        "--idle-ms".to_owned(),
        "500".to_owned(),
        "--reload-timeout".to_owned(),
        "5000".to_owned(),
    ]);

    let output = std::process::Command::new(ff_rdp_bin())
        .args(&args)
        .output()
        .expect("failed to spawn ff-rdp");

    handle.join().expect("server thread panicked");

    assert!(
        output.status.success(),
        "expected success, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("stdout must be valid JSON");

    assert_eq!(json["results"]["reloaded"], true);
    // Should have observed 2 network resources from the batch.
    assert_eq!(
        json["results"]["requests_observed"], 2,
        "should count 2 network events from the batch"
    );
    // idle_at_ms is present (may be 0 if connection closed immediately)
    assert!(
        !json["results"]["idle_at_ms"].is_null(),
        "idle_at_ms must be present in output"
    );
}

#[test]
fn reload_wait_idle_no_traffic_returns_idle_quickly() {
    // With no network events the loop exits when the mock server closes the
    // connection (EOF path in the idle-drain loop).  Since `last_event_at` is
    // only set once a non-empty network-event batch arrives, the total timeout
    // would govern on a live server with zero traffic — but in the mock the
    // connection closes after the followup batch is delivered, which triggers
    // the EOF break and returns before any timeout fires.
    // We use a single dummy empty followup batch to trigger the
    // close_after_followups behaviour.
    let empty_batch = json!({
        "type": "resources-available-array",
        "from": "server1.conn0.watcher4",
        "array": [["network-event", []]]
    });

    let server = reload_wait_idle_server(vec![empty_batch]);
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend([
        "reload".to_owned(),
        "--wait-idle".to_owned(),
        "--idle-ms".to_owned(),
        "100".to_owned(),
        "--reload-timeout".to_owned(),
        "5000".to_owned(),
    ]);

    let output = std::process::Command::new(ff_rdp_bin())
        .args(&args)
        .output()
        .expect("failed to spawn ff-rdp");

    handle.join().expect("server thread panicked");

    assert!(
        output.status.success(),
        "expected success, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("stdout must be valid JSON");
    assert_eq!(json["results"]["reloaded"], true);
    assert_eq!(json["results"]["requests_observed"], 0);
}

#[test]
fn back_outputs_json_envelope() {
    let server = nav_action_server("goBack");
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.push("back".to_owned());

    let output = std::process::Command::new(ff_rdp_bin())
        .args(&args)
        .output()
        .expect("failed to spawn ff-rdp");

    handle.join().unwrap();

    assert!(
        output.status.success(),
        "expected success, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["results"]["action"], "back");
}

#[test]
fn forward_outputs_json_envelope() {
    let server = nav_action_server("goForward");
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.push("forward".to_owned());

    let output = std::process::Command::new(ff_rdp_bin())
        .args(&args)
        .output()
        .expect("failed to spawn ff-rdp");

    handle.join().unwrap();

    assert!(
        output.status.success(),
        "expected success, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["results"]["action"], "forward");
}
