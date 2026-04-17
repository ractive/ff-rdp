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

fn scroll_server(eval_result_fixture: &str) -> MockRdpServer {
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
// scroll to
// ---------------------------------------------------------------------------

#[test]
fn scroll_to_returns_scrolled_json() {
    let server = scroll_server("eval_result_scroll_to.json");
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend([
        "scroll".to_owned(),
        "to".to_owned(),
        "h2.section-title".to_owned(),
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

    assert_eq!(json["results"]["scrolled"], true);
    assert_eq!(json["results"]["selector"], "h2.section-title");
    assert!(json["results"]["viewport"].is_object());
    assert!(json["results"]["target"].is_object());
    assert!(json["results"]["atEnd"].is_boolean());
}

#[test]
fn scroll_to_element_not_found_exits_nonzero() {
    let server = scroll_server("eval_result_element_not_found.json");
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend([
        "scroll".to_owned(),
        "to".to_owned(),
        ".missing-element".to_owned(),
    ]);

    let output = std::process::Command::new(ff_rdp_bin())
        .args(&args)
        .output()
        .expect("failed to spawn ff-rdp");

    handle.join().unwrap();

    assert!(
        !output.status.success(),
        "expected failure for missing element"
    );
    assert_eq!(output.status.code(), Some(1));
}

// ---------------------------------------------------------------------------
// scroll by
// ---------------------------------------------------------------------------

#[test]
fn scroll_by_page_down_returns_viewport_coords() {
    let server = scroll_server("eval_result_scroll_by.json");
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend([
        "scroll".to_owned(),
        "by".to_owned(),
        "--page-down".to_owned(),
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

    assert_eq!(json["results"]["scrolled"], true);
    assert!(json["results"]["viewport"].is_object());
    assert!(json["results"]["scrollHeight"].is_number());
    assert!(json["results"]["atEnd"].is_boolean());
}

#[test]
fn scroll_by_dy_pixels() {
    let server = scroll_server("eval_result_scroll_by.json");
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend([
        "scroll".to_owned(),
        "by".to_owned(),
        "--dy".to_owned(),
        "300".to_owned(),
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

    assert_eq!(json["results"]["scrolled"], true);
}

#[test]
fn scroll_by_page_down_and_dy_rejected_by_clap() {
    // clap should reject --page-down + --dy as conflicting args before we ever connect
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);

    let mut args = base_args(port);
    args.extend([
        "scroll".to_owned(),
        "by".to_owned(),
        "--page-down".to_owned(),
        "--dy".to_owned(),
        "100".to_owned(),
    ]);

    let output = std::process::Command::new(ff_rdp_bin())
        .args(&args)
        .output()
        .expect("failed to spawn ff-rdp");

    assert!(
        !output.status.success(),
        "expected failure for conflicting flags"
    );
}

// ---------------------------------------------------------------------------
// scroll container
// ---------------------------------------------------------------------------

#[test]
fn scroll_container_returns_before_after_at_end() {
    let server = scroll_server("eval_result_scroll_container.json");
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend([
        "scroll".to_owned(),
        "container".to_owned(),
        ".sidebar".to_owned(),
        "--dy".to_owned(),
        "300".to_owned(),
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

    assert_eq!(json["results"]["scrolled"], true);
    assert_eq!(json["results"]["selector"], ".sidebar");
    assert!(json["results"]["before"].is_object());
    assert!(json["results"]["after"].is_object());
    assert!(json["results"]["scrollHeight"].is_number());
    assert!(json["results"]["clientHeight"].is_number());
    assert!(json["results"]["atEnd"].is_boolean());
}

// ---------------------------------------------------------------------------
// scroll top / scroll bottom
// ---------------------------------------------------------------------------

#[test]
fn scroll_top_returns_scrolled_json_at_origin() {
    let server = scroll_server("eval_result_scroll_top.json");
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend(["scroll".to_owned(), "top".to_owned()]);

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

    assert_eq!(json["results"]["scrolled"], true);
    assert!(json["results"]["viewport"].is_object());
    // scroll top sets Y to 0
    assert_eq!(json["results"]["viewport"]["y"], 0);
    assert!(json["results"]["scrollHeight"].is_number());
    assert!(json["results"]["atEnd"].is_boolean());
}

#[test]
fn scroll_bottom_returns_scrolled_json_at_end() {
    let server = scroll_server("eval_result_scroll_bottom.json");
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend(["scroll".to_owned(), "bottom".to_owned()]);

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

    assert_eq!(json["results"]["scrolled"], true);
    assert!(json["results"]["viewport"].is_object());
    assert!(json["results"]["scrollHeight"].is_number());
    // scroll bottom sets atEnd to true
    assert_eq!(json["results"]["atEnd"], true);
}

// ---------------------------------------------------------------------------
// scroll until
// ---------------------------------------------------------------------------

#[test]
fn scroll_until_returns_found_and_elapsed_ms() {
    // scroll until needs two evaluateJSAsync calls:
    //   1. check if element is visible — returns true (already visible)
    //   2. collect result data
    let server = MockRdpServer::new()
        .on("listTabs", load_fixture("list_tabs_response.json"))
        .on("getTarget", load_fixture("get_target_response.json"))
        .on_sequence(
            "evaluateJSAsync",
            vec![
                // Call 1: visibility check — returns true immediately
                (
                    load_fixture("eval_immediate_response.json"),
                    vec![load_fixture("eval_result_scroll_until.json")],
                ),
                // Call 2: result collection
                (
                    load_fixture("eval_immediate_response.json"),
                    vec![load_fixture("eval_result_scroll_until_result.json")],
                ),
            ],
        );

    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend([
        "scroll".to_owned(),
        "until".to_owned(),
        "#load-more-sentinel".to_owned(),
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

    assert_eq!(json["results"]["found"], true);
    assert_eq!(json["results"]["selector"], "#load-more-sentinel");
    assert!(json["results"]["elapsed_ms"].is_number());
    assert!(json["results"]["scrolls"].is_number());
}

// ---------------------------------------------------------------------------
// scroll text
// ---------------------------------------------------------------------------

#[test]
fn scroll_text_returns_target() {
    let server = scroll_server("eval_result_scroll_text.json");
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend([
        "scroll".to_owned(),
        "text".to_owned(),
        "Contact Us".to_owned(),
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

    assert_eq!(json["results"]["scrolled"], true);
    assert_eq!(json["results"]["text"], "Contact Us");
    assert!(json["results"]["viewport"].is_object());
    assert!(json["results"]["target"].is_object());
    assert_eq!(json["results"]["target"]["tag"], "h2");
}

#[test]
fn scroll_text_not_found_exits_nonzero() {
    let server = scroll_server("eval_result_element_not_found.json");
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend([
        "scroll".to_owned(),
        "text".to_owned(),
        "Nonexistent text XYZ".to_owned(),
    ]);

    let output = std::process::Command::new(ff_rdp_bin())
        .args(&args)
        .output()
        .expect("failed to spawn ff-rdp");

    handle.join().unwrap();

    assert!(
        !output.status.success(),
        "expected failure for text not found"
    );
    assert_eq!(output.status.code(), Some(1));
}
