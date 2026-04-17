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

fn snapshot_server(eval_result_fixture: &str) -> MockRdpServer {
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
// Basic snapshot returns tree structure
// ---------------------------------------------------------------------------

#[test]
fn snapshot_returns_dom_tree() {
    let server = snapshot_server("eval_result_snapshot.json");
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.push("snapshot".to_owned());

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

    let results = &json["results"];
    assert_eq!(results["tag"], "html", "root tag should be html");
    assert_eq!(json["total"], 1);

    // Should have children array
    assert!(
        results["children"].is_array(),
        "root element should have children"
    );

    // Should contain body with nested content
    let children = results["children"].as_array().unwrap();
    let body = children
        .iter()
        .find(|c| c["tag"] == "body")
        .expect("should have a body element");
    assert!(body["children"].is_array(), "body should have children");
}

// ---------------------------------------------------------------------------
// Snapshot with --depth 2 shows truncation markers
// ---------------------------------------------------------------------------

#[test]
fn snapshot_with_depth_shows_truncation() {
    let server = snapshot_server("eval_result_snapshot_shallow.json");
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend(["snapshot".to_owned(), "--depth".to_owned(), "1".to_owned()]);

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

    let results = &json["results"];
    assert_eq!(results["tag"], "html");

    // Children should include truncation markers
    let children = results["children"].as_array().unwrap();
    let truncated_count = children
        .iter()
        .filter(|c| c.get("truncated").is_some())
        .count();
    assert!(
        truncated_count > 0,
        "at depth 2 some nodes should show truncation markers"
    );
}

// ---------------------------------------------------------------------------
// Snapshot with --jq filter
// ---------------------------------------------------------------------------

#[test]
fn snapshot_with_jq_filter() {
    let server = snapshot_server("eval_result_snapshot.json");
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend([
        "--jq".to_owned(),
        ".results.tag".to_owned(),
        "snapshot".to_owned(),
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
    assert_eq!(stdout.trim(), "\"html\"");
}

// ---------------------------------------------------------------------------
// Snapshot with null result (empty page)
// ---------------------------------------------------------------------------

#[test]
fn snapshot_null_result() {
    let server = snapshot_server("eval_result_dom_null.json");
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.push("snapshot".to_owned());

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

    assert!(
        json["results"].is_null(),
        "null eval result should yield null results"
    );
    assert_eq!(json["total"], 0);
}
