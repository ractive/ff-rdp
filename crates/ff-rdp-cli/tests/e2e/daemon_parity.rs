/// End-to-end tests verifying that commands produce identical results when run
/// through the daemon vs direct connection.
///
/// These tests start a real daemon process connected to a mock RDP server,
/// then run CLI commands through the daemon and verify output parity.
///
/// Each test creates an isolated HOME directory so the daemon registry never
/// touches `~/.ff-rdp/daemon.json`.  Tests are still serialized via a
/// process-wide mutex because the mock server binds a port and the daemon
/// is a child process.
use std::process::{Child, Command};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use super::support::{MockRdpServer, load_fixture};

// Serialize all daemon tests to avoid port/process conflicts.
fn daemon_test_mutex() -> &'static Mutex<()> {
    static MUTEX: OnceLock<Mutex<()>> = OnceLock::new();
    MUTEX.get_or_init(|| Mutex::new(()))
}

fn ff_rdp_bin() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_BIN_EXE_ff-rdp"))
}

// ---------------------------------------------------------------------------
// RAII daemon guard — kills the daemon and cleans up the registry on drop.
// ---------------------------------------------------------------------------

struct DaemonGuard {
    child: Option<Child>,
    home_dir: Option<std::path::PathBuf>,
}

impl DaemonGuard {
    fn new(child: Child, home_dir: std::path::PathBuf) -> Self {
        Self {
            child: Some(child),
            home_dir: Some(home_dir),
        }
    }

    fn kill(&mut self) {
        if let Some(mut c) = self.child.take() {
            let _ = c.kill();
            let _ = c.wait();
        }
        if let Some(ref home) = self.home_dir {
            let _ = std::fs::remove_file(home.join(".ff-rdp/daemon.json"));
        }
    }
}

impl Drop for DaemonGuard {
    fn drop(&mut self) {
        self.kill();
    }
}

// ---------------------------------------------------------------------------
// Helper: create an isolated HOME directory for this test.
// ---------------------------------------------------------------------------

fn isolated_home() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("creating temp dir for isolated HOME");
    std::fs::create_dir_all(dir.path().join(".ff-rdp")).expect("creating .ff-rdp in temp HOME");
    dir
}

// ---------------------------------------------------------------------------
// Helper: start the daemon connected to a mock server port.
// ---------------------------------------------------------------------------

fn start_daemon(mock_port: u16, home_dir: &std::path::Path) -> DaemonGuard {
    let child = Command::new(ff_rdp_bin())
        .env("FF_RDP_HOME", home_dir)
        .args([
            "--host",
            "127.0.0.1",
            "--port",
            &mock_port.to_string(),
            "_daemon",
        ])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("failed to spawn daemon process");
    DaemonGuard::new(child, home_dir.to_owned())
}

// ---------------------------------------------------------------------------
// Helper: wait for the daemon registry to appear for the given Firefox port.
// Returns the proxy_port the daemon is listening on.
// ---------------------------------------------------------------------------

fn wait_for_daemon_ready(mock_port: u16, timeout: Duration, home_dir: &std::path::Path) -> u16 {
    let start = Instant::now();
    loop {
        assert!(
            start.elapsed() <= timeout,
            "daemon did not become ready within {timeout:?}"
        );

        let registry_path = home_dir.join(".ff-rdp/daemon.json");

        if let Ok(contents) = std::fs::read_to_string(&registry_path)
            && let Ok(info) = serde_json::from_str::<serde_json::Value>(&contents)
            && info["firefox_port"].as_u64() == Some(u64::from(mock_port))
            && let Some(proxy_port) = info["proxy_port"].as_u64()
        {
            return u16::try_from(proxy_port).expect("proxy_port fits in u16");
        }

        std::thread::sleep(Duration::from_millis(50));
    }
}

// ---------------------------------------------------------------------------
// Helper: build CLI args that route through the daemon (no --no-daemon flag).
// --timeout is short so the event drain loop exits quickly.
// ---------------------------------------------------------------------------

fn daemon_args(mock_port: u16) -> Vec<String> {
    vec![
        "--host".to_owned(),
        "127.0.0.1".to_owned(),
        "--port".to_owned(),
        mock_port.to_string(),
        "--timeout".to_owned(),
        "2000".to_owned(),
    ]
}

// ---------------------------------------------------------------------------
// Mock server: daemon startup + navigate --with-network
//
// Message flow (single TCP connection from daemon):
//   Daemon startup: listTabs, getWatcher, watchResources (no followups)
//   CLI via daemon: listTabs (forwarded), getTarget (forwarded)
//   CLI daemon-local: stream, stop-stream (handled by daemon, not forwarded)
//   CLI via daemon: navigateTo (forwarded) + followup watcher events streamed to CLI
// ---------------------------------------------------------------------------

fn navigate_with_network_daemon_server() -> MockRdpServer {
    MockRdpServer::new()
        // Both daemon startup and CLI-forwarded call use the same Fixed handler.
        .on("listTabs", load_fixture("list_tabs_response.json"))
        // getTarget and startListeners are no longer called by the daemon
        // (Theme B removed the startListeners engagement); kept as handlers
        // so the mock server can respond if any legacy path still requests them.
        .on("getTarget", load_fixture("get_target_response.json"))
        .on(
            "startListeners",
            load_fixture("start_listeners_response.json"),
        )
        .on("getWatcher", load_fixture("get_watcher_response.json"))
        // Daemon startup calls watchTargets("frame") before watchResources.
        .on("watchTargets", load_fixture("watch_targets_response.json"))
        // Daemon startup watchResources has no followups; network events arrive
        // as followups to navigateTo because the daemon streams them in real-time.
        .on(
            "watchResources",
            load_fixture("watch_resources_response.json"),
        )
        .on_with_followups(
            "navigateTo",
            load_fixture("navigate_response.json"),
            vec![
                load_fixture("resources_available_network.json"),
                load_fixture("resources_updated_network.json"),
            ],
        )
}

// ---------------------------------------------------------------------------
// Mock server: daemon startup + network command (drain from buffer)
//
// Message flow:
//   Daemon startup: listTabs, getWatcher, watchResources + followup events
//   (daemon buffers the network events)
//   CLI via daemon: listTabs (forwarded), getTarget (forwarded)
//   CLI daemon-local: drain (handled by daemon, not forwarded)
// ---------------------------------------------------------------------------

fn network_daemon_server() -> MockRdpServer {
    MockRdpServer::new()
        .on("listTabs", load_fixture("list_tabs_response.json"))
        // getTarget and startListeners are no longer called by the daemon
        // (Theme B removed the startListeners engagement); kept as handlers
        // so the mock server can respond if any legacy path still requests them.
        .on("getTarget", load_fixture("get_target_response.json"))
        .on(
            "startListeners",
            load_fixture("start_listeners_response.json"),
        )
        .on("getWatcher", load_fixture("get_watcher_response.json"))
        // Daemon startup calls watchTargets("frame") before watchResources.
        .on("watchTargets", load_fixture("watch_targets_response.json"))
        // watchResources is called at daemon startup. The followups simulate
        // network events that the daemon buffers for later drain by the CLI.
        .on_with_followups(
            "watchResources",
            load_fixture("watch_resources_response.json"),
            vec![
                load_fixture("resources_available_network.json"),
                load_fixture("resources_updated_network.json"),
            ],
        )
}

// ---------------------------------------------------------------------------
// navigate --with-network through daemon
// ---------------------------------------------------------------------------

#[test]
fn daemon_navigate_with_network_captures_requests() {
    let _guard = daemon_test_mutex().lock().expect("daemon test mutex");
    let home = isolated_home();

    let server = navigate_with_network_daemon_server();
    let mock_port = server.port();
    // The mock thread will block in serve_one() until the daemon disconnects
    // (i.e., until DaemonGuard::kill() is called below).
    let mock_handle = std::thread::spawn(move || server.serve_one());

    let mut daemon = start_daemon(mock_port, home.path());
    let _proxy_port = wait_for_daemon_ready(mock_port, Duration::from_secs(5), home.path());

    let mut args = daemon_args(mock_port);
    args.extend([
        "navigate".to_owned(),
        "https://example.com".to_owned(),
        "--with-network".to_owned(),
    ]);

    let output = Command::new(ff_rdp_bin())
        .env("FF_RDP_HOME", home.path())
        .args(&args)
        .output()
        .expect("failed to spawn ff-rdp");

    // Kill daemon before asserting so cleanup always happens, even on panic.
    daemon.kill();
    // Mock thread unblocks once the daemon TCP connection drops.
    let _ = mock_handle.join();

    assert!(
        output.status.success(),
        "daemon navigate --with-network must succeed; stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("stdout must be valid JSON");

    assert_eq!(
        json["results"]["navigated"], "https://example.com",
        "navigated URL must be present in results"
    );

    // Default mode returns a summary object with the same fields as --no-daemon.
    let network = &json["results"]["network"];
    assert!(network.is_object(), "network should be a summary object");
    assert_eq!(
        network["total_requests"], 2,
        "expected 2 network entries through daemon; got: {network}"
    );
    assert!(
        network["total_transfer_bytes"].is_number(),
        "total_transfer_bytes must be a number"
    );
    assert!(
        network["by_cause_type"].is_object(),
        "by_cause_type must be an object"
    );
    assert!(network["slowest"].is_array(), "slowest must be an array");
}

// ---------------------------------------------------------------------------
// network command through daemon (drain from daemon buffer)
// ---------------------------------------------------------------------------

#[test]
fn daemon_network_shows_summary() {
    let _guard = daemon_test_mutex().lock().expect("daemon test mutex");
    let home = isolated_home();

    let server = network_daemon_server();
    let mock_port = server.port();
    let mock_handle = std::thread::spawn(move || server.serve_one());

    let mut daemon = start_daemon(mock_port, home.path());
    let _proxy_port = wait_for_daemon_ready(mock_port, Duration::from_secs(5), home.path());

    // Poll the daemon until it has buffered events, instead of a fixed sleep.
    // The daemon's Firefox-reader thread processes watchResources followups
    // asynchronously; poll with a short interval and a reasonable timeout.
    let poll_timeout = Duration::from_secs(5);
    let poll_start = Instant::now();
    let (json, stderr) = loop {
        let mut args = daemon_args(mock_port);
        args.push("network".to_owned());

        let output = Command::new(ff_rdp_bin())
            .env("FF_RDP_HOME", home.path())
            .args(&args)
            .output()
            .expect("failed to spawn ff-rdp");

        if output.status.success()
            && let Ok(parsed) = serde_json::from_slice::<serde_json::Value>(&output.stdout)
            && parsed["results"]["total_requests"].as_u64().unwrap_or(0) > 0
        {
            break (parsed, String::from_utf8_lossy(&output.stderr).to_string());
        }

        assert!(
            poll_start.elapsed() < poll_timeout,
            "daemon did not buffer events within {poll_timeout:?}; stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        std::thread::sleep(Duration::from_millis(50));
    };

    daemon.kill();
    let _ = mock_handle.join();

    // Summary mode: results is an object matching the --no-daemon output shape.
    assert!(
        json["results"].is_object(),
        "default network output should be summary (object), got: {}; stderr: {stderr}",
        json["results"]
    );
    assert_eq!(
        json["results"]["total_requests"], 2,
        "expected 2 network entries drained from daemon buffer; got: {}",
        json["results"]
    );
    assert!(
        json["results"]["slowest"].is_array(),
        "slowest must be an array"
    );
    assert!(
        json["results"]["by_cause_type"].is_object(),
        "by_cause_type must be an object"
    );
}

// ---------------------------------------------------------------------------
// iter-101 Theme E: error-shape / exit-code parity, daemon vs --no-daemon
// ---------------------------------------------------------------------------

/// Run `ff-rdp <args>` in an isolated HOME and return `(exit_code, error_type)`
/// parsed from the JSON error envelope on stdout.
fn run_and_extract_error(args: &[String], home: &std::path::Path) -> (Option<i32>, String) {
    let output = Command::new(ff_rdp_bin())
        .env("FF_RDP_HOME", home)
        .args(args)
        .output()
        .expect("failed to spawn ff-rdp");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let error_type = serde_json::from_str::<serde_json::Value>(stdout.trim())
        .ok()
        .and_then(|v| {
            v.get("error_type")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
        })
        .unwrap_or_default();
    (output.status.code(), error_type)
}

/// AC: `e2e_error_shape_parity_daemon` — the same failure scenario produces a
/// byte-identical `error_type` and exit code whether it is routed through the
/// daemon (default) or forced direct with `--no-daemon`.
///
/// Scenario covered here: **Firefox gone** (connection refused).  We bind then
/// immediately drop a listener to obtain a port nothing accepts on.  A command
/// that goes through `connect_and_get_target` (`eval`) is then run both ways.
/// In daemon mode the auto-start cannot reach Firefox and falls back to a direct
/// connect, which fails exactly as `--no-daemon` does — so the surfaced error
/// shape must match.
#[test]
fn e2e_error_shape_parity_daemon() {
    let _guard = daemon_test_mutex().lock().expect("daemon test mutex");
    let home = isolated_home();

    // A port with nothing listening — "Firefox gone".
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind for free port");
    let dead_port = listener.local_addr().expect("local_addr").port();
    drop(listener);

    let common = |extra: &[&str]| -> Vec<String> {
        let mut v = vec![
            "--host".to_owned(),
            "127.0.0.1".to_owned(),
            "--port".to_owned(),
            dead_port.to_string(),
            // Keep the daemon-autostart / connect attempt short so the fallback
            // path is reached quickly.
            "--timeout".to_owned(),
            "1500".to_owned(),
        ];
        v.extend(extra.iter().map(|s| (*s).to_owned()));
        v.push("eval".to_owned());
        v.push("1 + 1".to_owned());
        v
    };

    // Direct (`--no-daemon`) run.
    let (direct_code, direct_type) = run_and_extract_error(&common(&["--no-daemon"]), home.path());

    // Daemon-mode run (no `--no-daemon`): auto-start fails against the dead
    // port and falls back to direct.
    let (daemon_code, daemon_type) = run_and_extract_error(&common(&[]), home.path());

    // Both must be a non-zero connection failure.
    assert_eq!(
        direct_code,
        Some(3),
        "direct connection-refused must exit 3; got {direct_code:?}"
    );
    assert_eq!(
        direct_type, "Connection",
        "direct connection-refused error_type must be Connection"
    );

    // Parity: identical error_type and exit code across both modes.
    assert_eq!(
        daemon_code, direct_code,
        "exit code must be identical daemon vs --no-daemon (daemon={daemon_code:?}, direct={direct_code:?})"
    );
    assert_eq!(
        daemon_type, direct_type,
        "error_type must be identical daemon vs --no-daemon (daemon={daemon_type:?}, direct={direct_type:?})"
    );
}
