/// End-to-end tests for daemon-related CLI behaviour.
///
/// These tests cover:
///   - `--no-daemon` bypasses daemon logic and connects directly to Firefox
///     (exercised via the mock RDP server).
///   - `_daemon` is a recognised subcommand even though it is hidden from
///     `--help`; it should fail with a connection error, not an "unrecognised
///     subcommand" error.
///   - `--help` output advertises both `--no-daemon` and `--daemon-timeout`.
use super::support::{MockRdpServer, load_fixture};

fn ff_rdp_bin() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_BIN_EXE_ff-rdp"))
}

/// Build base args that always bypass the daemon and talk to the mock server
/// at `port`.  Using `--no-daemon` is required so the CLI does not attempt to
/// spawn a background process during tests.
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
// --no-daemon flag
// ---------------------------------------------------------------------------

/// With `--no-daemon` the CLI must connect directly to the mock server and
/// succeed just as it would without any daemon infrastructure.
#[test]
fn no_daemon_flag_bypasses_daemon_and_connects_directly() {
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

    handle.join().expect("mock server thread panicked");

    assert!(
        output.status.success(),
        "--no-daemon tabs must succeed; stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Verify the output is valid JSON — the daemon path is not involved.
    let stdout = String::from_utf8_lossy(&output.stdout);
    serde_json::from_str::<serde_json::Value>(stdout.trim())
        .expect("--no-daemon output must be valid JSON");
}

/// `--no-daemon` must be accepted as an early global flag, not just when
/// placed before the subcommand.
#[test]
fn no_daemon_flag_accepted_as_global_flag() {
    let list_tabs_response = load_fixture("list_tabs_response.json");
    let server = MockRdpServer::new().on("listTabs", list_tabs_response);
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    // Place --no-daemon before the subcommand (standard global-flag position).
    let output = std::process::Command::new(ff_rdp_bin())
        .args([
            "--host",
            "127.0.0.1",
            "--port",
            &port.to_string(),
            "--no-daemon",
            "tabs",
        ])
        .output()
        .expect("failed to spawn ff-rdp");

    handle.join().expect("mock server thread panicked");

    assert!(
        output.status.success(),
        "--no-daemon as global flag must succeed; stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

// ---------------------------------------------------------------------------
// _daemon subcommand recognition
// ---------------------------------------------------------------------------

/// `_daemon` is a hidden-but-valid subcommand.  When Firefox is not listening
/// on the specified port the process must fail with a connection error — *not*
/// with "unrecognized subcommand" or similar clap parse errors.
#[test]
fn daemon_subcommand_is_recognised_and_fails_gracefully_without_firefox() {
    // Bind to grab a free port then immediately drop the listener so nothing
    // is listening when the daemon tries to connect.
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind random port");
    let port = listener.local_addr().expect("local_addr").port();
    drop(listener);

    let output = std::process::Command::new(ff_rdp_bin())
        .args([
            "--host",
            "127.0.0.1",
            "--port",
            &port.to_string(),
            "_daemon",
        ])
        .output()
        .expect("failed to spawn ff-rdp");

    // Must fail (cannot connect) — but not because the subcommand is unknown.
    assert!(
        !output.status.success(),
        "_daemon without Firefox must exit non-zero"
    );

    let stderr = String::from_utf8_lossy(&output.stderr);

    // clap emits "error: unrecognized subcommand" for unknown subcommands;
    // we must not see that here.
    assert!(
        !stderr.to_lowercase().contains("unrecognized subcommand"),
        "_daemon must be a recognised subcommand; stderr: {stderr}"
    );
    assert!(
        !stderr.to_lowercase().contains("unknown subcommand"),
        "_daemon must be a recognised subcommand; stderr: {stderr}"
    );
}

// ---------------------------------------------------------------------------
// --help output
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Deferred-warning behaviour (iter-53 task 2)
//
// When the daemon path fails (e.g. corrupt registry, daemon spawn failure,
// timeout waiting for registry) the CLI falls back to a direct connection.
// The diagnostic message is deferred — printed only if the direct fallback
// also fails — so the happy path is silent.  Below tests both branches.
// ---------------------------------------------------------------------------

/// Plant a corrupt `daemon.json` in `<home>/.ff-rdp/` so `find_running_daemon`
/// returns Err on every invocation, which exercises the deferred-warning
/// branch in `resolve_connection_target`.
fn plant_corrupt_registry(home_dir: &std::path::Path) {
    let dir = home_dir.join(".ff-rdp");
    std::fs::create_dir_all(&dir).expect("create .ff-rdp");
    std::fs::write(dir.join("daemon.json"), b"not valid json").expect("write corrupt registry");
}

#[test]
fn registry_not_found_warning_silent_when_direct_fallback_succeeds() {
    let home = tempfile::tempdir().expect("tempdir");
    plant_corrupt_registry(home.path());

    // Use the `eval` command (rather than `tabs`) because it goes through
    // `connect_and_get_target`, which is the path that decides whether to
    // print the deferred warning.  `tabs` connects directly and never sees
    // the daemon resolver.
    let server = MockRdpServer::new()
        .on("listTabs", load_fixture("list_tabs_response.json"))
        .on("getTarget", load_fixture("get_target_response.json"))
        .on_with_followup(
            "evaluateJSAsync",
            load_fixture("eval_immediate_response.json"),
            load_fixture("eval_result_ready_state_complete.json"),
        );
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let output = std::process::Command::new(ff_rdp_bin())
        .env("FF_RDP_HOME", home.path())
        .args([
            "--host",
            "127.0.0.1",
            "--port",
            &port.to_string(),
            "eval",
            "document.readyState",
        ])
        .output()
        .expect("failed to spawn ff-rdp");

    handle.join().expect("mock server thread panicked");

    assert!(
        output.status.success(),
        "expected success when direct fallback works; stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("warning:"),
        "happy path must be silent (no daemon warnings on stderr); got: {stderr}"
    );
}

#[test]
fn registry_not_found_warning_visible_when_direct_fallback_also_fails() {
    let home = tempfile::tempdir().expect("tempdir");
    plant_corrupt_registry(home.path());

    // Bind and immediately drop so nothing is listening on this port.
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    drop(listener);

    let output = std::process::Command::new(ff_rdp_bin())
        .env("FF_RDP_HOME", home.path())
        .args([
            "--host",
            "127.0.0.1",
            "--port",
            &port.to_string(),
            "--timeout",
            "500",
            "eval",
            "1",
        ])
        .output()
        .expect("failed to spawn ff-rdp");

    assert!(
        !output.status.success(),
        "expected failure when both daemon and direct paths break"
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("warning:"),
        "broken path must surface the deferred daemon warning; got: {stderr}"
    );
    assert!(
        stderr.contains("could not connect to Firefox"),
        "broken path must also report the direct connection failure; got: {stderr}"
    );
}

/// The global help text must advertise both `--no-daemon` and
/// `--daemon-timeout` so users can discover them without reading the source.
#[test]
fn help_shows_daemon_flags() {
    let output = std::process::Command::new(ff_rdp_bin())
        .args(["--help"])
        .output()
        .expect("failed to spawn ff-rdp");

    // `--help` exits with code 0.
    assert!(
        output.status.success(),
        "--help must exit 0; stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(
        stdout.contains("--no-daemon"),
        "--help output must contain --no-daemon; got:\n{stdout}"
    );
    assert!(
        stdout.contains("--daemon-timeout"),
        "--help output must contain --daemon-timeout; got:\n{stdout}"
    );
}
