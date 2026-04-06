mod support;

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
    ]
}

fn wait_server(eval_result_fixture: &str) -> MockRdpServer {
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
// Happy-path tests
// ---------------------------------------------------------------------------

#[test]
fn wait_selector_succeeds_immediately() {
    let server = wait_server("eval_result_wait_true.json");
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend([
        "wait".to_owned(),
        "--selector".to_owned(),
        ".results".to_owned(),
        "--wait-timeout".to_owned(),
        "5000".to_owned(),
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

    assert_eq!(json["results"]["matched"], true);
    assert!(json["results"]["elapsed_ms"].is_number());
}

#[test]
fn wait_eval_succeeds_immediately() {
    let server = wait_server("eval_result_wait_true.json");
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend([
        "wait".to_owned(),
        "--eval".to_owned(),
        "document.readyState === 'complete'".to_owned(),
        "--wait-timeout".to_owned(),
        "5000".to_owned(),
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

    assert_eq!(json["results"]["matched"], true);
}

#[test]
fn wait_text_succeeds_immediately() {
    let server = wait_server("eval_result_wait_true.json");
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend([
        "wait".to_owned(),
        "--text".to_owned(),
        "Success".to_owned(),
        "--wait-timeout".to_owned(),
        "5000".to_owned(),
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

    assert_eq!(json["results"]["matched"], true);
}

// ---------------------------------------------------------------------------
// Error-path tests
// ---------------------------------------------------------------------------

#[test]
fn wait_no_condition_exits_nonzero() {
    // No mock server needed — clap enforces at least one condition via the
    // "condition" argument group. We still need a port to pass.
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);

    let mut args = base_args(port);
    args.push("wait".to_owned());

    let output = std::process::Command::new(ff_rdp_bin())
        .args(&args)
        .output()
        .expect("failed to spawn ff-rdp");

    assert!(
        !output.status.success(),
        "expected failure when no condition given"
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("selector") || stderr.contains("text") || stderr.contains("eval"),
        "stderr should mention the required flags: {stderr}"
    );
}

#[test]
fn wait_exception_exits_nonzero() {
    // The eval throws an exception — wait should report the error and exit 1.
    let server = wait_server("eval_result_exception.json");
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend([
        "wait".to_owned(),
        "--selector".to_owned(),
        ".never-appears".to_owned(),
        "--wait-timeout".to_owned(),
        "5000".to_owned(),
    ]);

    let output = std::process::Command::new(ff_rdp_bin())
        .args(&args)
        .output()
        .expect("failed to spawn ff-rdp");

    handle.join().unwrap();

    assert!(
        !output.status.success(),
        "expected failure for exception during wait"
    );
    assert_eq!(output.status.code(), Some(1));

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("error"),
        "stderr should contain an error message: {stderr}"
    );
}

#[test]
fn wait_timeout_exits_nonzero() {
    // The eval returns false every poll — wait should time out.
    let server = wait_server("eval_result_wait_false.json");
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend([
        "wait".to_owned(),
        "--selector".to_owned(),
        ".never-appears".to_owned(),
        "--wait-timeout".to_owned(),
        "150".to_owned(), // Short timeout so the test is fast
    ]);

    let output = std::process::Command::new(ff_rdp_bin())
        .args(&args)
        .output()
        .expect("failed to spawn ff-rdp");

    handle.join().unwrap();

    assert!(
        !output.status.success(),
        "expected failure when wait times out"
    );
    assert_eq!(output.status.code(), Some(1));

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("timed out"),
        "stderr should mention timeout: {stderr}"
    );
}
