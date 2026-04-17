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
        "--timeout".to_owned(),
        "5000".to_owned(),
        "--no-daemon".to_owned(),
    ]
}

/// Build the evaluateJSAsync sequence for `n` URLs.
///
/// Each URL requires two `evaluateJSAsync` calls:
///   1. readyState poll → returns `"complete"`
///   2. perf collection → returns the full perf-data JSON string
///
/// The sequence is: [complete, data, complete, data, ...]
fn eval_sequence_for_n_urls(n: usize) -> Vec<(serde_json::Value, Vec<serde_json::Value>)> {
    let immediate = load_fixture("eval_immediate_response.json");
    let ready_state = load_fixture("eval_result_ready_state_complete.json");
    let perf_data = load_fixture("eval_result_perf_compare_data.json");

    let mut entries = Vec::with_capacity(n * 2);
    for _ in 0..n {
        entries.push((immediate.clone(), vec![ready_state.clone()]));
        entries.push((immediate.clone(), vec![perf_data.clone()]));
    }
    entries
}

fn perf_compare_server(n_urls: usize) -> MockRdpServer {
    MockRdpServer::new()
        .on("listTabs", load_fixture("list_tabs_response.json"))
        .on("getTarget", load_fixture("get_target_response.json"))
        .on("navigateTo", load_fixture("navigate_response.json"))
        .on_sequence("evaluateJSAsync", eval_sequence_for_n_urls(n_urls))
}

// ---------------------------------------------------------------------------
// perf compare — two URLs
// ---------------------------------------------------------------------------

#[test]
fn perf_compare_two_urls() {
    let server = perf_compare_server(2);
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend([
        "perf".to_owned(),
        "compare".to_owned(),
        "https://example.com/".to_owned(),
        "https://example.com/other".to_owned(),
    ]);

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

    assert_eq!(json["total"], 2, "should have 2 results");

    let results = json["results"].as_array().expect("results is array");
    assert_eq!(results.len(), 2);

    // First result uses URL as label (no --label given).
    assert_eq!(results[0]["label"], "https://example.com/");
    assert_eq!(results[0]["url"], "https://example.com/");

    // Second result
    assert_eq!(results[1]["label"], "https://example.com/other");
    assert_eq!(results[1]["url"], "https://example.com/other");

    // Vitals section is present and contains expected keys.
    for result in results {
        let vitals = &result["vitals"];
        assert!(
            vitals.is_object(),
            "vitals must be an object for each result"
        );
        assert!(vitals["lcp_ms"].is_number(), "lcp_ms must be present");
        assert!(vitals["fcp_ms"].is_number(), "fcp_ms must be present");
        assert!(vitals["ttfb_ms"].is_number(), "ttfb_ms must be present");
        assert!(vitals["cls"].is_number(), "cls must be present");

        // Navigation section is present.
        let navigation = &result["navigation"];
        assert!(navigation.is_object(), "navigation must be an object");

        // Resources section is present.
        let resources = &result["resources"];
        assert!(resources.is_object(), "resources must be an object");
        assert_eq!(resources["count"], 1, "one resource entry in fixture");
    }
}

// ---------------------------------------------------------------------------
// perf compare — with --label flag
// ---------------------------------------------------------------------------

#[test]
fn perf_compare_with_labels() {
    let server = perf_compare_server(2);
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend([
        "perf".to_owned(),
        "compare".to_owned(),
        "https://example.com/".to_owned(),
        "https://example.com/other".to_owned(),
        "--label".to_owned(),
        "Homepage,OtherPage".to_owned(),
    ]);

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

    let results = json["results"].as_array().expect("results is array");
    assert_eq!(results[0]["label"], "Homepage");
    assert_eq!(results[1]["label"], "OtherPage");
}

// ---------------------------------------------------------------------------
// perf compare — label count mismatch → non-zero exit before connecting
// ---------------------------------------------------------------------------

#[test]
fn perf_compare_label_mismatch_error() {
    // This test does not need a real server because the error is caught before
    // any connection is established.  We still bind a port so the CLI has a
    // valid --port argument and fails for the right reason, not a parse error.
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().unwrap().port();
    // Don't accept — the CLI should exit before connecting.
    drop(listener);

    let mut args = base_args(port);
    args.extend([
        "perf".to_owned(),
        "compare".to_owned(),
        "https://example.com/".to_owned(),
        "https://example.com/other".to_owned(),
        "--label".to_owned(),
        "OnlyOne".to_owned(),
    ]);

    let output = std::process::Command::new(ff_rdp_bin())
        .args(&args)
        .output()
        .expect("failed to spawn ff-rdp");

    assert!(
        !output.status.success(),
        "expected non-zero exit for label mismatch"
    );
    assert_eq!(output.status.code(), Some(1));

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains('1') && stderr.contains('2'),
        "error should mention label count (1) and URL count (2): {stderr}"
    );
}

// ---------------------------------------------------------------------------
// perf compare — --jq filter applied to output
// ---------------------------------------------------------------------------

#[test]
fn perf_compare_with_jq_filter() {
    let server = perf_compare_server(2);
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend([
        "perf".to_owned(),
        "compare".to_owned(),
        "https://example.com/".to_owned(),
        "https://example.com/other".to_owned(),
        "--jq".to_owned(),
        ".results[].label".to_owned(),
    ]);

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
    assert_eq!(lines.len(), 2, "jq should emit one line per result");
    assert_eq!(lines[0], r#""https://example.com/""#);
    assert_eq!(lines[1], r#""https://example.com/other""#);
}

// ---------------------------------------------------------------------------
// perf compare — single URL is rejected (clap enforces num_args = 2..)
// ---------------------------------------------------------------------------

#[test]
fn perf_compare_single_url_error() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().unwrap().port();
    drop(listener);

    let mut args = base_args(port);
    args.extend([
        "perf".to_owned(),
        "compare".to_owned(),
        "https://example.com/".to_owned(),
    ]);

    let output = std::process::Command::new(ff_rdp_bin())
        .args(&args)
        .output()
        .expect("failed to spawn ff-rdp");

    assert!(
        !output.status.success(),
        "expected non-zero exit when only one URL is given"
    );
    // clap exits with code 2 for argument parse errors.
    assert_eq!(output.status.code(), Some(2));
}
