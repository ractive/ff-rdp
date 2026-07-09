/// Exit code contract tests — assert the documented exit codes are honoured.
///
/// EXIT CODES (from --help):
///   0   success
///   1   runtime / user error (selector not found, invalid args passed through, etc.)
///   2   usage error (clap parse failure — unknown flag, missing subcommand)
///   3   connection failure (could not reach Firefox or daemon)
///   124 timeout (operation exceeded its deadline)
use super::support::{MockRdpServer, load_fixture};

fn ff_rdp_bin() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_BIN_EXE_ff-rdp"))
}

/// Base args that bypass the daemon and talk to a known port.
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
// Exit 0 — happy path
// ---------------------------------------------------------------------------

#[test]
fn exit_0_happy_path_tabs() {
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

    assert_eq!(
        output.status.code(),
        Some(0),
        "happy path must exit 0; stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

// ---------------------------------------------------------------------------
// Exit 2 — usage/argument error (clap parse failure)
// ---------------------------------------------------------------------------

#[test]
fn exit_2_unknown_flag() {
    let output = std::process::Command::new(ff_rdp_bin())
        .args(["--bogus-unknown-flag-that-does-not-exist"])
        .output()
        .expect("failed to spawn ff-rdp");

    assert_eq!(
        output.status.code(),
        Some(2),
        "unknown flag must exit 2; stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn exit_2_missing_subcommand() {
    // Running with no subcommand should exit 2 (clap usage error).
    let output = std::process::Command::new(ff_rdp_bin())
        .output()
        .expect("failed to spawn ff-rdp");

    // Clap exits 2 on missing subcommand.
    assert_eq!(
        output.status.code(),
        Some(2),
        "missing subcommand must exit 2; stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

// ---------------------------------------------------------------------------
// Exit 3 — connection failure
// ---------------------------------------------------------------------------

/// Connecting to a port that has nothing listening must exit 3 and print a
/// helpful message.
#[test]
fn exit_3_connection_refused_tabs() {
    // Bind a listener to get a free port, then drop it so nothing accepts.
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);

    let mut args = base_args(port);
    args.push("tabs".to_owned());

    let output = std::process::Command::new(ff_rdp_bin())
        .args(&args)
        .output()
        .expect("failed to spawn ff-rdp");

    assert_eq!(
        output.status.code(),
        Some(3),
        "connection refused must exit 3; stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // The connection failure is reported as the single JSON error envelope on
    // stdout (iter-98 Theme D removed the duplicate human `error:` stderr line).
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value = serde_json::from_str(stdout.trim())
        .unwrap_or_else(|e| panic!("stdout must be a JSON error envelope: {e}\nstdout: {stdout}"));
    assert!(
        json["error"]
            .as_str()
            .is_some_and(|m| m.contains("could not connect")),
        "JSON envelope must describe the connection failure; got: {json}"
    );
}

/// Same connection-failure scenario reached via a command that goes through
/// `connect_and_get_target` (not just `tabs` which has its own connection path).
#[test]
fn exit_3_connection_refused_eval() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);

    let mut args = base_args(port);
    args.extend(["eval".to_owned(), "1+1".to_owned()]);

    let output = std::process::Command::new(ff_rdp_bin())
        .args(&args)
        .output()
        .expect("failed to spawn ff-rdp");

    assert_eq!(
        output.status.code(),
        Some(3),
        "connection refused in eval must exit 3; stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

// ---------------------------------------------------------------------------
// Exit 124 — timeout
// ---------------------------------------------------------------------------

/// `wait --selector` that never matches must exit 124.
///
/// We drive it against a mock server that returns `false` on every eval poll
/// so the wait condition never fires.
#[test]
fn exit_124_wait_timeout() {
    let server = MockRdpServer::new()
        .on("listTabs", load_fixture("list_tabs_response.json"))
        .on("getTarget", load_fixture("get_target_response.json"))
        .on_with_followup(
            "evaluateJSAsync",
            load_fixture("eval_immediate_response.json"),
            load_fixture("eval_result_wait_false.json"),
        );
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend([
        "wait".to_owned(),
        "--selector".to_owned(),
        ".never-appears".to_owned(),
        "--wait-timeout".to_owned(),
        "150".to_owned(), // Short so the test is fast.
    ]);

    let output = std::process::Command::new(ff_rdp_bin())
        .args(&args)
        .output()
        .expect("failed to spawn ff-rdp");

    handle.join().unwrap();

    assert_eq!(
        output.status.code(),
        Some(124),
        "wait timeout must exit 124; stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // A2: selector-wait timeout now says "not found" instead of "timed out".
    // The message is emitted as the JSON error envelope on stdout (iter-98
    // Theme D removed the duplicate human `error:` stderr line).
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let combined = format!("{stderr}{stdout}");
    assert!(
        combined.contains("never-appears")
            || combined.contains("not found")
            || combined.contains("timed out"),
        "output must mention the timeout or selector; stderr={stderr:?} stdout={stdout:?}"
    );
}

// ---------------------------------------------------------------------------
// Exit 1 — runtime user error
// ---------------------------------------------------------------------------

/// A JS exception thrown during wait exits 1, not 124.
///
/// This distinguishes a *timeout* (124) from an *error that happens fast* (1).
#[test]
fn exit_1_wait_js_exception() {
    let server = MockRdpServer::new()
        .on("listTabs", load_fixture("list_tabs_response.json"))
        .on("getTarget", load_fixture("get_target_response.json"))
        .on_with_followup(
            "evaluateJSAsync",
            load_fixture("eval_immediate_response.json"),
            load_fixture("eval_result_exception.json"),
        );
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

    assert_eq!(
        output.status.code(),
        Some(1),
        "JS exception during wait must exit 1; stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

// ---------------------------------------------------------------------------
// iter-98 Theme D — errors are emitted exactly once (JSON envelope only)
// ---------------------------------------------------------------------------

/// `pre_fix_repro_error_emitted_twice`: a failing command must emit the error
/// exactly once — as the JSON error envelope on stdout — with no duplicate
/// human `error: {msg}` line on stderr.
///
/// Pre-fix, `main.rs` printed both `eprintln!("error: {err}")` and the JSON
/// envelope, so the same message appeared twice. This asserts the envelope is
/// the single emission and stderr carries no `error:` duplicate.
#[test]
fn pre_fix_repro_error_emitted_twice() {
    // Connection-refused is the simplest deterministic failure that routes
    // through the general error arm in `main.rs`.
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);

    let mut args = base_args(port);
    args.push("tabs".to_owned());

    let output = std::process::Command::new(ff_rdp_bin())
        .args(&args)
        .output()
        .expect("failed to spawn ff-rdp");

    assert!(!output.status.success(), "command must fail");

    // stdout: exactly one JSON error envelope carrying the message.
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value = serde_json::from_str(stdout.trim())
        .unwrap_or_else(|e| panic!("stdout must be a JSON error envelope: {e}\nstdout: {stdout}"));
    let envelope_msg = json["error"]
        .as_str()
        .expect("JSON envelope must carry an `error` message");
    assert!(
        !envelope_msg.is_empty(),
        "envelope message must be non-empty"
    );

    // stderr: must NOT carry a duplicate human `error:` line — the envelope on
    // stdout is the single emission.
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("error:"),
        "stderr must not carry a duplicate `error:` line; the JSON envelope is \
         the single emission. stderr: {stderr}"
    );
}
