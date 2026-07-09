use std::path::PathBuf;

use super::support::{MockRdpServer, load_fixture};
use base64::Engine as _;

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

/// The canonical 1×1 PNG data URL used across all screenshot tests.
const PNG_1X1: &str = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVR4nGP4//8/AAX+Av4N70a4AAAAAElFTkSuQmCC";

/// Build a mock server that serves a successful two-step screenshot flow.
///
/// Flow: `listTabs` → `getTarget` → `prepareCapture` → `getRoot` → `capture`
fn screenshot_server() -> MockRdpServer {
    let prepare_response = serde_json::json!({
        "from": "server1.conn0.child2/screenshotContentActor15",
        "value": {
            "rect": null,
            "messages": [],
            "windowDpr": 1.0,
            "windowZoom": 1.0
        }
    });
    let get_root_response = serde_json::json!({
        "from": "root",
        "screenshotActor": "server1.conn0.screenshotActor7",
        "preferenceActor": "server1.conn0.preferenceActor1",
        "addonsActor": "server1.conn0.addonsActor2"
    });
    let capture_response = serde_json::json!({
        "from": "server1.conn0.screenshotActor7",
        "value": {
            "data": PNG_1X1,
            "filename": "screenshot.png",
            "messages": []
        }
    });

    MockRdpServer::new()
        .on("listTabs", load_fixture("list_tabs_response.json"))
        .on("getTarget", load_fixture("get_target_response.json"))
        .on("prepareCapture", prepare_response)
        .on("getRoot", get_root_response)
        .on("capture", capture_response)
}

// iter-92 Theme A: the previous `screenshot_full_page_server` mock seeded
// the standard `screenshotActor.capture` path, which is now bypassed for
// `--full-page` (the CLI routes through the parent-process drawSnapshot
// fallback to avoid the FF 151 viewport-clamp regression).  Flag-parse
// acceptance is covered by `clap_screenshot_full_page_flag_parsed` in
// `crates/ff-rdp-cli/src/commands/screenshot.rs`; functional coverage is
// `live_screenshot_full_page` (live_61l) and the iter-92 pre-fix repro
// `pre_fix_repro_screenshot_full_page_taller_than_viewport`.

/// Create a unique temp directory under the OS temp dir for one test.
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
// Happy-path: two-step snapshot actor protocol
// ---------------------------------------------------------------------------

#[test]
fn screenshot_saves_png_to_explicit_output_path() {
    let server = screenshot_server();
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

    assert!(out_path.exists(), "PNG file should have been written");
    let png_bytes = std::fs::read(&out_path).expect("read png");
    assert_eq!(&png_bytes[..4], b"\x89PNG", "file should be a PNG");

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
    let server = screenshot_server();
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

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

    assert!(
        PathBuf::from(path_str).exists(),
        "auto-named PNG should exist at {path_str}"
    );

    let _ = std::fs::remove_dir_all(&work_dir);
}

// ---------------------------------------------------------------------------
// --base64 mode: returns PNG as base64 in JSON, no file written
// ---------------------------------------------------------------------------

#[test]
fn screenshot_base64_returns_png_data_without_writing_file() {
    let server = screenshot_server();
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let work_dir = unique_temp_dir("screenshot_base64");

    let mut args = base_args(port);
    args.extend(["screenshot".to_owned(), "--base64".to_owned()]);

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

    assert_eq!(json["total"], 1);
    assert_eq!(json["results"]["width"], 1);
    assert_eq!(json["results"]["height"], 1);
    assert!(json["results"]["bytes"].as_u64().unwrap_or(0) > 0);

    let b64_str = json["results"]["base64"]
        .as_str()
        .expect("results.base64 should be a string");
    assert!(!b64_str.is_empty(), "base64 string should not be empty");

    let png_bytes = base64::engine::general_purpose::STANDARD
        .decode(b64_str)
        .expect("results.base64 must be valid base64");
    assert_eq!(&png_bytes[..4], b"\x89PNG", "decoded data should be a PNG");

    let files_in_dir: Vec<_> = std::fs::read_dir(&work_dir)
        .expect("read work_dir")
        .filter_map(Result::ok)
        .collect();
    assert!(
        files_in_dir.is_empty(),
        "no file should be written when --base64 is used, found: {:?}",
        files_in_dir
            .iter()
            .map(std::fs::DirEntry::path)
            .collect::<Vec<_>>()
    );

    assert!(
        json["results"]["path"].is_null(),
        "path should not appear in --base64 output"
    );

    let _ = std::fs::remove_dir_all(&work_dir);
}

// ---------------------------------------------------------------------------
// Error-path: prepareCapture fails (actor module load failure)
// ---------------------------------------------------------------------------

#[test]
fn screenshot_module_load_failure_surfaces_clean_version_mismatch_message() {
    // Firefox's actual error shape for the missing screenshot ESM:
    //   error="unknownError" message="Error occurred while creating actor' …
    //   Error: Unable to load actor module 'devtools/server/actors/screenshot' …"
    let module_load_err = serde_json::json!({
        "from": "server1.conn0.child2/screenshotContentActor15",
        "error": "unknownError",
        "message": "Error occurred while creating actor' server1.conn0.child2/screenshotContentActor15: \
                    Error: Unable to load actor module 'devtools/server/actors/screenshot' \
                    ChromeUtils.importESModule: global option is required in DevTools distinct global"
    });

    let server = MockRdpServer::new()
        .on("listTabs", load_fixture("list_tabs_response.json"))
        .on("getTarget", load_fixture("get_target_response.json"))
        .on("prepareCapture", module_load_err);

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
        "expected non-zero exit when screenshot actor module fails to load"
    );

    // The clean error is emitted as the JSON error envelope on stdout (iter-98
    // Theme D removed the duplicate human `error:` stderr line).
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let combined = format!("{stderr}{stdout}");
    assert!(
        combined.contains("screenshot actor not found in Firefox"),
        "output should include 'screenshot actor not found in Firefox': stderr={stderr:?} stdout={stdout:?}"
    );
    assert!(
        combined.contains("ff-rdp doctor"),
        "output should reference `ff-rdp doctor`: stderr={stderr:?} stdout={stdout:?}"
    );
    // The raw Firefox stack trace must not be echoed anywhere.
    assert!(
        !combined.contains("Unable to load actor module"),
        "output must not echo the raw Firefox stack: stderr={stderr:?} stdout={stdout:?}"
    );
}

#[test]
fn screenshot_module_load_failure_in_two_step_protocol_surfaces_clean_message() {
    let module_load_err = serde_json::json!({
        "from": "server1.conn0.child2/screenshotContentActor15",
        "error": "unknownError",
        "message": "Error occurred while creating actor' …: Error: Unable to load actor module 'devtools/server/actors/screenshot'"
    });

    let server = MockRdpServer::new()
        .on("listTabs", load_fixture("list_tabs_response.json"))
        .on("getTarget", load_fixture("get_target_response.json"))
        .on("prepareCapture", module_load_err);

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
        "expected non-zero exit when prepareCapture fails with module-load error"
    );

    // The clean error is emitted as the JSON error envelope on stdout (iter-98
    // Theme D removed the duplicate human `error:` stderr line).
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let combined = format!("{stderr}{stdout}");
    assert!(
        combined.contains("screenshot actor not found in Firefox"),
        "output should include 'screenshot actor not found in Firefox': stderr={stderr:?} stdout={stdout:?}"
    );
    assert!(
        combined.contains("ff-rdp doctor"),
        "output should reference `ff-rdp doctor`: stderr={stderr:?} stdout={stdout:?}"
    );
}

// ---------------------------------------------------------------------------
// iter-43: --full-page / --viewport-height
// ---------------------------------------------------------------------------

#[test]
fn screenshot_full_page_and_viewport_height_conflict() {
    // Providing both flags is a user error — clap rejects it before connecting.
    let output = std::process::Command::new(ff_rdp_bin())
        .args(["screenshot", "--full-page", "--viewport-height", "1000"])
        .output()
        .expect("failed to spawn ff-rdp");

    assert!(
        !output.status.success(),
        "expected failure when both --full-page and --viewport-height are given"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("full-page")
            || stderr.contains("viewport-height")
            || stderr.contains("cannot be used with"),
        "expected conflict error, got: {stderr}"
    );
}

#[test]
fn screenshot_viewport_height_flag_returns_error() {
    // --viewport-height is no longer supported by the snapshot-actor path.
    // The flag is accepted by clap but returns an error before connecting.
    let output = std::process::Command::new(ff_rdp_bin())
        .args([
            "--host",
            "127.0.0.1",
            "--port",
            "1",
            "--no-daemon",
            "screenshot",
            "--viewport-height",
            "2500",
        ])
        .output()
        .expect("failed to spawn ff-rdp");

    assert!(
        !output.status.success(),
        "expected failure when --viewport-height is used"
    );
    // The unsupported-flag error is emitted as the JSON error envelope on
    // stdout (iter-98 Theme D removed the duplicate human `error:` stderr line).
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let combined = format!("{stderr}{stdout}");
    assert!(
        combined.contains("viewport-height") || combined.contains("not supported"),
        "expected unsupported error, got: stderr={stderr:?} stdout={stdout:?}"
    );
}

#[test]
fn screenshot_with_jq_filter_extracts_path() {
    let server = screenshot_server();
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
