use super::support::{MockRdpServer, load_fixture};

fn ff_rdp_bin() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_BIN_EXE_ff-rdp"))
}

fn base_args(port: u16) -> Vec<String> {
    vec![
        "--host".to_owned(),
        "127.0.0.1".to_owned(),
        "--port".to_owned(),
        port.to_string(),
        // Short timeout so the event drain loop exits quickly.
        "--timeout".to_owned(),
        "1000".to_owned(),
        "--no-daemon".to_owned(),
    ]
}

fn network_server() -> MockRdpServer {
    MockRdpServer::new()
        .on("listTabs", load_fixture("list_tabs_response.json"))
        .on("getTarget", load_fixture("get_target_response.json"))
        .on("getWatcher", load_fixture("get_watcher_response.json"))
        .on_with_followups(
            "watchResources",
            load_fixture("watch_resources_response.json"),
            vec![
                load_fixture("resources_available_network.json"),
                load_fixture("resources_updated_network.json"),
            ],
        )
        // unwatchResources is called during cleanup; provide a response.
        .on(
            "unwatchResources",
            load_fixture("watch_resources_response.json"),
        )
}

// ---------------------------------------------------------------------------
// Summary mode (default)
// ---------------------------------------------------------------------------

#[test]
fn network_shows_summary_by_default() {
    let server = network_server();
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.push("network".to_owned());

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

    // Summary mode: results is an object, not an array.
    assert!(
        json["results"].is_object(),
        "default network output should be summary (object), got: {}",
        json["results"]
    );
    assert_eq!(json["results"]["total_requests"], 2);
    assert!(json["results"]["slowest"].is_array());
    assert!(json["results"]["by_cause_type"].is_object());
}

// ---------------------------------------------------------------------------
// Detail mode (--detail flag)
// ---------------------------------------------------------------------------

#[test]
fn network_detail_shows_requests() {
    let server = network_server();
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend(["--detail".to_owned(), "network".to_owned()]);

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

    assert_eq!(json["total"], 2);
    let results = json["results"].as_array().expect("results is array");

    // Results are sorted by duration_ms desc (default for detail mode).
    assert_eq!(results[0]["method"], "GET");
    assert_eq!(results[0]["url"], "https://example.com/");
    assert_eq!(results[0]["status"], 200);
    assert_eq!(results[0]["is_xhr"], false);

    assert_eq!(results[1]["method"], "GET");
    assert_eq!(results[1]["url"], "https://example.com/favicon.ico");
    assert_eq!(results[1]["status"], 404);
}

// ---------------------------------------------------------------------------
// --limit flag triggers detail mode and truncates
// ---------------------------------------------------------------------------

#[test]
fn network_limit_shows_detail_with_truncation() {
    let server = network_server();
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend(["--limit".to_owned(), "1".to_owned(), "network".to_owned()]);

    let output = std::process::Command::new(ff_rdp_bin())
        .args(&args)
        .output()
        .expect("failed to spawn ff-rdp");

    handle.join().unwrap();

    assert!(output.status.success());

    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    // Total should reflect the actual count before truncation.
    assert_eq!(json["total"], 2);
    // Only 1 result shown.
    assert_eq!(json["results"].as_array().unwrap().len(), 1);
    assert_eq!(json["truncated"], true);
    assert!(json["hint"].as_str().unwrap().contains("--all"));
}

// ---------------------------------------------------------------------------
// --all flag overrides default limit in detail mode
// ---------------------------------------------------------------------------

#[test]
fn network_all_overrides_limit() {
    let server = network_server();
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend(["--all".to_owned(), "network".to_owned()]);

    let output = std::process::Command::new(ff_rdp_bin())
        .args(&args)
        .output()
        .expect("failed to spawn ff-rdp");

    handle.join().unwrap();

    assert!(output.status.success());

    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["total"], 2);
    // All 2 results shown, no truncation.
    assert_eq!(json["results"].as_array().unwrap().len(), 2);
    assert!(json.get("truncated").is_none());
}

// ---------------------------------------------------------------------------
// --filter URL
// ---------------------------------------------------------------------------

#[test]
fn network_filter_by_url() {
    let server = network_server();
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend([
        "--detail".to_owned(),
        "network".to_owned(),
        "--filter".to_owned(),
        "favicon".to_owned(),
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
    assert_eq!(results[0]["url"], "https://example.com/favicon.ico");
}

// ---------------------------------------------------------------------------
// --method filter
// ---------------------------------------------------------------------------

#[test]
fn network_filter_by_method() {
    let server = network_server();
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend([
        "--detail".to_owned(),
        "network".to_owned(),
        "--method".to_owned(),
        "POST".to_owned(),
    ]);

    let output = std::process::Command::new(ff_rdp_bin())
        .args(&args)
        .output()
        .expect("failed to spawn ff-rdp");

    handle.join().unwrap();

    assert!(output.status.success());

    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    // No POST requests in our fixtures — should be empty.
    assert_eq!(json["total"], 0);
}

// ---------------------------------------------------------------------------
// --jq filter activates detail mode
// ---------------------------------------------------------------------------

#[test]
fn network_with_jq_filter() {
    let server = network_server();
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend([
        "--jq".to_owned(),
        ".results[] | select(.status >= 400)".to_owned(),
        "network".to_owned(),
    ]);

    let output = std::process::Command::new(ff_rdp_bin())
        .args(&args)
        .output()
        .expect("failed to spawn ff-rdp");

    handle.join().unwrap();

    assert!(output.status.success());

    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(json["status"], 404);
    assert_eq!(json["url"], "https://example.com/favicon.ico");
}

// ---------------------------------------------------------------------------
// Empty result set
// ---------------------------------------------------------------------------

#[test]
fn network_empty_when_no_events() {
    // Server without the watchResources followups — no events arrive.
    let server = MockRdpServer::new()
        .on("listTabs", load_fixture("list_tabs_response.json"))
        .on("getTarget", load_fixture("get_target_response.json"))
        .on("getWatcher", load_fixture("get_watcher_response.json"))
        .on(
            "watchResources",
            load_fixture("watch_resources_response.json"),
        )
        .on(
            "unwatchResources",
            load_fixture("watch_resources_response.json"),
        );

    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.push("network".to_owned());

    let output = std::process::Command::new(ff_rdp_bin())
        .args(&args)
        .output()
        .expect("failed to spawn ff-rdp");

    handle.join().unwrap();

    assert!(output.status.success());

    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    // Summary mode with no entries: total_requests = 0
    assert_eq!(json["results"]["total_requests"], 0);
}

// ---------------------------------------------------------------------------
// Performance API fallback when watcher has no events
// ---------------------------------------------------------------------------

#[test]
fn network_falls_back_to_performance_api_when_watcher_empty() {
    // Watcher returns no network events (no followups); Performance API eval
    // returns two resource entries as a plain JSON array.
    let server = MockRdpServer::new()
        .on("listTabs", load_fixture("list_tabs_response.json"))
        .on("getTarget", load_fixture("get_target_response.json"))
        .on("getWatcher", load_fixture("get_watcher_response.json"))
        .on(
            "watchResources",
            load_fixture("watch_resources_response.json"),
        )
        .on(
            "unwatchResources",
            load_fixture("watch_resources_response.json"),
        )
        .on_with_followup(
            "evaluateJSAsync",
            load_fixture("eval_immediate_response.json"),
            load_fixture("eval_result_network_perf_fallback.json"),
        );

    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend(["--detail".to_owned(), "network".to_owned()]);

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

    // Should have fallen back to Performance API and returned 2 entries.
    assert_eq!(json["total"], 2, "expected 2 entries from perf fallback");
    let results = json["results"].as_array().expect("results is array");
    // All entries should have source = "performance-api".
    for entry in results {
        assert_eq!(
            entry["source"], "performance-api",
            "expected performance-api source, got: {entry}"
        );
    }
    // Meta should advertise the performance-api source.
    assert_eq!(json["meta"]["source"], "performance-api");
}

#[test]
fn network_summary_falls_back_to_performance_api() {
    // Summary mode: watcher empty, perf fallback returns 2 entries.
    let server = MockRdpServer::new()
        .on("listTabs", load_fixture("list_tabs_response.json"))
        .on("getTarget", load_fixture("get_target_response.json"))
        .on("getWatcher", load_fixture("get_watcher_response.json"))
        .on(
            "watchResources",
            load_fixture("watch_resources_response.json"),
        )
        .on(
            "unwatchResources",
            load_fixture("watch_resources_response.json"),
        )
        .on_with_followup(
            "evaluateJSAsync",
            load_fixture("eval_immediate_response.json"),
            load_fixture("eval_result_network_perf_fallback.json"),
        );

    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.push("network".to_owned());

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

    // Summary mode: results is an object with total_requests = 2.
    assert!(json["results"].is_object(), "expected summary object");
    assert_eq!(json["results"]["total_requests"], 2);
    assert_eq!(json["meta"]["source"], "performance-api");
}

#[test]
fn network_prints_hint_when_both_sources_empty() {
    // Watcher has no events and Performance API returns an empty array.
    // The command should still succeed but print a hint to stderr.
    let server = MockRdpServer::new()
        .on("listTabs", load_fixture("list_tabs_response.json"))
        .on("getTarget", load_fixture("get_target_response.json"))
        .on("getWatcher", load_fixture("get_watcher_response.json"))
        .on(
            "watchResources",
            load_fixture("watch_resources_response.json"),
        )
        .on(
            "unwatchResources",
            load_fixture("watch_resources_response.json"),
        )
        .on_with_followup(
            "evaluateJSAsync",
            load_fixture("eval_immediate_response.json"),
            load_fixture("eval_result_empty_array.json"),
        );

    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.push("network".to_owned());

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

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("hint:"),
        "expected a hint on stderr when both sources empty, got: {stderr:?}"
    );
    assert!(
        stderr.contains("--follow") || stderr.contains("Navigate"),
        "hint should mention --follow or Navigate, got: {stderr:?}"
    );

    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("stdout must be valid JSON");
    // Summary mode with no entries: total_requests = 0.
    assert_eq!(json["results"]["total_requests"], 0);
}

// ---------------------------------------------------------------------------
// --follow: streaming mode
// ---------------------------------------------------------------------------

#[test]
fn network_follow_streams_request_and_response_events() {
    // --follow uses watchResources then loops until EOF.
    // close_after_followups causes the server to drop the connection after
    // delivering the followup events, which triggers a clean EOF in the
    // follow loop and allows the client to exit.
    let server = MockRdpServer::new()
        .on("listTabs", load_fixture("list_tabs_response.json"))
        .on("getTarget", load_fixture("get_target_response.json"))
        .on("getWatcher", load_fixture("get_watcher_response.json"))
        .on_with_followups(
            "watchResources",
            load_fixture("watch_resources_response.json"),
            vec![
                load_fixture("resources_available_network.json"),
                load_fixture("resources_updated_network.json"),
            ],
        )
        .on(
            "unwatchResources",
            load_fixture("watch_resources_response.json"),
        )
        .close_after_followups();

    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend(["network".to_owned(), "--follow".to_owned()]);

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

    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<&str> = stdout.trim().lines().collect();

    // Expect at least 2 request events (one per resource) plus response events.
    assert!(
        lines.len() >= 2,
        "expected multiple NDJSON lines, got: {stdout}"
    );

    // First lines should be "request" events.
    let first: serde_json::Value = serde_json::from_str(lines[0]).expect("valid JSON on line 0");
    assert_eq!(first["event"], "request");
    assert_eq!(first["method"], "GET");
    assert!(
        first["url"].as_str().is_some_and(|u| !u.is_empty()),
        "url should be present"
    );

    // There should also be "response" events in the output.
    let has_response = lines.iter().any(|line| {
        serde_json::from_str::<serde_json::Value>(line)
            .map(|v| v["event"] == "response")
            .unwrap_or(false)
    });
    assert!(has_response, "expected at least one response event");
}

#[test]
fn network_follow_filter_suppresses_non_matching_requests() {
    let server = MockRdpServer::new()
        .on("listTabs", load_fixture("list_tabs_response.json"))
        .on("getTarget", load_fixture("get_target_response.json"))
        .on("getWatcher", load_fixture("get_watcher_response.json"))
        .on_with_followups(
            "watchResources",
            load_fixture("watch_resources_response.json"),
            vec![
                load_fixture("resources_available_network.json"),
                load_fixture("resources_updated_network.json"),
            ],
        )
        .on(
            "unwatchResources",
            load_fixture("watch_resources_response.json"),
        )
        .close_after_followups();

    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend([
        "network".to_owned(),
        "--follow".to_owned(),
        "--filter".to_owned(),
        "favicon".to_owned(),
    ]);

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

    let stdout = String::from_utf8_lossy(&output.stdout);
    // All output lines must reference the favicon URL only.
    for line in stdout.trim().lines() {
        let v: serde_json::Value = serde_json::from_str(line).expect("valid JSON");
        assert!(
            v["url"].as_str().unwrap_or("").contains("favicon"),
            "unexpected URL in filtered output: {v}"
        );
    }
}
