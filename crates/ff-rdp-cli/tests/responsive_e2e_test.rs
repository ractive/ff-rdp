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

/// Build a `MockRdpServer` pre-wired for a `responsive` run.
///
/// The server handles:
/// - `evaluateJSAsync` with a sequence: GET_VIEWPORT (once) + per-width
///   pairs of (set-css, geometry) + restore-css (once).
///
/// All viewport simulation is now done via `evaluateJSAsync` (CSS inline
/// style manipulation), so no `setViewportSize` actor call is needed.
fn responsive_server(
    eval_sequence: Vec<(serde_json::Value, Vec<serde_json::Value>)>,
) -> MockRdpServer {
    MockRdpServer::new()
        .on("listTabs", load_fixture("list_tabs_response.json"))
        .on("getTarget", load_fixture("get_target_response.json"))
        .on_sequence("evaluateJSAsync", eval_sequence)
}

/// Build an eval sequence for `N` viewport widths.
///
/// The `evaluateJSAsync` sequence is:
///   1. GET_VIEWPORT        (1×)  → eval_result_responsive_viewport.json
///   2. Per width (N×):
///      a. SET_VIEWPORT_CSS → eval_result_responsive_undefined.json
///      b. geometry IIFE    → eval_result_responsive_geometry.json
///   3. RESTORE_VIEWPORT_CSS (1×) → eval_result_responsive_undefined.json
///
/// Total: 1 + 2×N + 1 calls.
fn build_eval_sequence(width_count: usize) -> Vec<(serde_json::Value, Vec<serde_json::Value>)> {
    let immediate = load_fixture("eval_immediate_response.json");
    let viewport = load_fixture("eval_result_responsive_viewport.json");
    let geometry = load_fixture("eval_result_responsive_geometry.json");
    let undefined = load_fixture("eval_result_responsive_undefined.json");

    let mut seq = Vec::new();

    // Step 1: get current viewport
    seq.push((immediate.clone(), vec![viewport]));

    // Step 2: for each width — set CSS, then collect geometry
    for _ in 0..width_count {
        seq.push((immediate.clone(), vec![undefined.clone()])); // SET_VIEWPORT_CSS_JS
        seq.push((immediate.clone(), vec![geometry.clone()])); // geometry IIFE
    }

    // Step 3: restore CSS styles
    seq.push((immediate.clone(), vec![undefined.clone()])); // RESTORE_VIEWPORT_CSS_JS

    seq
}

// ---------------------------------------------------------------------------
// Happy path — single width
// ---------------------------------------------------------------------------

#[test]
fn responsive_single_width() {
    let server = responsive_server(build_eval_sequence(1));
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend([
        "responsive".to_owned(),
        "--widths".to_owned(),
        "320".to_owned(),
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

    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("stdout must be valid JSON");

    // Envelope structure
    assert_eq!(json["total"], 1, "total should equal width count");
    assert!(json["results"]["breakpoints"].is_array());

    let breakpoints = json["results"]["breakpoints"]
        .as_array()
        .expect("breakpoints must be an array");
    assert_eq!(breakpoints.len(), 1);
    assert_eq!(breakpoints[0]["width"], 320);

    // Elements at the single breakpoint
    let elements = breakpoints[0]["elements"]
        .as_array()
        .expect("elements must be an array");
    assert_eq!(elements.len(), 1);
    assert_eq!(elements[0]["selector"], "h1");
    assert_eq!(elements[0]["tag"], "h1");
    assert_eq!(elements[0]["computed"]["font_size"], "32px");

    // Original viewport is preserved in output (keyed by innerWidth/innerHeight
    // since GET_VIEWPORT_JS now returns window.innerWidth/innerHeight).
    assert_eq!(json["results"]["original_viewport"]["innerWidth"], 1280);
    assert_eq!(json["results"]["original_viewport"]["innerHeight"], 800);
}

// ---------------------------------------------------------------------------
// Multiple widths
// ---------------------------------------------------------------------------

#[test]
fn responsive_multiple_widths() {
    let server = responsive_server(build_eval_sequence(3));
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend([
        "responsive".to_owned(),
        "--widths".to_owned(),
        "320,768,1024".to_owned(),
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
        "expected success, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("stdout must be valid JSON");

    assert_eq!(json["total"], 3);

    let breakpoints = json["results"]["breakpoints"]
        .as_array()
        .expect("breakpoints must be an array");
    assert_eq!(breakpoints.len(), 3);

    // Widths are emitted in order
    assert_eq!(breakpoints[0]["width"], 320);
    assert_eq!(breakpoints[1]["width"], 768);
    assert_eq!(breakpoints[2]["width"], 1024);

    // Meta contains selectors and widths
    let meta_selectors = json["meta"]["selectors"]
        .as_array()
        .expect("meta.selectors must be an array");
    assert_eq!(meta_selectors.len(), 2);
    assert_eq!(meta_selectors[0], "h1");
    assert_eq!(meta_selectors[1], "p");

    let meta_widths = json["meta"]["widths"]
        .as_array()
        .expect("meta.widths must be an array");
    assert_eq!(meta_widths.len(), 3);
}

// ---------------------------------------------------------------------------
// Default widths (320, 768, 1024, 1440)
// ---------------------------------------------------------------------------

#[test]
fn responsive_default_widths() {
    let server = responsive_server(build_eval_sequence(4));
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    // No --widths flag → uses default 320,768,1024,1440
    args.extend(["responsive".to_owned(), "h1".to_owned()]);

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

    assert_eq!(json["total"], 4, "default is 4 breakpoints");

    let breakpoints = json["results"]["breakpoints"]
        .as_array()
        .expect("breakpoints must be an array");
    assert_eq!(breakpoints[0]["width"], 320);
    assert_eq!(breakpoints[1]["width"], 768);
    assert_eq!(breakpoints[2]["width"], 1024);
    assert_eq!(breakpoints[3]["width"], 1440);
}

// ---------------------------------------------------------------------------
// --jq filter
// ---------------------------------------------------------------------------

#[test]
fn responsive_with_jq_filter() {
    let server = responsive_server(build_eval_sequence(1));
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend([
        "--jq".to_owned(),
        ".results.breakpoints[0].width".to_owned(),
        "responsive".to_owned(),
        "--widths".to_owned(),
        "320".to_owned(),
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

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(stdout.trim(), "320");
}

// ---------------------------------------------------------------------------
// Zero width is rejected before connecting
// ---------------------------------------------------------------------------

#[test]
fn responsive_zero_width_rejected() {
    // This test does not need a real server because the error is caught before
    // any connection is established.  We bind a port so the CLI has a valid
    // --port argument and fails for the right reason, not a parse error.
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().unwrap().port();
    drop(listener);

    let mut args = base_args(port);
    args.extend([
        "responsive".to_owned(),
        "--widths".to_owned(),
        "320,0,1024".to_owned(),
        "h1".to_owned(),
    ]);

    let output = std::process::Command::new(ff_rdp_bin())
        .args(&args)
        .output()
        .expect("failed to spawn ff-rdp");

    assert!(!output.status.success(), "expected failure for zero width");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("greater than 0") || stderr.contains("width"),
        "unexpected stderr: {stderr}"
    );
}
