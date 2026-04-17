use super::support::{MockRdpServer, load_fixture};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Return the path to the `ff-rdp` binary built by Cargo.
///
/// `CARGO_BIN_EXE_ff-rdp` is set by Cargo for every `[[bin]]` target declared
/// in the same crate, so this is always the freshly-compiled binary under test.
fn ff_rdp_bin() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_BIN_EXE_ff-rdp"))
}

/// Base args shared by all tests that talk to a local mock server.
///
/// Use an explicit IPv4 loopback address to keep tests deterministic
/// across environments with different localhost resolution (IPv4 vs IPv6).
fn base_args(port: u16) -> Vec<String> {
    vec![
        "--host".to_owned(),
        "127.0.0.1".to_owned(),
        "--port".to_owned(),
        port.to_string(),
        "--no-daemon".to_owned(),
    ]
}

// ---------------------------------------------------------------------------
// Happy-path tests
// ---------------------------------------------------------------------------

/// The CLI must emit a JSON envelope with `results` (array), `total`, and `meta`.
#[test]
fn tabs_outputs_json_envelope() {
    let list_tabs_response = load_fixture("list_tabs_response.json");

    let server = MockRdpServer::new().on("listTabs", list_tabs_response);
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.push("tabs".to_owned());

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

    assert_eq!(json["total"], 2, "total must be 2");
    assert!(json["results"].is_array(), "results must be an array");
    assert_eq!(
        json["results"].as_array().unwrap().len(),
        2,
        "results must contain 2 tabs"
    );
    assert!(json["meta"].is_object(), "meta must be present");
}

/// `--jq '.results[0].url'` must extract the first tab's URL as a JSON string.
#[test]
fn tabs_with_jq_filter_extracts_url() {
    let list_tabs_response = load_fixture("list_tabs_response.json");

    let server = MockRdpServer::new().on("listTabs", list_tabs_response);
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend([
        "tabs".to_owned(),
        "--jq".to_owned(),
        ".results[0].url".to_owned(),
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
    let stdout = stdout.trim();
    // jq outputs a JSON string including the surrounding quotes.
    assert_eq!(stdout, r#""https://example.com/""#);
}

/// `--jq '.results | length'` must output the count of tabs.
#[test]
fn tabs_with_jq_total() {
    let list_tabs_response = load_fixture("list_tabs_response.json");

    let server = MockRdpServer::new().on("listTabs", list_tabs_response);
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend([
        "tabs".to_owned(),
        "--jq".to_owned(),
        ".results | length".to_owned(),
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
    let stdout = stdout.trim();
    assert_eq!(stdout, "2");
}

// ---------------------------------------------------------------------------
// Error-path tests
// ---------------------------------------------------------------------------

/// Connecting to a port with nothing listening must exit 1 and print a
/// user-friendly message that mentions whether Firefox is running.
#[test]
fn tabs_connection_refused_shows_helpful_error() {
    // Bind a listener to grab a free port, then drop it so nothing accepts.
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);

    let mut args = base_args(port);
    args.push("tabs".to_owned());

    let output = std::process::Command::new(ff_rdp_bin())
        .args(&args)
        .output()
        .expect("failed to spawn ff-rdp");

    assert!(
        !output.status.success(),
        "expected failure, but the process succeeded"
    );
    assert_eq!(output.status.code(), Some(1));

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("is Firefox running"),
        "stderr must mention whether Firefox is running; got: {stderr}"
    );
}

/// When the server accepts but never sends a greeting, the CLI must time out
/// and exit with a non-zero status within the specified `--timeout`.
#[test]
fn tabs_timeout_flag_is_respected() {
    let server = MockRdpServer::new();
    let port = server.port();
    // Spawn the silent variant — accepts the connection but never writes.
    let handle = std::thread::spawn(move || server.serve_one_silent());

    let mut args = base_args(port);
    args.extend(["--timeout".to_owned(), "200".to_owned(), "tabs".to_owned()]);

    let output = std::process::Command::new(ff_rdp_bin())
        .args(&args)
        .output()
        .expect("failed to spawn ff-rdp");

    // The server thread will be holding the connection; we don't need its
    // result — it will eventually be cleaned up by the OS.
    drop(handle);

    assert!(
        !output.status.success(),
        "expected failure due to timeout, but the process succeeded"
    );

    // Any non-zero exit with some diagnostic output is acceptable.
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        !stderr.is_empty() || !stdout.is_empty(),
        "expected some output on timeout"
    );
}
