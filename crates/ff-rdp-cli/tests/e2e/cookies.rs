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

fn cookies_server(store_objects_fixture: &str) -> MockRdpServer {
    MockRdpServer::new()
        .on("listTabs", load_fixture("list_tabs_response.json"))
        .on("getTarget", load_fixture("get_target_response.json"))
        .on("getWatcher", load_fixture("get_watcher_response.json"))
        .on(
            "watchResources",
            load_fixture("watch_resources_cookies_response.json"),
        )
        .on("getStoreObjects", load_fixture(store_objects_fixture))
        .on(
            "unwatchResources",
            load_fixture("unwatch_resources_response.json"),
        )
}

// ---------------------------------------------------------------------------
// Happy path — two cookies returned with full metadata
// ---------------------------------------------------------------------------

#[test]
fn cookies_returns_parsed_array() {
    let server = cookies_server("get_store_objects_cookies_response.json");
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.push("cookies".to_owned());

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

    let results = json["results"]
        .as_array()
        .expect("results should be an array");

    assert_eq!(results.len(), 2, "expected 2 cookies");
    assert_eq!(results[0]["name"], "session_id");
    assert_eq!(results[0]["value"], "abc123");
    assert_eq!(results[0]["isHttpOnly"], true);
    assert_eq!(results[0]["isSecure"], true);
    assert_eq!(results[0]["sameSite"], "Lax");
    assert_eq!(results[0]["expires"], "Session");

    assert_eq!(results[1]["name"], "theme");
    assert_eq!(results[1]["value"], "dark");
    assert_eq!(results[1]["isHttpOnly"], false);
    assert!(results[1]["expires"].is_u64());
    assert_eq!(json["total"], 2);
}

// ---------------------------------------------------------------------------
// Empty cookies
// ---------------------------------------------------------------------------

#[test]
fn cookies_returns_empty_array_when_no_cookies() {
    let server = cookies_server("get_store_objects_cookies_empty_response.json");
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.push("cookies".to_owned());

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

    let results = json["results"]
        .as_array()
        .expect("results should be an array");

    assert!(results.is_empty(), "expected zero cookies");
    assert_eq!(json["total"], 0);
}

// ---------------------------------------------------------------------------
// Filter by --name: matching cookie
// ---------------------------------------------------------------------------

#[test]
fn cookies_filter_by_name_returns_match() {
    let server = cookies_server("get_store_objects_cookies_response.json");
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend([
        "cookies".to_owned(),
        "--name".to_owned(),
        "session_id".to_owned(),
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

    let results = json["results"]
        .as_array()
        .expect("results should be an array");

    assert_eq!(results.len(), 1, "expected exactly one cookie after filter");
    assert_eq!(results[0]["name"], "session_id");
    assert_eq!(results[0]["value"], "abc123");
    assert_eq!(json["total"], 1);
}

// ---------------------------------------------------------------------------
// Filter by --name: no match → empty result
// ---------------------------------------------------------------------------

#[test]
fn cookies_filter_by_name_no_match_returns_empty() {
    let server = cookies_server("get_store_objects_cookies_response.json");
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend([
        "cookies".to_owned(),
        "--name".to_owned(),
        "nonexistent".to_owned(),
    ]);

    let output = std::process::Command::new(ff_rdp_bin())
        .args(&args)
        .output()
        .expect("failed to spawn ff-rdp");

    handle.join().unwrap();

    assert!(
        output.status.success(),
        "expected success even when no cookies match the filter, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("stdout must be valid JSON");

    let results = json["results"]
        .as_array()
        .expect("results should be an array");

    assert!(results.is_empty(), "expected zero cookies after filter");
    assert_eq!(json["total"], 0);
}
