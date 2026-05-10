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

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("could not connect"),
        "stderr must describe the connection failure; got: {stderr}"
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

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("timed out"),
        "stderr must mention the timeout; got: {stderr}"
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
