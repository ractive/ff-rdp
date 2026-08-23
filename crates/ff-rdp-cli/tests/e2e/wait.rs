use super::support::{self, MockRdpServer, load_fixture};

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
        support::output_note(&output)
    );

    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("stdout must be valid JSON");

    assert_eq!(json["results"]["matched"], true);
    assert!(json["results"]["elapsed_ms"].is_number());
}

// ---------------------------------------------------------------------------
// iter-142 Theme F: plain sleep form
// ---------------------------------------------------------------------------

/// AC: `e2e_wait_sleep_form` — `ff-rdp wait --sleep-ms <N>` sleeps for
/// approximately `N` ms and succeeds with NO server listening at all on the
/// target port — proving the sleep form genuinely skips the Firefox
/// connection rather than merely tolerating a fast-failing one.
#[test]
fn e2e_wait_sleep_form() {
    // Discover a port nothing is listening on — no MockRdpServer spawned.
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind ephemeral port");
    let port = listener.local_addr().expect("local_addr").port();
    drop(listener);

    let mut args = base_args(port);
    // base_args includes --no-daemon; irrelevant here since the sleep path
    // never opens a connection at all, daemon or otherwise.
    args.extend(["wait".to_owned(), "--sleep-ms".to_owned(), "50".to_owned()]);

    let started = std::time::Instant::now();
    let output = std::process::Command::new(ff_rdp_bin())
        .args(&args)
        .output()
        .expect("failed to spawn ff-rdp");
    let elapsed = started.elapsed();

    assert!(
        output.status.success(),
        "expected success with no server listening on port {port}, stderr: {}",
        support::output_note(&output)
    );
    assert!(
        elapsed >= std::time::Duration::from_millis(50),
        "must actually sleep for the requested duration, elapsed={elapsed:?}"
    );

    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("stdout must be valid JSON");
    assert_eq!(json["results"]["matched"], true);
    assert_eq!(json["results"]["elapsed_ms"], 50);
}

/// The legacy `--time` alias (the flag name dogfooding session 63 reached
/// for) parses identically to `--sleep-ms`.
#[test]
fn e2e_wait_sleep_form_time_alias() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind ephemeral port");
    let port = listener.local_addr().expect("local_addr").port();
    drop(listener);

    let mut args = base_args(port);
    args.extend(["wait".to_owned(), "--time".to_owned(), "20".to_owned()]);

    let output = std::process::Command::new(ff_rdp_bin())
        .args(&args)
        .output()
        .expect("failed to spawn ff-rdp");

    assert!(
        output.status.success(),
        "--time alias must work like --sleep-ms, stderr: {}",
        support::output_note(&output)
    );
    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("stdout must be valid JSON");
    assert_eq!(json["results"]["matched"], true);
    assert_eq!(json["results"]["elapsed_ms"], 20);
}

/// `wait` with no condition flag at all (the pre-iter-142 dogfooding
/// friction point) must still fail — `--sleep-ms` opts *in* to a delay, it
/// does not become the default.
#[test]
fn wait_requires_a_condition_or_sleep() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind ephemeral port");
    let port = listener.local_addr().expect("local_addr").port();
    drop(listener);

    let mut args = base_args(port);
    args.push("wait".to_owned());

    let output = std::process::Command::new(ff_rdp_bin())
        .args(&args)
        .output()
        .expect("failed to spawn ff-rdp");

    assert!(
        !output.status.success(),
        "wait with no condition/sleep must fail"
    );
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
        support::output_note(&output)
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
        support::output_note(&output)
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
        "stderr should mention the required flags: {stderr} ({})",
        support::output_note(&output)
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

    // iter-141 Theme E: `poll_js_condition`'s JS-exception path is routed
    // through the standard JSON error envelope on stdout — the single
    // emission per the JSON-only output convention — rather than a bare
    // `error: ...` line on stderr.
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value = serde_json::from_str(stdout.trim())
        .unwrap_or_else(|e| panic!("stdout must be a JSON error envelope: {e}\nstdout: {stdout}"));
    assert_eq!(json["error_type"], "User", "got: {json}");
    assert!(
        json["error"].as_str().is_some_and(|s| !s.is_empty()),
        "envelope must carry a non-empty error message: {json}"
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
    assert_eq!(output.status.code(), Some(124));

    // A2: For a selector wait, the message should name the selector, not just
    // say "timed out".  The timeout error is emitted as the JSON error envelope
    // on stdout (iter-98 Theme D removed the duplicate human `error:` stderr
    // line).  Accept either stream to be resilient to message wording.
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let combined = format!("{stderr}{stdout}");
    assert!(
        combined.contains("never-appears")
            || combined.contains("not found")
            || combined.contains("timed out"),
        "output should mention the selector or timeout: stderr={stderr:?} stdout={stdout:?}"
    );
}
