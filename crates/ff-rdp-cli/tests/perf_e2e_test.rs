mod support;

use support::{MockRdpServer, load_fixture};

/// Full JSON produced by the mock substring actor for the long-string resource test.
const LONGSTRING_PERF_JSON: &str = r#"[{"name":"https://example.com/app.js","initiatorType":"script","duration":42.5,"transferSize":12345,"encodedBodySize":12000,"decodedBodySize":36000,"startTime":100.0,"responseEnd":142.5,"nextHopProtocol":"h2"}]"#;

fn ff_rdp_bin() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_BIN_EXE_ff-rdp"))
}

fn base_args(port: u16) -> Vec<String> {
    vec![
        "--host".to_owned(),
        "127.0.0.1".to_owned(),
        "--port".to_owned(),
        port.to_string(),
        "--timeout".to_owned(),
        "1000".to_owned(),
        "--no-daemon".to_owned(),
    ]
}

fn perf_server(fixture: &str) -> MockRdpServer {
    MockRdpServer::new()
        .on("listTabs", load_fixture("list_tabs_response.json"))
        .on("getTarget", load_fixture("get_target_response.json"))
        .on_with_followup(
            "evaluateJSAsync",
            load_fixture("eval_immediate_response.json"),
            load_fixture(fixture),
        )
}

// ---------------------------------------------------------------------------
// perf (resource type — default)
// ---------------------------------------------------------------------------

#[test]
fn perf_shows_resources() {
    let server = perf_server("eval_result_perf_resource.json");
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.push("perf".to_owned());

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

    assert_eq!(json["total"], 2);
    let results = json["results"].as_array().expect("results is array");

    assert_eq!(results[0]["url"], "https://example.com/app.js");
    assert_eq!(results[0]["initiator_type"], "script");
    assert_eq!(results[0]["duration_ms"], 42.5);
    assert_eq!(results[0]["transfer_size"], 12345);
    assert_eq!(results[0]["decoded_size"], 36000);
    assert_eq!(results[0]["protocol"], "h2");

    assert_eq!(results[1]["url"], "https://example.com/favicon.ico");
    assert_eq!(results[1]["initiator_type"], "img");
}

#[test]
fn perf_resource_explicit_type_flag() {
    let server = perf_server("eval_result_perf_resource.json");
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend([
        "perf".to_owned(),
        "--type".to_owned(),
        "resource".to_owned(),
    ]);

    let output = std::process::Command::new(ff_rdp_bin())
        .args(&args)
        .output()
        .expect("failed to spawn ff-rdp");

    handle.join().unwrap();

    assert!(output.status.success());
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["total"], 2);
}

#[test]
fn perf_filter_by_url() {
    let server = perf_server("eval_result_perf_resource.json");
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend([
        "perf".to_owned(),
        "--filter".to_owned(),
        "favicon".to_owned(),
    ]);

    let output = std::process::Command::new(ff_rdp_bin())
        .args(&args)
        .output()
        .expect("failed to spawn ff-rdp");

    handle.join().unwrap();

    assert!(output.status.success());
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["total"], 1);
    assert_eq!(json["results"][0]["url"], "https://example.com/favicon.ico");
}

#[test]
fn perf_with_jq_filter() {
    let server = perf_server("eval_result_perf_resource.json");
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend([
        "perf".to_owned(),
        "--jq".to_owned(),
        ".[] | select(.initiator_type == \"script\") | .url".to_owned(),
    ]);

    let output = std::process::Command::new(ff_rdp_bin())
        .args(&args)
        .output()
        .expect("failed to spawn ff-rdp");

    handle.join().unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(stdout.trim(), r#""https://example.com/app.js""#);
}

#[test]
fn perf_jq_array_iteration_works() {
    // Regression test: `.[].url` must work because jq is applied to .results
    // directly, not to the full `{meta, results, total}` envelope.
    let server = perf_server("eval_result_perf_resource.json");
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend(["perf".to_owned(), "--jq".to_owned(), ".[].url".to_owned()]);

    let output = std::process::Command::new(ff_rdp_bin())
        .args(&args)
        .output()
        .expect("failed to spawn ff-rdp");

    handle.join().unwrap();

    assert!(
        output.status.success(),
        ".[].url should work: stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<&str> = stdout.trim().lines().collect();
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[0], r#""https://example.com/app.js""#);
    assert_eq!(lines[1], r#""https://example.com/favicon.ico""#);
}

// ---------------------------------------------------------------------------
// perf --type navigation
// ---------------------------------------------------------------------------

#[test]
fn perf_navigation_type() {
    let server = perf_server("eval_result_perf_navigation.json");
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend([
        "perf".to_owned(),
        "--type".to_owned(),
        "navigation".to_owned(),
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
    assert_eq!(json["total"], 1);
    let nav = &json["results"][0];
    assert_eq!(nav["url"], "https://example.com/");
    // dns_ms = domainLookupEnd(30) - domainLookupStart(10) = 20
    assert_eq!(nav["dns_ms"], 20.0);
    // ttfb_ms = responseStart(340) - activationStart(0) = 340
    assert_eq!(nav["ttfb_ms"], 340.0);
    // tls_ms = connectEnd(60) - secureConnectionStart(35) = 25
    assert_eq!(nav["tls_ms"], 25.0);
}

// ---------------------------------------------------------------------------
// perf --type paint
// ---------------------------------------------------------------------------

#[test]
fn perf_paint_type() {
    let server = perf_server("eval_result_perf_paint.json");
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend(["perf".to_owned(), "--type".to_owned(), "paint".to_owned()]);

    let output = std::process::Command::new(ff_rdp_bin())
        .args(&args)
        .output()
        .expect("failed to spawn ff-rdp");

    handle.join().unwrap();

    assert!(output.status.success());

    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["total"], 2);
    assert_eq!(json["results"][0]["name"], "first-paint");
    assert_eq!(json["results"][1]["name"], "first-contentful-paint");
}

// ---------------------------------------------------------------------------
// perf --type lcp (alias)
// ---------------------------------------------------------------------------

#[test]
fn perf_lcp_alias() {
    let server = perf_server("eval_result_perf_lcp.json");
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend(["perf".to_owned(), "--type".to_owned(), "lcp".to_owned()]);

    let output = std::process::Command::new(ff_rdp_bin())
        .args(&args)
        .output()
        .expect("failed to spawn ff-rdp");

    handle.join().unwrap();

    assert!(output.status.success());

    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["total"], 2);
    // Last LCP entry is the banner
    assert_eq!(json["results"][1]["url"], "https://example.com/banner.jpg");
    assert_eq!(json["results"][1]["start_time_ms"], 1850.0);
    assert_eq!(json["results"][1]["size"], 120_000);
}

// ---------------------------------------------------------------------------
// perf --type cls (alias)
// ---------------------------------------------------------------------------

#[test]
fn perf_cls_alias() {
    let server = perf_server("eval_result_perf_cls.json");
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend(["perf".to_owned(), "--type".to_owned(), "cls".to_owned()]);

    let output = std::process::Command::new(ff_rdp_bin())
        .args(&args)
        .output()
        .expect("failed to spawn ff-rdp");

    handle.join().unwrap();

    assert!(output.status.success());

    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["total"], 3);
    // Third shift has hadRecentInput: true
    assert_eq!(json["results"][2]["had_recent_input"], true);
}

// ---------------------------------------------------------------------------
// perf --type longtask
// ---------------------------------------------------------------------------

#[test]
fn perf_longtask_type() {
    let server = perf_server("eval_result_perf_longtask.json");
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend([
        "perf".to_owned(),
        "--type".to_owned(),
        "longtask".to_owned(),
    ]);

    let output = std::process::Command::new(ff_rdp_bin())
        .args(&args)
        .output()
        .expect("failed to spawn ff-rdp");

    handle.join().unwrap();

    assert!(output.status.success());

    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["total"], 2);
    assert_eq!(json["results"][0]["duration_ms"], 120.0);
    assert_eq!(json["results"][1]["duration_ms"], 80.0);
}

// ---------------------------------------------------------------------------
// perf vitals
// ---------------------------------------------------------------------------

#[test]
fn perf_vitals_computes_cwv() {
    let server = perf_server("eval_result_perf_vitals.json");
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend(["perf".to_owned(), "vitals".to_owned()]);

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

    let r = &json["results"];

    // LCP: last entry startTime = 1850.0
    assert_eq!(r["lcp_ms"], 1850.0);
    assert_eq!(r["lcp_rating"], "good");

    // FCP: first-contentful-paint startTime = 980.0
    assert_eq!(r["fcp_ms"], 980.0);
    assert_eq!(r["fcp_rating"], "good");

    // TTFB: responseStart(340) - activationStart(0) = 340
    assert_eq!(r["ttfb_ms"], 340.0);
    assert_eq!(r["ttfb_rating"], "good");

    // CLS: two shifts 0.02 + 0.03 = 0.05 (same window, gap 100ms < 1s)
    assert_eq!(r["cls"], 0.05);
    assert_eq!(r["cls_rating"], "good");

    // TBT: one longtask duration=170ms > 50ms, after FCP (980ms).
    // Task starts at 1000ms, ends at 1170ms > 980ms → blocking = 170 - 50 = 120ms
    assert_eq!(r["tbt_ms"], 120.0);
    assert_eq!(r["tbt_rating"], "good");
}

// ---------------------------------------------------------------------------
// Error path: exception exits non-zero
// ---------------------------------------------------------------------------

#[test]
fn perf_exception_exits_nonzero() {
    let server = MockRdpServer::new()
        .on("listTabs", load_fixture("list_tabs_response.json"))
        .on("getTarget", load_fixture("get_target_response.json"))
        .on_with_followup(
            "evaluateJSAsync",
            load_fixture("eval_immediate_response.json"),
            load_fixture("eval_result_cached_exception.json"),
        );

    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.push("perf".to_owned());

    let output = std::process::Command::new(ff_rdp_bin())
        .args(&args)
        .output()
        .expect("failed to spawn ff-rdp");

    handle.join().unwrap();

    assert!(
        !output.status.success(),
        "expected non-zero exit for exception"
    );
    assert_eq!(output.status.code(), Some(1));
}

// ---------------------------------------------------------------------------
// Long-string grip handling
// ---------------------------------------------------------------------------

#[test]
fn perf_handles_long_string() {
    let longstring_result = load_fixture("eval_result_cached_longstring.json");
    let substring_response = serde_json::json!({
        "from": "server1.conn0.longstr1",
        "substring": LONGSTRING_PERF_JSON
    });

    let server = MockRdpServer::new()
        .on("listTabs", load_fixture("list_tabs_response.json"))
        .on("getTarget", load_fixture("get_target_response.json"))
        .on_with_followup(
            "evaluateJSAsync",
            load_fixture("eval_immediate_response.json"),
            longstring_result,
        )
        .on("substring", substring_response);

    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.push("perf".to_owned());

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

    assert_eq!(json["total"], 1);
    assert_eq!(json["results"][0]["url"], "https://example.com/app.js");
    assert_eq!(json["results"][0]["initiator_type"], "script");
    assert_eq!(json["results"][0]["duration_ms"], 42.5);
}
