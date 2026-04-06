mod support;

use std::path::PathBuf;

use support::{MockRdpServer, load_fixture};

fn ff_rdp_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_ff-rdp"))
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

fn screenshot_server(eval_result_fixture: &str) -> MockRdpServer {
    MockRdpServer::new()
        .on("listTabs", load_fixture("list_tabs_response.json"))
        .on("getTarget", load_fixture("get_target_response.json"))
        .on_with_followup(
            "evaluateJSAsync",
            load_fixture("eval_immediate_response.json"),
            load_fixture(eval_result_fixture),
        )
}

/// Create a unique temp directory under the OS temp dir for one test.
///
/// Returns the path; the caller is responsible for cleaning it up (or leaving
/// it — test temp files are harmless and cleaned on the next reboot).
fn unique_temp_dir(label: &str) -> PathBuf {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("ff_rdp_test_{label}_{ts}"));
    std::fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

// ---------------------------------------------------------------------------
// Happy-path: data URL returned as a plain string value
// ---------------------------------------------------------------------------

#[test]
fn screenshot_saves_png_to_explicit_output_path() {
    let server = screenshot_server("eval_result_screenshot.json");
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let out_dir = unique_temp_dir("screenshot_explicit");
    let out_path = out_dir.join("test.png");

    let mut args = base_args(port);
    args.extend([
        "screenshot".to_owned(),
        "--output".to_owned(),
        out_path.to_string_lossy().into_owned(),
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

    // PNG file must exist and start with the PNG magic bytes.
    assert!(out_path.exists(), "PNG file should have been written");
    let png_bytes = std::fs::read(&out_path).expect("read png");
    assert_eq!(&png_bytes[..4], b"\x89PNG", "file should be a PNG");

    // JSON output envelope.
    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("stdout must be valid JSON");

    assert_eq!(json["total"], 1);
    assert_eq!(json["results"]["width"], 1);
    assert_eq!(json["results"]["height"], 1);
    assert!(json["results"]["bytes"].as_u64().unwrap_or(0) > 0);
    assert!(
        json["results"]["path"]
            .as_str()
            .unwrap_or("")
            .ends_with("test.png"),
        "path should end with test.png: {}",
        json["results"]["path"]
    );

    let _ = std::fs::remove_dir_all(&out_dir);
}

#[test]
fn screenshot_auto_names_file_when_no_output_given() {
    let server = screenshot_server("eval_result_screenshot.json");
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    // Run in a temp directory so the auto-named file lands there.
    let work_dir = unique_temp_dir("screenshot_auto");

    let mut args = base_args(port);
    args.push("screenshot".to_owned());

    let output = std::process::Command::new(ff_rdp_bin())
        .args(&args)
        .current_dir(&work_dir)
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

    let path_str = json["results"]["path"].as_str().unwrap_or("");
    assert!(
        path_str.contains("screenshot-")
            && std::path::Path::new(path_str)
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("png")),
        "expected auto-named screenshot path, got: {path_str}"
    );

    // The file should also actually exist.
    assert!(
        PathBuf::from(path_str).exists(),
        "auto-named PNG should exist at {path_str}"
    );

    let _ = std::fs::remove_dir_all(&work_dir);
}

// ---------------------------------------------------------------------------
// Happy-path: data URL returned as a LongString (fetched via substring)
// ---------------------------------------------------------------------------

#[test]
fn screenshot_handles_longstring_data_url() {
    let server = MockRdpServer::new()
        .on("listTabs", load_fixture("list_tabs_response.json"))
        .on("getTarget", load_fixture("get_target_response.json"))
        .on_with_followup(
            "evaluateJSAsync",
            load_fixture("eval_immediate_response.json"),
            load_fixture("eval_result_screenshot_longstring.json"),
        )
        .on(
            "substring",
            load_fixture("substring_screenshot_response.json"),
        );

    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let out_dir = unique_temp_dir("screenshot_longstring");
    let out_path = out_dir.join("longstring.png");

    let mut args = base_args(port);
    args.extend([
        "screenshot".to_owned(),
        "--output".to_owned(),
        out_path.to_string_lossy().into_owned(),
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

    assert!(out_path.exists(), "PNG file should have been written");
    let png_bytes = std::fs::read(&out_path).expect("read png");
    assert_eq!(&png_bytes[..4], b"\x89PNG");

    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("stdout must be valid JSON");
    assert_eq!(json["total"], 1);
    assert_eq!(json["results"]["width"], 1);
    assert_eq!(json["results"]["height"], 1);

    let _ = std::fs::remove_dir_all(&out_dir);
}

// ---------------------------------------------------------------------------
// Error-path: JS returns null (drawWindow not available)
// ---------------------------------------------------------------------------

#[test]
fn screenshot_null_result_exits_nonzero_with_helpful_message() {
    let server = screenshot_server("eval_result_screenshot_null.json");
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.push("screenshot".to_owned());

    let output = std::process::Command::new(ff_rdp_bin())
        .args(&args)
        .output()
        .expect("failed to spawn ff-rdp");

    handle.join().unwrap();

    assert!(
        !output.status.success(),
        "expected failure when drawWindow is unavailable"
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("drawWindow"),
        "stderr should mention drawWindow: {stderr}"
    );
}

// ---------------------------------------------------------------------------
// jq filter on success output
// ---------------------------------------------------------------------------

#[test]
fn screenshot_with_jq_filter_extracts_path() {
    let server = screenshot_server("eval_result_screenshot.json");
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let out_dir = unique_temp_dir("screenshot_jq");
    let out_path = out_dir.join("jq_test.png");

    let mut args = base_args(port);
    args.extend([
        "--jq".to_owned(),
        ".results.path".to_owned(),
        "screenshot".to_owned(),
        "--output".to_owned(),
        out_path.to_string_lossy().into_owned(),
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
    assert!(
        stdout.trim().ends_with("jq_test.png\""),
        "jq filter should return the path string: {stdout}"
    );

    let _ = std::fs::remove_dir_all(&out_dir);
}
