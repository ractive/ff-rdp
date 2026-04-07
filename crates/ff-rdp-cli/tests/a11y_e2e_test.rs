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
        "--no-daemon".to_owned(),
    ]
}

/// Build a mock server that handles the full a11y protocol sequence.
///
/// Protocol flow:
///   listTabs → getTarget → getWalker → getRootNode → children (for nodes with childCount > 0)
fn a11y_server() -> MockRdpServer {
    MockRdpServer::new()
        .on("listTabs", load_fixture("list_tabs_response.json"))
        .on("getTarget", load_fixture("get_target_response.json"))
        .on("getWalker", load_fixture("a11y_get_walker_response.json"))
        .on("getRootNode", load_fixture("a11y_get_root_response.json"))
        .on("children", load_fixture("a11y_children_response.json"))
}

/// Build a mock server for a11y contrast (uses JS eval path like snapshot).
fn a11y_contrast_server() -> MockRdpServer {
    MockRdpServer::new()
        .on("listTabs", load_fixture("list_tabs_response.json"))
        .on("getTarget", load_fixture("get_target_response.json"))
        .on_with_followup(
            "evaluateJSAsync",
            load_fixture("eval_immediate_response.json"),
            load_fixture("eval_result_contrast.json"),
        )
}

// ---------------------------------------------------------------------------
// a11y: basic output
// ---------------------------------------------------------------------------

#[test]
fn a11y_outputs_json_with_accessibility_tree() {
    let server = a11y_server();
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.push("a11y".to_owned());

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

    // Root node should be the document role.
    assert_eq!(
        json["results"]["role"], "document",
        "root role should be document"
    );
    assert_eq!(json["total"], 1);

    // Should have children populated from the children fixture.
    let children = json["results"]["children"]
        .as_array()
        .expect("results should have a children array");
    assert!(!children.is_empty(), "tree should have at least one child");

    // Actor IDs must be stripped from output.
    assert!(
        json["results"].get("actor").is_none(),
        "actor IDs should be stripped from output"
    );
}

// ---------------------------------------------------------------------------
// a11y: interactive filter
// ---------------------------------------------------------------------------

#[test]
fn a11y_interactive_filters_to_interactive_elements() {
    let server = a11y_server();
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend(["a11y".to_owned(), "--interactive".to_owned()]);

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

    // The fixture children include a "link" node which is interactive.
    // The root document is kept because it has an interactive descendant.
    let results = &json["results"];
    assert_eq!(json["total"], 1);

    // Interactive filter should retain only interactive roles in the subtree.
    // The link child must be present; the non-interactive heading must be absent.
    let children = results["children"]
        .as_array()
        .expect("filtered results should still have children");

    let link_present = children.iter().any(|c| c["role"] == "link");
    assert!(link_present, "interactive filter should retain link role");

    let heading_present = children.iter().any(|c| c["role"] == "heading");
    assert!(
        !heading_present,
        "interactive filter should remove heading role"
    );
}

// ---------------------------------------------------------------------------
// a11y: --jq filter
// ---------------------------------------------------------------------------

#[test]
fn a11y_with_jq_extracts_role() {
    let server = a11y_server();
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend([
        "--jq".to_owned(),
        ".results.role".to_owned(),
        "a11y".to_owned(),
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
    assert_eq!(stdout.trim(), "\"document\"");
}

// ---------------------------------------------------------------------------
// a11y contrast: basic output
// ---------------------------------------------------------------------------

#[test]
fn a11y_contrast_outputs_json_with_checks() {
    let server = a11y_contrast_server();
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend(["a11y".to_owned(), "contrast".to_owned()]);

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

    // Results should be an array of contrast check objects.
    let results = json["results"]
        .as_array()
        .expect("contrast results should be an array");

    assert!(
        !results.is_empty(),
        "should have at least one contrast check"
    );

    // Each check should have the expected WCAG fields.
    let first = &results[0];
    assert!(
        first.get("ratio").is_some(),
        "check must have a ratio field"
    );
    assert!(
        first.get("foreground").is_some(),
        "check must have foreground"
    );
    assert!(
        first.get("background").is_some(),
        "check must have background"
    );
    assert!(
        first.get("aa_normal").is_some(),
        "check must have aa_normal"
    );

    // Meta should include summary.
    assert!(
        json["meta"]["summary"].is_object(),
        "meta should contain summary"
    );
    assert!(
        json["meta"]["summary"]["total"].is_number(),
        "summary should have total"
    );
}

// ---------------------------------------------------------------------------
// a11y contrast: --fail-only flag
// ---------------------------------------------------------------------------

#[test]
fn a11y_contrast_fail_only_filters_passing_checks() {
    let server = a11y_contrast_server();
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend([
        "a11y".to_owned(),
        "contrast".to_owned(),
        "--fail-only".to_owned(),
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

    // The fixture has all checks passing (aa_normal: true), so --fail-only should return empty.
    let results = json["results"]
        .as_array()
        .expect("contrast results should be an array");
    assert!(
        results.is_empty(),
        "all fixture checks pass AA — fail-only should return empty array"
    );

    // total reflects the full summary count from the JS result (not the filtered count),
    // so the caller knows how many elements were checked in total.
    assert_eq!(
        json["total"], 2,
        "total reflects all checked elements even when filtered"
    );
}

// ---------------------------------------------------------------------------
// a11y contrast: --jq filter
// ---------------------------------------------------------------------------

#[test]
fn a11y_contrast_with_jq_extracts_total() {
    let server = a11y_contrast_server();
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend([
        "--jq".to_owned(),
        ".meta.summary.total".to_owned(),
        "a11y".to_owned(),
        "contrast".to_owned(),
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
    assert_eq!(stdout.trim(), "2", "fixture has 2 contrast checks");
}
