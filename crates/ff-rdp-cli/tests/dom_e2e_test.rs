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

fn dom_server(eval_result_fixture: &str) -> MockRdpServer {
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
// Single element — default (outerHTML)
// ---------------------------------------------------------------------------

#[test]
fn dom_single_element_outer_html() {
    let server = dom_server("eval_result_dom_single.json");
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend(["dom".to_owned(), "h1".to_owned()]);

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

    assert_eq!(json["results"], "<h1>Example Domain</h1>");
    assert_eq!(json["total"], 1);
    assert_eq!(json["meta"]["selector"], "h1");
}

// ---------------------------------------------------------------------------
// Single element — --text
// ---------------------------------------------------------------------------

#[test]
fn dom_single_element_text() {
    let server = dom_server("eval_result_dom_text.json");
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend(["dom".to_owned(), "h1".to_owned(), "--text".to_owned()]);

    let output = std::process::Command::new(ff_rdp_bin())
        .args(&args)
        .output()
        .expect("failed to spawn ff-rdp");

    handle.join().unwrap();

    assert!(output.status.success());

    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["results"], "Example Domain");
    assert_eq!(json["total"], 1);
}

// ---------------------------------------------------------------------------
// No match — returns null
// ---------------------------------------------------------------------------

#[test]
fn dom_no_match_returns_null() {
    let server = dom_server("eval_result_dom_null.json");
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend(["dom".to_owned(), ".nonexistent".to_owned()]);

    let output = std::process::Command::new(ff_rdp_bin())
        .args(&args)
        .output()
        .expect("failed to spawn ff-rdp");

    handle.join().unwrap();

    assert!(output.status.success());

    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(json["results"].is_null());
    assert_eq!(json["total"], 0);
}

// ---------------------------------------------------------------------------
// Multiple elements — --text returns array
// ---------------------------------------------------------------------------

#[test]
fn dom_multiple_elements_text() {
    let server = dom_server("eval_result_dom_multi_text.json");
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend(["dom".to_owned(), "ul li".to_owned(), "--text".to_owned()]);

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

    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let results = json["results"]
        .as_array()
        .expect("results should be an array");
    assert_eq!(results.len(), 3);
    assert_eq!(results[0], "Item one");
    assert_eq!(results[1], "Item two");
    assert_eq!(results[2], "Item three");
    assert_eq!(json["total"], 3);
}

// ---------------------------------------------------------------------------
// Attrs mode
// ---------------------------------------------------------------------------

#[test]
fn dom_single_element_attrs() {
    let server = dom_server("eval_result_dom_attrs.json");
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend(["dom".to_owned(), "a".to_owned(), "--attrs".to_owned()]);

    let output = std::process::Command::new(ff_rdp_bin())
        .args(&args)
        .output()
        .expect("failed to spawn ff-rdp");

    handle.join().unwrap();

    assert!(output.status.success());

    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let results = &json["results"];
    assert_eq!(results["href"], "https://www.iana.org/domains/example");
    assert_eq!(results["class"], "link");
    assert_eq!(json["total"], 1);
}

// ---------------------------------------------------------------------------
// With --jq filter
// ---------------------------------------------------------------------------

#[test]
fn dom_with_jq_filter() {
    let server = dom_server("eval_result_dom_text.json");
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend([
        "dom".to_owned(),
        "h1".to_owned(),
        "--text".to_owned(),
        "--jq".to_owned(),
        ".".to_owned(),
    ]);

    let output = std::process::Command::new(ff_rdp_bin())
        .args(&args)
        .output()
        .expect("failed to spawn ff-rdp");

    handle.join().unwrap();

    assert!(output.status.success());

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(stdout.trim(), "\"Example Domain\"");
}
