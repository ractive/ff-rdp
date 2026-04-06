mod support;

use support::{MockRdpServer, load_fixture};

/// Full JSON produced by the mock substring actor for the long-string test.
const LONGSTRING_PERF_JSON: &str = r#"[{"name":"https://example.com/app.js","initiatorType":"script","duration":42.5,"transferSize":12345,"encodedBodySize":12000,"decodedBodySize":36000,"startTime":100.0,"responseEnd":142.5,"protocol":"h2"}]"#;

fn cached_network_server() -> MockRdpServer {
    MockRdpServer::new()
        .on("listTabs", load_fixture("list_tabs_response.json"))
        .on("getTarget", load_fixture("get_target_response.json"))
        .on_with_followup(
            "evaluateJSAsync",
            load_fixture("eval_immediate_response.json"),
            load_fixture("eval_result_perf_timing.json"),
        )
}

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
// Happy-path tests
// ---------------------------------------------------------------------------

#[test]
fn network_shows_requests() {
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

    assert_eq!(json["total"], 2);
    let results = json["results"].as_array().expect("results is array");

    // First request: the main document
    assert_eq!(results[0]["method"], "GET");
    assert_eq!(results[0]["url"], "https://example.com/");
    assert_eq!(results[0]["status"], 200);
    assert_eq!(results[0]["is_xhr"], false);

    // Second request: favicon
    assert_eq!(results[1]["method"], "GET");
    assert_eq!(results[1]["url"], "https://example.com/favicon.ico");
    assert_eq!(results[1]["status"], 404);
}

#[test]
fn network_filter_by_url() {
    let server = network_server();
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend([
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

#[test]
fn network_filter_by_method() {
    let server = network_server();
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend([
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

#[test]
fn network_with_jq_filter() {
    let server = network_server();
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend([
        "network".to_owned(),
        "--jq".to_owned(),
        ".results[] | select(.status >= 400)".to_owned(),
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
    assert_eq!(json["total"], 0);
}

// ---------------------------------------------------------------------------
// --cached mode tests (Performance Resource Timing API)
// ---------------------------------------------------------------------------

#[test]
fn network_cached_shows_resources() {
    let server = cached_network_server();
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend(["network".to_owned(), "--cached".to_owned()]);

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

    assert_eq!(results[0]["url"], "https://example.com/app.js");
    assert_eq!(results[0]["initiator_type"], "script");
    assert_eq!(results[0]["duration_ms"], 42.5);
    assert_eq!(results[0]["transfer_size"], 12345);
    assert_eq!(results[0]["decoded_size"], 36000);
    assert_eq!(results[0]["protocol"], "h2");

    assert_eq!(results[1]["url"], "https://example.com/favicon.ico");
    assert_eq!(results[1]["initiator_type"], "img");
}

#[test]
fn network_cached_filter_by_url() {
    let server = cached_network_server();
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend([
        "network".to_owned(),
        "--cached".to_owned(),
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
    assert_eq!(results[0]["initiator_type"], "img");
}

#[test]
fn network_cached_with_jq() {
    let server = cached_network_server();
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend([
        "network".to_owned(),
        "--cached".to_owned(),
        "--jq".to_owned(),
        ".results[] | select(.initiator_type == \"script\") | .url".to_owned(),
    ]);

    let output = std::process::Command::new(ff_rdp_bin())
        .args(&args)
        .output()
        .expect("failed to spawn ff-rdp");

    handle.join().unwrap();

    assert!(output.status.success());

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(stdout.trim(), r#""https://example.com/app.js""#);
}

// ---------------------------------------------------------------------------
// --cached mode error-path tests
// ---------------------------------------------------------------------------

#[test]
fn network_cached_exception_exits_nonzero() {
    let server = MockRdpServer::new()
        .on("listTabs", load_fixture("list_tabs_response.json"))
        .on("getTarget", load_fixture("get_target_response.json"))
        .on_with_followup(
            "evaluateJSAsync",
            load_fixture("eval_immediate_response.json"),
            load_fixture("eval_result_cached_exception.json"),
        );

    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend(["network".to_owned(), "--cached".to_owned()]);

    let output = std::process::Command::new(ff_rdp_bin())
        .args(&args)
        .output()
        .expect("failed to spawn ff-rdp");

    handle.join().unwrap();

    assert!(
        !output.status.success(),
        "expected non-zero exit for exception"
    );
    assert_eq!(output.status.code(), Some(1));

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("performance timing failed"),
        "stderr should mention the error message: {stderr}"
    );
}

#[test]
fn network_cached_handles_long_string() {
    let longstring_result = load_fixture("eval_result_cached_longstring.json");
    let substring_response = serde_json::json!({
        "from": "server1.conn0.longstr1",
        "substring": LONGSTRING_PERF_JSON
    });

    let server = MockRdpServer::new()
        .on("listTabs", load_fixture("list_tabs_response.json"))
        .on("getTarget", load_fixture("get_target_response.json"))
        .on_with_followup(
            "evaluateJSAsync",
            load_fixture("eval_immediate_response.json"),
            longstring_result,
        )
        .on("substring", substring_response);

    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend(["network".to_owned(), "--cached".to_owned()]);

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

    assert_eq!(json["total"], 1);
    let results = json["results"].as_array().expect("results is array");
    assert_eq!(results[0]["url"], "https://example.com/app.js");
    assert_eq!(results[0]["initiator_type"], "script");
    assert_eq!(results[0]["duration_ms"], 42.5);
}
