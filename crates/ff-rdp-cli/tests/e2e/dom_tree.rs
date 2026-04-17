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

/// Build a mock server for the `dom tree` command.
///
/// Protocol flow (no selector):
///   listTabs → getTarget → getWalker (inspector) → documentElement (walker)
///   → children (walker, recursive until numChildren == 0)
///
/// Protocol flow (with selector):
///   listTabs → getTarget → getWalker (inspector) → documentElement (walker)
///   → querySelector (walker) → children (walker, recursive until numChildren == 0)
fn dom_tree_server() -> MockRdpServer {
    MockRdpServer::new()
        .on("listTabs", load_fixture("list_tabs_response.json"))
        .on("getTarget", load_fixture("get_target_response.json"))
        .on(
            "getWalker",
            load_fixture("inspector_get_walker_response.json"),
        )
        .on(
            "documentElement",
            load_fixture("dom_walker_document_element_response.json"),
        )
        .on(
            "querySelector",
            load_fixture("dom_walker_query_selector_response.json"),
        )
        .on(
            "children",
            load_fixture("dom_walker_children_response.json"),
        )
}

// ---------------------------------------------------------------------------
// dom tree: basic output (no selector)
// ---------------------------------------------------------------------------

#[test]
fn dom_tree_outputs_json() {
    let server = dom_tree_server();
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend(["dom".to_owned(), "tree".to_owned()]);

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

    // Results should be the root DOM node (HTML element from documentElement fixture).
    let results = &json["results"];
    assert_eq!(
        results["nodeType"], 1,
        "root node should be an element (nodeType 1)"
    );
    assert_eq!(
        results["nodeName"], "HTML",
        "root should be the HTML element"
    );

    // The HTML element has numChildren=2, so children should be populated.
    let children = results["children"]
        .as_array()
        .expect("results should have a children array");
    assert!(!children.is_empty(), "HTML should have child nodes");

    // Children from the fixture are HEAD and BODY.
    let names: Vec<&str> = children
        .iter()
        .filter_map(|c| c["nodeName"].as_str())
        .collect();
    assert!(names.contains(&"HEAD"), "children should include HEAD");
    assert!(names.contains(&"BODY"), "children should include BODY");

    assert_eq!(json["total"], 1);
}

// ---------------------------------------------------------------------------
// dom tree: with CSS selector
// ---------------------------------------------------------------------------

#[test]
fn dom_tree_with_selector() {
    let server = dom_tree_server();
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend(["dom".to_owned(), "tree".to_owned(), "h1".to_owned()]);

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

    // With a selector, the result is the querySelector-matched node (H1 from fixture).
    let results = &json["results"];
    assert_eq!(
        results["nodeName"], "H1",
        "with selector h1, result should be H1 element"
    );
    assert_eq!(
        results["nodeType"], 1,
        "H1 should be an element node (nodeType 1)"
    );

    // Meta should include the selector.
    assert_eq!(
        json["meta"]["selector"], "h1",
        "meta should record the selector"
    );
}

// ---------------------------------------------------------------------------
// dom tree: actor IDs must be stripped from output
// ---------------------------------------------------------------------------

#[test]
fn dom_tree_strips_actor_ids() {
    let server = dom_tree_server();
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend(["dom".to_owned(), "tree".to_owned()]);

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

    // The root node must not expose the actor ID field.
    assert!(
        json["results"].get("actor").is_none(),
        "actor ID should be stripped from root node"
    );

    // Actor IDs must also be absent from child nodes.
    if let Some(children) = json["results"]["children"].as_array() {
        for child in children {
            assert!(
                child.get("actor").is_none(),
                "actor ID should be stripped from child node"
            );
        }
    }
}

// ---------------------------------------------------------------------------
// dom tree: --jq filter
// ---------------------------------------------------------------------------

#[test]
fn dom_tree_with_jq_filter() {
    let server = dom_tree_server();
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend([
        "--jq".to_owned(),
        ".results.nodeName".to_owned(),
        "dom".to_owned(),
        "tree".to_owned(),
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
    assert_eq!(
        stdout.trim(),
        "\"HTML\"",
        "jq should extract the root nodeName"
    );
}
