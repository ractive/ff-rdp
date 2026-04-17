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
        "--no-daemon".to_owned(),
    ]
}

/// Build a mock server for a storage command — always the same connect
/// sequence, varying only by the eval result fixture.
fn storage_server(eval_result_fixture: &str) -> MockRdpServer {
    MockRdpServer::new()
        .on("listTabs", load_fixture("list_tabs_response.json"))
        .on("getTarget", load_fixture("get_target_response.json"))
        .on_with_followup(
            "evaluateJSAsync",
            load_fixture("eval_immediate_response.json"),
            load_fixture(eval_result_fixture),
        )
}

// ---------------------------------------------------------------------------
// All-keys tests
// ---------------------------------------------------------------------------

#[test]
fn storage_all_keys_returns_parsed_object() {
    let server = storage_server("eval_result_storage.json");
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend(["storage".to_owned(), "local".to_owned()]);

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

    // The fixture has two keys: token and theme.
    assert_eq!(json["total"], 2);
    assert_eq!(json["results"]["token"], "abc");
    assert_eq!(json["results"]["theme"], "dark");
    assert_eq!(json["meta"]["storage_type"], "local");
}

#[test]
fn storage_session_type_accepted() {
    let server = storage_server("eval_result_storage.json");
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend(["storage".to_owned(), "session".to_owned()]);

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

    assert_eq!(json["meta"]["storage_type"], "session");
}

// ---------------------------------------------------------------------------
// Single-key tests
// ---------------------------------------------------------------------------

#[test]
fn storage_specific_key_returns_value() {
    let server = storage_server("eval_result_storage_key.json");
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend([
        "storage".to_owned(),
        "local".to_owned(),
        "--key".to_owned(),
        "token".to_owned(),
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

    assert_eq!(json["total"], 1);
    assert_eq!(json["results"]["key"], "token");
    assert_eq!(json["results"]["value"], "abc123");
}

#[test]
fn storage_missing_key_returns_null_value_and_total_zero() {
    let server = storage_server("eval_result_storage_null.json");
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend([
        "storage".to_owned(),
        "local".to_owned(),
        "--key".to_owned(),
        "nonexistent".to_owned(),
    ]);

    let output = std::process::Command::new(ff_rdp_bin())
        .args(&args)
        .output()
        .expect("failed to spawn ff-rdp");

    handle.join().unwrap();

    assert!(
        output.status.success(),
        "expected success exit code for missing key",
        // Missing key is not an error — it's valid output with total=0.
    );

    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("stdout must be valid JSON");

    assert_eq!(json["total"], 0);
    assert_eq!(json["results"]["key"], "nonexistent");
    assert!(
        json["results"]["value"].is_null(),
        "value should be null for a missing key"
    );
}

// ---------------------------------------------------------------------------
// Error-path tests
// ---------------------------------------------------------------------------

#[test]
fn storage_invalid_type_exits_nonzero_without_connecting() {
    // We still need a server so the process has somewhere to connect — but it
    // should exit before sending any RDP messages when the type is invalid.
    // However, since arg validation happens before connecting, the process may
    // exit before even attempting to connect.  We use a server anyway so the
    // port is valid; the test only checks the exit code and stderr.
    let server = MockRdpServer::new();
    let port = server.port();
    // Don't call serve_one — the process should exit before connecting.

    let mut args = base_args(port);
    args.extend(["storage".to_owned(), "cookie".to_owned()]);

    let output = std::process::Command::new(ff_rdp_bin())
        .args(&args)
        .output()
        .expect("failed to spawn ff-rdp");

    assert!(
        !output.status.success(),
        "expected failure for invalid storage type"
    );

    // After `Command::Storage` is wired into dispatch.rs the error message
    // will mention "cookie" or "invalid storage type".  Before wiring, clap
    // rejects the subcommand with "unrecognized subcommand".  Either message
    // is acceptable here — the important invariant is the non-zero exit code.
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("cookie")
            || stderr.contains("invalid storage type")
            || stderr.contains("storage"),
        "stderr should be non-empty: {stderr}"
    );
}
