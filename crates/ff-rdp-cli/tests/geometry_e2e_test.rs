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

fn geometry_server(eval_result_fixture: &str) -> MockRdpServer {
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
// Basic geometry — single selector returns elements array with viewport
// ---------------------------------------------------------------------------

#[test]
fn geometry_single_selector() {
    let server = geometry_server("eval_result_geometry.json");
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend(["geometry".to_owned(), "h1".to_owned(), "p".to_owned()]);

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

    // Envelope structure
    assert_eq!(json["total"], 2, "total should reflect element count");
    assert!(json["results"]["viewport"].is_object());
    assert_eq!(json["results"]["viewport"]["width"], 1024);
    assert_eq!(json["results"]["viewport"]["height"], 768);

    // Elements array
    let elements = json["results"]["elements"]
        .as_array()
        .expect("elements must be an array");
    assert_eq!(elements.len(), 2);

    // First element (h1)
    assert_eq!(elements[0]["selector"], "h1");
    assert_eq!(elements[0]["index"], 0);
    assert_eq!(elements[0]["tag"], "h1");
    assert_eq!(elements[0]["rect"]["width"], 400);
    assert_eq!(elements[0]["rect"]["height"], 40);
    assert_eq!(elements[0]["computed"]["position"], "static");
    assert_eq!(elements[0]["computed"]["z_index"], "auto");
    assert_eq!(elements[0]["visible"], true);
    assert_eq!(elements[0]["in_viewport"], true);

    // Second element (p)
    assert_eq!(elements[1]["selector"], "p");
    assert_eq!(elements[1]["tag"], "p");

    // No overlaps
    let overlaps = json["results"]["overlaps"]
        .as_array()
        .expect("overlaps must be an array");
    assert!(overlaps.is_empty());
}

// ---------------------------------------------------------------------------
// Multiple selectors in one call
// ---------------------------------------------------------------------------

#[test]
fn geometry_multiple_selectors() {
    let server = geometry_server("eval_result_geometry.json");
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend(["geometry".to_owned(), "h1".to_owned(), "p".to_owned()]);

    let output = std::process::Command::new(ff_rdp_bin())
        .args(&args)
        .output()
        .expect("failed to spawn ff-rdp");

    handle.join().unwrap();

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    // meta should contain both selectors
    let meta_selectors = json["meta"]["selectors"]
        .as_array()
        .expect("meta.selectors must be an array");
    assert_eq!(meta_selectors.len(), 2);
    assert_eq!(meta_selectors[0], "h1");
    assert_eq!(meta_selectors[1], "p");
}

// ---------------------------------------------------------------------------
// Overlap detection
// ---------------------------------------------------------------------------

#[test]
fn geometry_overlap_detection() {
    let server = geometry_server("eval_result_geometry_overlap.json");
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend([
        "geometry".to_owned(),
        ".box-a".to_owned(),
        ".box-b".to_owned(),
    ]);

    let output = std::process::Command::new(ff_rdp_bin())
        .args(&args)
        .output()
        .expect("failed to spawn ff-rdp");

    handle.join().unwrap();

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();

    let overlaps = json["results"]["overlaps"]
        .as_array()
        .expect("overlaps must be an array");
    assert_eq!(overlaps.len(), 1, "expected one overlapping pair");

    let pair = overlaps[0]
        .as_array()
        .expect("overlap entry must be an array");
    assert_eq!(pair.len(), 2);
    assert_eq!(pair[0], ".box-a[0]");
    assert_eq!(pair[1], ".box-b[0]");
}

// ---------------------------------------------------------------------------
// Null result — no elements matched
// ---------------------------------------------------------------------------

#[test]
fn geometry_null_result() {
    let server = geometry_server("eval_result_dom_null.json");
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend(["geometry".to_owned(), ".nonexistent".to_owned()]);

    let output = std::process::Command::new(ff_rdp_bin())
        .args(&args)
        .output()
        .expect("failed to spawn ff-rdp");

    handle.join().unwrap();

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["total"], 0);
    let elements = json["results"]["elements"]
        .as_array()
        .expect("elements must be an array");
    assert!(elements.is_empty());
}

// ---------------------------------------------------------------------------
// --jq filter
// ---------------------------------------------------------------------------

#[test]
fn geometry_with_jq_filter() {
    let server = geometry_server("eval_result_geometry.json");
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend([
        "--jq".to_owned(),
        ".results.viewport.width".to_owned(),
        "geometry".to_owned(),
        "h1".to_owned(),
    ]);

    let output = std::process::Command::new(ff_rdp_bin())
        .args(&args)
        .output()
        .expect("failed to spawn ff-rdp");

    handle.join().unwrap();

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(stdout.trim(), "1024");
}

// ---------------------------------------------------------------------------
// --limit truncates elements array
// ---------------------------------------------------------------------------

#[test]
fn geometry_limit_truncates_elements() {
    let server = geometry_server("eval_result_geometry.json");
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend([
        "--limit".to_owned(),
        "1".to_owned(),
        "geometry".to_owned(),
        "h1".to_owned(),
        "p".to_owned(),
    ]);

    let output = std::process::Command::new(ff_rdp_bin())
        .args(&args)
        .output()
        .expect("failed to spawn ff-rdp");

    handle.join().unwrap();

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    // 2 total elements, only 1 shown
    assert_eq!(json["total"], 2);
    assert_eq!(json["results"]["elements"].as_array().unwrap().len(), 1);
    assert_eq!(json["truncated"], true);
    assert!(json["hint"].as_str().unwrap().contains("--all"));
}
