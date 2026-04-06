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
        // Short timeout so the event drain loop exits quickly.
        "--timeout".to_owned(),
        "1000".to_owned(),
    ]
}

fn navigate_server() -> MockRdpServer {
    MockRdpServer::new()
        .on("listTabs", load_fixture("list_tabs_response.json"))
        .on("getTarget", load_fixture("get_target_response.json"))
        .on("navigateTo", load_fixture("navigate_response.json"))
}

fn navigate_with_network_server() -> MockRdpServer {
    // Resource events are sent as followups to navigateTo, simulating Firefox
    // emitting network events triggered by the navigation. They must arrive
    // after the navigateTo response so the drain loop can pick them up (the
    // actor_request for navigateTo discards messages from other actors).
    MockRdpServer::new()
        .on("listTabs", load_fixture("list_tabs_response.json"))
        .on("getTarget", load_fixture("get_target_response.json"))
        .on("getWatcher", load_fixture("get_watcher_response.json"))
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
        .on(
            "unwatchResources",
            load_fixture("watch_resources_response.json"),
        )
}

#[test]
fn navigate_outputs_json_envelope() {
    let server = navigate_server();
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend(["navigate".to_owned(), "https://example.com".to_owned()]);

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

    assert_eq!(json["results"]["navigated"], "https://example.com");
    assert_eq!(json["total"], 1);
}

#[test]
fn navigate_with_jq_extracts_url() {
    let server = navigate_server();
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend([
        "navigate".to_owned(),
        "https://example.com".to_owned(),
        "--jq".to_owned(),
        ".results.navigated".to_owned(),
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
    assert_eq!(stdout.trim(), r#""https://example.com""#);
}

// ---------------------------------------------------------------------------
// --with-network tests
// ---------------------------------------------------------------------------

#[test]
fn navigate_with_network_captures_requests() {
    let server = navigate_with_network_server();
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend([
        "navigate".to_owned(),
        "https://example.com".to_owned(),
        "--with-network".to_owned(),
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

    // The navigated field is present.
    assert_eq!(json["results"]["navigated"], "https://example.com");

    // The network array is present and populated.
    let network = json["results"]["network"]
        .as_array()
        .expect("network is an array");
    assert_eq!(network.len(), 2, "expected 2 network entries");

    // total reflects the number of network entries.
    assert_eq!(json["total"], 2);

    // First entry: main document request.
    assert_eq!(network[0]["method"], "GET");
    assert_eq!(network[0]["url"], "https://example.com/");
    assert_eq!(network[0]["status"], 200);
    assert_eq!(network[0]["is_xhr"], false);

    // Second entry: favicon request.
    assert_eq!(network[1]["method"], "GET");
    assert_eq!(network[1]["url"], "https://example.com/favicon.ico");
    assert_eq!(network[1]["status"], 404);
}

#[test]
fn navigate_with_network_empty_when_no_events() {
    // Server handles the protocol sequence but sends no resource event followups.
    let server = MockRdpServer::new()
        .on("listTabs", load_fixture("list_tabs_response.json"))
        .on("getTarget", load_fixture("get_target_response.json"))
        .on("getWatcher", load_fixture("get_watcher_response.json"))
        .on(
            "watchResources",
            load_fixture("watch_resources_response.json"),
        )
        .on("navigateTo", load_fixture("navigate_response.json"))
        .on(
            "unwatchResources",
            load_fixture("watch_resources_response.json"),
        );

    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend([
        "navigate".to_owned(),
        "https://example.com".to_owned(),
        "--with-network".to_owned(),
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

    assert_eq!(json["results"]["navigated"], "https://example.com");

    let network = json["results"]["network"]
        .as_array()
        .expect("network is an array");
    assert!(network.is_empty(), "expected no network entries");

    assert_eq!(json["total"], 0);
}
