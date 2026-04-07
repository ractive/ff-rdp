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

/// Build a mock server for the styles command.
///
/// Protocol flow:
///   listTabs → getTarget → getWalker (inspector) → getPageStyle (inspector)
///   → documentElement (walker) → querySelector (walker) → <style_method> (pagestyle)
///
/// `style_method` is the RDP message type (e.g. `"getComputed"`) and
/// `style_fixture` is the fixture filename (e.g. `"page_style_get_computed_response.json"`).
fn styles_server(style_method: &str, style_fixture: &str) -> MockRdpServer {
    MockRdpServer::new()
        .on("listTabs", load_fixture("list_tabs_response.json"))
        .on("getTarget", load_fixture("get_target_response.json"))
        .on(
            "getWalker",
            load_fixture("inspector_get_walker_response.json"),
        )
        .on(
            "getPageStyle",
            load_fixture("inspector_get_page_style_response.json"),
        )
        .on(
            "documentElement",
            load_fixture("dom_walker_document_element_response.json"),
        )
        .on(
            "querySelector",
            load_fixture("dom_walker_query_selector_response.json"),
        )
        .on(style_method, load_fixture(style_fixture))
}

// ---------------------------------------------------------------------------
// styles: computed (default)
// ---------------------------------------------------------------------------

#[test]
fn styles_computed_outputs_json() {
    let server = styles_server("getComputed", "page_style_get_computed_response.json");
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend(["styles".to_owned(), "h1".to_owned()]);

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

    // Results should be an array of computed property objects.
    let results = json["results"]
        .as_array()
        .expect("computed results should be an array");

    assert!(!results.is_empty(), "should have at least one property");

    // Each entry must have name, value, priority.
    let first = &results[0];
    assert!(first.get("name").is_some(), "entry must have name");
    assert!(first.get("value").is_some(), "entry must have value");
    assert!(first.get("priority").is_some(), "entry must have priority");

    // The fixture has color, display, font-size, margin-top — sorted alphabetically.
    let names: Vec<&str> = results.iter().filter_map(|e| e["name"].as_str()).collect();
    assert!(names.contains(&"color"), "should include color property");
    assert!(
        names.contains(&"font-size"),
        "should include font-size property"
    );

    // Verify meta.
    assert_eq!(json["meta"]["selector"], "h1");
}

// ---------------------------------------------------------------------------
// styles: applied rules (--applied)
// ---------------------------------------------------------------------------

#[test]
fn styles_applied_outputs_json() {
    let server = styles_server("getApplied", "page_style_get_applied_response.json");
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend(["styles".to_owned(), "h1".to_owned(), "--applied".to_owned()]);

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

    // Results should be an array of applied rule objects.
    let results = json["results"]
        .as_array()
        .expect("applied results should be an array");

    assert!(!results.is_empty(), "should have at least one applied rule");

    // Each rule must have selector, source, line, properties.
    let first = &results[0];
    assert!(first.get("selector").is_some(), "rule must have selector");
    assert!(
        first.get("properties").is_some(),
        "rule must have properties"
    );

    // The fixture has two rules: "h1" and "h1, .title".
    let selectors: Vec<&str> = results
        .iter()
        .filter_map(|r| r["selector"].as_str())
        .collect();
    assert!(selectors.contains(&"h1"), "should include h1 selector");
    assert!(
        selectors.contains(&"h1, .title"),
        "should include combined selector"
    );

    // Properties inside each rule must have name, value, priority.
    let props = first["properties"]
        .as_array()
        .expect("properties should be an array");
    assert!(!props.is_empty(), "rule should have declarations");
    assert!(props[0].get("name").is_some(), "property must have name");
    assert!(props[0].get("value").is_some(), "property must have value");
}

// ---------------------------------------------------------------------------
// styles: box model layout (--layout)
// ---------------------------------------------------------------------------

#[test]
fn styles_layout_outputs_json() {
    let server = styles_server("getLayout", "page_style_get_layout_response.json");
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend(["styles".to_owned(), "h1".to_owned(), "--layout".to_owned()]);

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

    // Top-level dimensions.
    assert_eq!(results["width"], 784.0, "width should match fixture");
    assert_eq!(results["height"], 37.0, "height should match fixture");

    // Box model sides.
    assert!(results.get("margin").is_some(), "should have margin");
    assert!(results.get("border").is_some(), "should have border");
    assert!(results.get("padding").is_some(), "should have padding");

    // Margin top matches fixture (21.44).
    let margin_top = results["margin"]["top"]
        .as_f64()
        .expect("margin.top should be a number");
    assert!(
        (margin_top - 21.44).abs() < 0.01,
        "margin.top should be ~21.44, got {margin_top}"
    );

    // Box model metadata fields.
    assert_eq!(results["boxSizing"], "content-box");
    assert_eq!(results["position"], "static");
    assert_eq!(results["display"], "block");
}

// ---------------------------------------------------------------------------
// styles: --jq filter
// ---------------------------------------------------------------------------

#[test]
fn styles_with_jq_filter() {
    let server = styles_server("getComputed", "page_style_get_computed_response.json");
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend([
        "--jq".to_owned(),
        ".results[0].name".to_owned(),
        "styles".to_owned(),
        "h1".to_owned(),
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

    // Properties are sorted alphabetically; "color" should come before "display", "font-size".
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(
        stdout.trim(),
        "\"color\"",
        "first sorted property should be color"
    );
}
