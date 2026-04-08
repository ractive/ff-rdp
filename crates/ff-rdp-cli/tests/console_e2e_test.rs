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

fn console_server() -> MockRdpServer {
    MockRdpServer::new()
        .on("listTabs", load_fixture("list_tabs_response.json"))
        .on("getTarget", load_fixture("get_target_response.json"))
        .on(
            "startListeners",
            load_fixture("start_listeners_response.json"),
        )
        .on(
            "getCachedMessages",
            load_fixture("get_cached_messages_response.json"),
        )
}

// ---------------------------------------------------------------------------
// Happy-path tests
// ---------------------------------------------------------------------------

#[test]
fn console_shows_all_messages() {
    let server = console_server();
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.push("console".to_owned());

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

    assert_eq!(json["total"], 3);
    let results = json["results"].as_array().expect("results is array");
    // Default sort is timestamp desc (newest first).
    assert_eq!(results[0]["level"], "error");
    assert_eq!(results[0]["message"], "error msg");
    assert_eq!(results[1]["level"], "warn");
    assert_eq!(results[1]["message"], "warning msg");
    assert_eq!(results[2]["level"], "log");
    assert_eq!(results[2]["message"], "hello from test");
}

#[test]
fn console_filter_by_level() {
    let server = console_server();
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend([
        "console".to_owned(),
        "--level".to_owned(),
        "error".to_owned(),
    ]);

    let output = std::process::Command::new(ff_rdp_bin())
        .args(&args)
        .output()
        .expect("failed to spawn ff-rdp");

    handle.join().unwrap();

    assert!(output.status.success());

    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["total"], 1);
    let results = json["results"].as_array().unwrap();
    assert_eq!(results[0]["level"], "error");
    assert_eq!(results[0]["message"], "error msg");
}

#[test]
fn console_filter_by_pattern() {
    let server = console_server();
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend([
        "console".to_owned(),
        "--pattern".to_owned(),
        "warn".to_owned(),
    ]);

    let output = std::process::Command::new(ff_rdp_bin())
        .args(&args)
        .output()
        .expect("failed to spawn ff-rdp");

    handle.join().unwrap();

    assert!(output.status.success());

    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["total"], 1);
    let results = json["results"].as_array().unwrap();
    assert_eq!(results[0]["message"], "warning msg");
}

#[test]
fn console_with_jq_filter() {
    let server = console_server();
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend([
        "console".to_owned(),
        "--jq".to_owned(),
        ".results[] | select(.level == \"error\") | .message".to_owned(),
    ]);

    let output = std::process::Command::new(ff_rdp_bin())
        .args(&args)
        .output()
        .expect("failed to spawn ff-rdp");

    handle.join().unwrap();

    assert!(output.status.success());

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(stdout.trim(), r#""error msg""#);
}

#[test]
fn console_level_and_pattern_combined() {
    let server = console_server();
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend([
        "console".to_owned(),
        "--level".to_owned(),
        "log".to_owned(),
        "--pattern".to_owned(),
        "hello".to_owned(),
    ]);

    let output = std::process::Command::new(ff_rdp_bin())
        .args(&args)
        .output()
        .expect("failed to spawn ff-rdp");

    handle.join().unwrap();

    assert!(output.status.success());

    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["total"], 1);
    let results = json["results"].as_array().unwrap();
    assert_eq!(results[0]["message"], "hello from test");
}

#[test]
fn console_no_match_returns_empty() {
    let server = console_server();
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend([
        "console".to_owned(),
        "--level".to_owned(),
        "debug".to_owned(),
    ]);

    let output = std::process::Command::new(ff_rdp_bin())
        .args(&args)
        .output()
        .expect("failed to spawn ff-rdp");

    handle.join().unwrap();

    assert!(output.status.success());

    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["total"], 0);
    assert_eq!(json["results"].as_array().unwrap().len(), 0);
}

#[test]
fn console_handles_page_error_messages() {
    // Verify that pageError-type messages are parsed alongside consoleAPICall messages.
    let response_with_page_errors = serde_json::json!({
        "from": "server1.conn0.child2/consoleActor3",
        "messages": [
            {
                "message": {
                    "arguments": ["hello"],
                    "level": "log",
                    "filename": "test.js",
                    "lineNumber": 1,
                    "columnNumber": 1,
                    "timeStamp": 1000.0
                },
                "type": "consoleAPICall"
            },
            {
                "pageError": {
                    "errorMessage": "ReferenceError: foo is not defined",
                    "sourceName": "https://example.com/app.js",
                    "lineNumber": 42,
                    "columnNumber": 5,
                    "timeStamp": 2000.0
                },
                "type": "pageError"
            }
        ]
    });

    let server = MockRdpServer::new()
        .on("listTabs", load_fixture("list_tabs_response.json"))
        .on("getTarget", load_fixture("get_target_response.json"))
        .on(
            "startListeners",
            load_fixture("start_listeners_response.json"),
        )
        .on("getCachedMessages", response_with_page_errors);

    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.push("console".to_owned());

    let output = std::process::Command::new(ff_rdp_bin())
        .args(&args)
        .output()
        .expect("failed to spawn ff-rdp");

    handle.join().unwrap();

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["total"], 2);
    let results = json["results"].as_array().unwrap();
    // Default sort is timestamp desc (newest first).
    // pageError has timeStamp=2000, consoleAPICall has timeStamp=1000.
    assert_eq!(results[0]["level"], "error");
    assert_eq!(results[0]["message"], "ReferenceError: foo is not defined");
    assert_eq!(results[1]["level"], "log");
    assert_eq!(results[1]["message"], "hello");
}

// ---------------------------------------------------------------------------
// --limit flag truncates results
// ---------------------------------------------------------------------------

#[test]
fn console_limit_truncates_results() {
    let server = console_server();
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend(["--limit".to_owned(), "2".to_owned(), "console".to_owned()]);

    let output = std::process::Command::new(ff_rdp_bin())
        .args(&args)
        .output()
        .expect("failed to spawn ff-rdp");

    handle.join().unwrap();

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    // Total reflects actual count (3), but only 2 shown.
    assert_eq!(json["total"], 3);
    assert_eq!(json["results"].as_array().unwrap().len(), 2);
    assert_eq!(json["truncated"], true);
    assert!(json["hint"].as_str().unwrap().contains("--all"));
}

// ---------------------------------------------------------------------------
// --follow flag: streams messages as NDJSON until connection closes
// ---------------------------------------------------------------------------

fn follow_server_with_events(console_event: serde_json::Value) -> MockRdpServer {
    // `close_after_followups` causes the server to drop the connection
    // immediately after delivering the console event followup.  The client's
    // follow_loop then receives EOF and exits cleanly without blocking.
    MockRdpServer::new()
        .on("listTabs", load_fixture("list_tabs_response.json"))
        .on("getTarget", load_fixture("get_target_response.json"))
        // run_follow_direct calls startListeners before subscribing via the
        // Watcher to ensure console events flow through the watcher subscription.
        .on(
            "startListeners",
            load_fixture("start_listeners_response.json"),
        )
        .on("getWatcher", load_fixture("get_watcher_response.json"))
        .on_with_followups(
            "watchResources",
            load_fixture("watch_resources_response.json"),
            vec![console_event],
        )
        .on(
            "unwatchResources",
            load_fixture("unwatch_resources_response.json"),
        )
        .close_after_followups()
}

#[test]
fn console_follow_streams_messages_as_ndjson() {
    let console_event = json!({
        "type": "resources-available-array",
        "from": "server1.conn0.watcher4",
        "array": [
            ["console-message", [
                {
                    "resourceType": "console-message",
                    "message": {
                        "arguments": ["live message 1"],
                        "level": "log",
                        "filename": "test.js",
                        "lineNumber": 1,
                        "columnNumber": 1,
                        "timeStamp": 1000.0
                    }
                },
                {
                    "resourceType": "console-message",
                    "message": {
                        "arguments": ["live message 2"],
                        "level": "warn",
                        "filename": "test.js",
                        "lineNumber": 2,
                        "columnNumber": 1,
                        "timeStamp": 2000.0
                    }
                }
            ]]
        ]
    });

    let server = follow_server_with_events(console_event);
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend(["console".to_owned(), "--follow".to_owned()]);

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

    let stdout = String::from_utf8_lossy(&output.stdout);
    // Each message is emitted as a separate JSON line (NDJSON).
    let lines: Vec<&str> = stdout.trim().lines().collect();
    assert_eq!(lines.len(), 2, "expected 2 NDJSON lines, got: {stdout}");

    let msg1: serde_json::Value =
        serde_json::from_str(lines[0]).expect("line 1 must be valid JSON");
    assert_eq!(msg1["level"], "log");
    assert_eq!(msg1["message"], "live message 1");
    assert_eq!(msg1["source"], "test.js");

    let msg2: serde_json::Value =
        serde_json::from_str(lines[1]).expect("line 2 must be valid JSON");
    assert_eq!(msg2["level"], "warn");
    assert_eq!(msg2["message"], "live message 2");
}

#[test]
fn console_follow_level_filter_applies_to_stream() {
    // Only warn-level messages should be emitted when --level warn is given.
    let console_event = json!({
        "type": "resources-available-array",
        "from": "server1.conn0.watcher4",
        "array": [
            ["console-message", [
                {
                    "resourceType": "console-message",
                    "message": {
                        "arguments": ["debug noise"],
                        "level": "debug",
                        "filename": "app.js",
                        "lineNumber": 10,
                        "columnNumber": 1,
                        "timeStamp": 500.0
                    }
                },
                {
                    "resourceType": "console-message",
                    "message": {
                        "arguments": ["important warning"],
                        "level": "warn",
                        "filename": "app.js",
                        "lineNumber": 20,
                        "columnNumber": 1,
                        "timeStamp": 600.0
                    }
                }
            ]]
        ]
    });

    let server = follow_server_with_events(console_event);
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend([
        "console".to_owned(),
        "--follow".to_owned(),
        "--level".to_owned(),
        "warn".to_owned(),
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

    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<&str> = stdout.trim().lines().collect();
    assert_eq!(lines.len(), 1, "expected only 1 (warn) line, got: {stdout}");

    let msg: serde_json::Value = serde_json::from_str(lines[0]).expect("output must be valid JSON");
    assert_eq!(msg["level"], "warn");
    assert_eq!(msg["message"], "important warning");
}

// ---------------------------------------------------------------------------
// Direct consoleAPICall push (Firefox 149+ eval-triggered messages)
// ---------------------------------------------------------------------------

/// Build a server that delivers a direct `consoleAPICall` push notification
/// (the path taken by Firefox 149+ when `console.log()` is called via eval).
///
/// The `consoleAPICall` event arrives directly from the console actor rather
/// than via the Watcher's `resources-available-array` stream.
fn follow_server_with_direct_notification(notification: serde_json::Value) -> MockRdpServer {
    MockRdpServer::new()
        .on("listTabs", load_fixture("list_tabs_response.json"))
        .on("getTarget", load_fixture("get_target_response.json"))
        .on(
            "startListeners",
            load_fixture("start_listeners_response.json"),
        )
        .on("getWatcher", load_fixture("get_watcher_response.json"))
        .on_with_followups(
            "watchResources",
            load_fixture("watch_resources_response.json"),
            vec![notification],
        )
        .on(
            "unwatchResources",
            load_fixture("unwatch_resources_response.json"),
        )
        .close_after_followups()
}

#[test]
fn console_follow_handles_direct_consoleapicall_notification() {
    // Simulate Firefox 149+ sending a `consoleAPICall` push directly on the
    // console actor (triggered by console.log() called from evaluateJSAsync).
    let notification = json!({
        "type": "consoleAPICall",
        "from": "server1.conn0.child2/consoleActor3",
        "message": {
            "arguments": ["eval log output"],
            "level": "log",
            "filename": "debugger eval code",
            "lineNumber": 1,
            "columnNumber": 9,
            "timeStamp": 1_775_439_071_165.699_f64
        }
    });

    let server = follow_server_with_direct_notification(notification);
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend(["console".to_owned(), "--follow".to_owned()]);

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

    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<&str> = stdout.trim().lines().collect();
    assert_eq!(
        lines.len(),
        1,
        "expected 1 NDJSON line from direct push, got: {stdout}"
    );

    let msg: serde_json::Value = serde_json::from_str(lines[0]).expect("output must be valid JSON");
    assert_eq!(msg["level"], "log");
    assert_eq!(msg["message"], "eval log output");
    assert_eq!(msg["source"], "debugger eval code");
}

#[test]
fn console_follow_handles_direct_pageerror_notification() {
    // Simulate Firefox 149+ sending a `pageError` push directly on the
    // console actor.
    let notification = json!({
        "type": "pageError",
        "from": "server1.conn0.child2/consoleActor3",
        "pageError": {
            "errorMessage": "ReferenceError: x is not defined",
            "sourceName": "https://example.com/app.js",
            "lineNumber": 42,
            "columnNumber": 5,
            "timeStamp": 1_775_439_071_200.0_f64
        }
    });

    let server = follow_server_with_direct_notification(notification);
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend(["console".to_owned(), "--follow".to_owned()]);

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

    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<&str> = stdout.trim().lines().collect();
    assert_eq!(
        lines.len(),
        1,
        "expected 1 NDJSON line from direct pageError push, got: {stdout}"
    );

    let msg: serde_json::Value = serde_json::from_str(lines[0]).expect("output must be valid JSON");
    assert_eq!(msg["level"], "error");
    assert_eq!(msg["message"], "ReferenceError: x is not defined");
    assert_eq!(msg["source"], "https://example.com/app.js");
}
