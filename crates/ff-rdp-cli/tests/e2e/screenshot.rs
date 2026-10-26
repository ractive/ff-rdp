use std::path::PathBuf;

use super::support::{self, MockRdpServer, load_fixture};
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
    ]
}

/// The canonical 1×1 PNG data URL used across all screenshot tests.
const PNG_1X1: &str = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVR4nGP4//8/AAX+Av4N70a4AAAAAElFTkSuQmCC";

/// Build a mock server that serves a successful two-step screenshot flow.
///
/// Flow: `listTabs` → `getTarget` → `prepareCapture` → `getRoot` → `capture`
fn screenshot_server() -> MockRdpServer {
    screenshot_server_capturing(PNG_1X1)
}

/// [`screenshot_server`] whose `capture` replies with `png_data_url`.
fn screenshot_server_capturing(png_data_url: &str) -> MockRdpServer {
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
            "data": png_data_url,
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
        support::output_note(&output)
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
        support::output_note(&output)
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

/// iter-296: `--output-root DIR` without `-o` writes the auto-named file
/// inside DIR (it used to land in the cwd and fail as "escapes --output-root").
#[test]
fn screenshot_output_root_without_output_writes_inside_root() {
    let server = screenshot_server();
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let work_dir = unique_temp_dir("screenshot_root_cwd");
    let root = unique_temp_dir("screenshot_root");

    let mut args = base_args(port);
    args.extend([
        "screenshot".to_owned(),
        "--output-root".to_owned(),
        root.to_string_lossy().into_owned(),
    ]);
    let output = std::process::Command::new(ff_rdp_bin())
        .args(&args)
        .current_dir(&work_dir)
        .output()
        .expect("failed to spawn ff-rdp");
    handle.join().unwrap();

    assert!(output.status.success(), "{}", support::output_note(&output));
    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("stdout must be valid JSON");
    let path = PathBuf::from(json["results"]["path"].as_str().unwrap_or(""));
    assert!(path.exists(), "PNG should exist at {}", path.display());
    assert_eq!(
        path.parent().map(std::path::Path::to_path_buf),
        Some(root.canonicalize().unwrap()),
        "default file must land in --output-root"
    );
    assert_eq!(std::fs::read_dir(&work_dir).unwrap().count(), 0);

    let _ = std::fs::remove_dir_all(&work_dir);
    let _ = std::fs::remove_dir_all(&root);
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
        support::output_note(&output)
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
    // iter-135: this path is reached *after* an actor was found and called, so
    // the pre-135 "screenshot actor not found in Firefox N root form" text was
    // false here.  What must hold is that the error is clean, actionable, and
    // free of the misleading headless hint.
    assert!(
        combined.contains("rendered no image for this capture"),
        "output should state the real failure: stderr={stderr:?} stdout={stdout:?}"
    );
    assert!(
        !combined.contains("screenshot actor not found in Firefox"),
        "output must not claim the actor is missing on a path that found one: \
         stderr={stderr:?} stdout={stdout:?}"
    );
    assert!(
        !combined.contains("relaunch with: ff-rdp launch --headless"),
        "output must not tell an already-headless user to relaunch headless: \
         stderr={stderr:?} stdout={stdout:?}"
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
    // iter-135: this path is reached *after* an actor was found and called, so
    // the pre-135 "screenshot actor not found in Firefox N root form" text was
    // false here.  What must hold is that the error is clean, actionable, and
    // free of the misleading headless hint.
    assert!(
        combined.contains("rendered no image for this capture"),
        "output should state the real failure: stderr={stderr:?} stdout={stdout:?}"
    );
    assert!(
        !combined.contains("screenshot actor not found in Firefox"),
        "output must not claim the actor is missing on a path that found one: \
         stderr={stderr:?} stdout={stdout:?}"
    );
    assert!(
        !combined.contains("relaunch with: ff-rdp launch --headless"),
        "output must not tell an already-headless user to relaunch headless: \
         stderr={stderr:?} stdout={stdout:?}"
    );
    assert!(
        combined.contains("ff-rdp doctor"),
        "output should reference `ff-rdp doctor`: stderr={stderr:?} stdout={stdout:?}"
    );
}

// ---------------------------------------------------------------------------
// --viewport-height was removed: it had been refused at runtime since
// iter-61v, while README and --help still advertised it (dogfooding 64 #32).
// ---------------------------------------------------------------------------

#[test]
fn screenshot_viewport_height_flag_is_gone() {
    let output = std::process::Command::new(ff_rdp_bin())
        .args(["screenshot", "--viewport-height", "1000"])
        .output()
        .expect("failed to spawn ff-rdp");

    assert!(
        !output.status.success(),
        "--viewport-height must be rejected as an unknown flag"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("unexpected argument"),
        "expected clap's unknown-argument error, got: {stderr} ({})",
        support::output_note(&output)
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
        support::output_note(&output)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.trim().ends_with("jq_test.png\""),
        "jq filter should return the path string: {stdout}"
    );

    let _ = std::fs::remove_dir_all(&out_dir);
}

// ---------------------------------------------------------------------------
// iter-133 Theme B — `screenshot --window-size` batch capture
// ---------------------------------------------------------------------------

/// AC: `e2e_help_viewport_pointers` — `screenshot --help` documents
/// `--window-size` (the batch capture path) and the headless `resizeTo()`
/// no-op, without needing Firefox installed.
#[test]
fn screenshot_help_mentions_window_size_and_resize_to() {
    let output = std::process::Command::new(ff_rdp_bin())
        .args(["screenshot", "--help"])
        .output()
        .expect("failed to spawn ff-rdp");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("window-size"),
        "screenshot --help must document --window-size: {stdout}"
    );
    assert!(
        stdout.contains("resizeTo"),
        "screenshot --help must document the headless resizeTo() no-op: {stdout}"
    );
}

/// iter-296: `--window-size` composes with `--full-page` (a width-only batch
/// capture). clap no longer rejects the pair; with no Firefox listening the
/// command gets as far as connecting and fails with the JSON envelope.
#[test]
fn screenshot_window_size_composes_with_full_page() {
    let port = {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.local_addr().unwrap().port()
    };
    let home = tempfile::tempdir().unwrap();
    let output = std::process::Command::new(ff_rdp_bin())
        .env("FF_RDP_HOME", home.path())
        .args(["--host", "127.0.0.1", "--port", &port.to_string()])
        .args(["--timeout", "500"])
        .args(["screenshot", "--window-size", "390x844", "--full-page"])
        .output()
        .expect("failed to spawn ff-rdp");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("cannot be used with"),
        "the pair must parse, got: {stderr} ({})",
        support::output_note(&output)
    );
    assert_ne!(
        output.status.code(),
        Some(2),
        "{}",
        support::output_note(&output)
    );
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap_or_else(|e| {
        panic!(
            "JSON envelope expected ({e}): {}",
            support::output_note(&output)
        )
    });
    assert!(json.get("error").is_some(), "{json}");
}

/// `screenshot` has no `--dppx` flag — it was dropped (see the `window_size`
/// help text): `layout.css.devPixelsPerPx` was found to have zero effect on
/// the batch-capture raster (Firefox 153.0.3). An unknown-flag error must
/// name `--dppx`, not silently accept it.
#[test]
fn screenshot_has_no_dppx_flag() {
    let output = std::process::Command::new(ff_rdp_bin())
        .args(["screenshot", "--window-size", "390x844", "--dppx", "2"])
        .output()
        .expect("failed to spawn ff-rdp");
    assert!(
        !output.status.success(),
        "--dppx must be rejected as an unknown flag on screenshot"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("dppx") || stderr.contains("unexpected argument"),
        "expected an unknown-flag error naming --dppx: {stderr} ({})",
        support::output_note(&output)
    );
}

/// A malformed `--window-size` value is rejected with a user error naming
/// the expected `WxH` form — before any RDP connection is attempted, so
/// this needs no Firefox (port 1 is unroutable).
#[test]
fn screenshot_window_size_invalid_rejected() {
    let output = std::process::Command::new(ff_rdp_bin())
        .args([
            "--host",
            "127.0.0.1",
            "--port",
            "1",
            "screenshot",
            "--window-size",
            "not-a-size",
        ])
        .output()
        .expect("failed to spawn ff-rdp");
    assert!(
        !output.status.success(),
        "expected failure for an invalid --window-size"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let combined = format!("{stderr}{stdout}");
    assert!(
        combined.contains("WxH"),
        "error must name the expected WxH form; stderr={stderr:?} stdout={stdout:?}"
    );
}

// ---------------------------------------------------------------------------
// --color-scheme / --media (per-command emulation)
// ---------------------------------------------------------------------------

/// `screenshot_server` plus the emulation prelude: `getWatcher` →
/// `getTargetConfigurationActor` → `updateConfiguration` (replying with
/// `update_fixture`) → `evaluateJSAsync` checks (matchMedia, then animations settled), all true.
fn emulation_server(update_fixture: &str) -> MockRdpServer {
    screenshot_server()
        .on("getWatcher", load_fixture("get_watcher_response.json"))
        .on(
            "getTargetConfigurationActor",
            load_fixture("get_target_configuration_actor_response.json"),
        )
        .on("updateConfiguration", load_fixture(update_fixture))
        .on_with_followup(
            "evaluateJSAsync",
            load_fixture("eval_immediate_response.json"),
            load_fixture("eval_result_wait_true.json"),
        )
}

/// Run `screenshot --base64 <extra...>` against `server`, returning the
/// output and every request the CLI sent.
fn run_emulated(
    server: MockRdpServer,
    extra: &[&str],
) -> (std::process::Output, Vec<serde_json::Value>) {
    let port = server.port();
    let log = server.request_log();
    let handle = std::thread::spawn(move || server.serve_one());
    let mut args = base_args(port);
    args.extend(["screenshot".to_owned(), "--base64".to_owned()]);
    args.extend(extra.iter().map(|s| (*s).to_owned()));
    let output = std::process::Command::new(ff_rdp_bin())
        .args(&args)
        .output()
        .expect("failed to spawn ff-rdp");
    handle.join().unwrap();
    let requests = log.lock().unwrap().clone();
    (output, requests)
}

fn sent_configuration(requests: &[serde_json::Value]) -> Vec<serde_json::Value> {
    requests
        .iter()
        .filter(|r| r["type"] == "updateConfiguration")
        .map(|r| r["configuration"].clone())
        .collect()
}

#[test]
fn screenshot_color_scheme_dark_applies_before_capture() {
    let (output, requests) = run_emulated(
        emulation_server("update_configuration_color_scheme_response.json"),
        &["--color-scheme", "dark"],
    );
    assert!(
        output.status.success(),
        "expected success: {}",
        support::output_note(&output)
    );
    assert_eq!(
        sent_configuration(&requests),
        vec![serde_json::json!({"colorSchemeSimulation": "dark"})]
    );
    let types: Vec<&str> = requests.iter().filter_map(|r| r["type"].as_str()).collect();
    let update = types
        .iter()
        .position(|t| *t == "updateConfiguration")
        .unwrap_or_else(|| panic!("no updateConfiguration: {types:?}"));
    let capture = types
        .iter()
        .position(|t| *t == "capture")
        .unwrap_or_else(|| panic!("no capture: {types:?}"));
    assert!(
        update < capture,
        "the setting must be applied before the capture: {types:?}"
    );
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        json["results"]["emulation"],
        serde_json::json!({"color_scheme": "dark", "settled": true})
    );
}

#[test]
fn screenshot_media_print_applies_print_simulation() {
    let (output, requests) = run_emulated(
        emulation_server("update_configuration_print_response.json"),
        &["--media", "print"],
    );
    assert!(
        output.status.success(),
        "expected success: {}",
        support::output_note(&output)
    );
    assert_eq!(
        sent_configuration(&requests),
        vec![serde_json::json!({"printSimulationEnabled": true})]
    );
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        json["results"]["emulation"],
        serde_json::json!({"media": "print", "settled": true})
    );
}

/// `--media screen` is the unemulated default: no configuration round trip.
#[test]
fn screenshot_media_screen_sends_no_configuration() {
    let (output, requests) = run_emulated(screenshot_server(), &["--media", "screen"]);
    assert!(
        output.status.success(),
        "expected success: {}",
        support::output_note(&output)
    );
    assert!(sent_configuration(&requests).is_empty());
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        json["results"]["emulation"],
        serde_json::json!({"media": "screen"})
    );
}

/// Firefox echoes only the options it applied; a reply that lacks the one
/// sent fails the command instead of capturing an unemulated page.
#[test]
fn screenshot_emulation_fails_when_firefox_drops_the_setting() {
    let (output, _) = run_emulated(
        emulation_server("update_configuration_color_scheme_response.json"),
        &["--media", "print"],
    );
    assert!(!output.status.success(), "a dropped setting must fail");
    let note = support::output_note(&output);
    assert!(note.contains("printSimulationEnabled"), "{note}");
}

/// Both refusals happen before any connection (port 1 is unroutable).
#[test]
fn screenshot_emulation_refuses_impossible_combinations() {
    for (extra, needle) in [
        (
            &["--color-scheme", "dark", "--media", "print"][..],
            "light colour scheme",
        ),
        (
            &["--color-scheme", "dark", "--window-size", "400x300"][..],
            "--window-size",
        ),
    ] {
        let output = std::process::Command::new(ff_rdp_bin())
            .args(["--host", "127.0.0.1", "--port", "1", "screenshot"])
            .args(extra)
            .output()
            .expect("failed to spawn ff-rdp");
        assert!(!output.status.success(), "{extra:?} must be refused");
        let note = support::output_note(&output);
        assert!(note.contains(needle), "{extra:?}: {note}");
    }
}

// ---------------------------------------------------------------------------
// --viewport WxH (iframe wrapper on the live tab)
// ---------------------------------------------------------------------------

/// A white `width`x`height` PNG as a data URL: the capture reply for a mock
/// `--viewport` run (the CLI checks the PNG matches the requested size).
fn png_data_url(width: u32, height: u32) -> String {
    let mut bytes = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut bytes, width, height);
        encoder.set_color(png::ColorType::Rgb);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().expect("png header");
        writer
            .write_image_data(&vec![255u8; (width * height * 3) as usize])
            .expect("png data");
    }
    format!(
        "data:image/png;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(bytes)
    )
}

/// The screenshot flow (its `capture` replying with `png`) plus the three
/// recorded `evaluateJSAsync` replies of a `--viewport` capture: inject →
/// poll (loaded, images settled) → cleanup.
///
/// The recorder normalises actor ids to `child0`, while
/// `get_target_response.json` hands out `child2/consoleActor3`; the mock only
/// rewrites `from` on replies, not on `evaluationResult` events, so the test
/// addresses each event to the console actor the client was given.
fn viewport_server(png: &str) -> MockRdpServer {
    let console = load_fixture("get_target_response.json")["frame"]["consoleActor"].clone();
    assert!(
        console.is_string(),
        "consoleActor in get_target_response.json"
    );
    let result = |name: &str| {
        let mut event = load_fixture(name);
        event["from"] = console.clone();
        vec![event]
    };
    let immediate = load_fixture("eval_immediate_response_location_href.json");
    screenshot_server_capturing(png).on_sequence(
        "evaluateJSAsync",
        vec![
            // The consent-overlay probe runs first; any non-probe reply (here
            // the recorded cleanup's `1`) reads as "no banner".
            (
                immediate.clone(),
                result("eval_result_viewport_cleanup.json"),
            ),
            (
                immediate.clone(),
                result("eval_result_viewport_inject.json"),
            ),
            (
                immediate.clone(),
                result("eval_result_viewport_poll_loaded.json"),
            ),
            (immediate, result("eval_result_viewport_cleanup.json")),
        ],
    )
}

#[test]
fn screenshot_viewport_captures_the_wrapper_rect_and_removes_it() {
    let (output, requests) = run_emulated(
        viewport_server(&png_data_url(390, 844)),
        &["--viewport", "390x844"],
    );
    assert!(output.status.success(), "{}", support::output_note(&output));
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).expect("JSON");
    let results = &json["results"];
    assert_eq!(results["capture"], "live-viewport-iframe", "{json}");
    assert_eq!(results["viewport"]["width"], 390, "{json}");
    assert_eq!(results["viewport"]["height"], 844, "{json}");
    assert_eq!(results["url"], "https://example.com/", "{json}");
    assert_eq!(results["images_settled"], true, "{json}");

    let capture = requests
        .iter()
        .find(|r| r["type"] == "capture")
        .expect("a capture request");
    let rect = &capture["args"]["rect"];
    assert_eq!(rect["width"], 390.0, "{capture}");
    assert_eq!(rect["height"], 844.0, "{capture}");
    assert_eq!(rect["left"], 0.0, "{capture}");
    assert_eq!(capture["args"]["fullpage"], false, "{capture}");

    // overlay probe, inject, poll, cleanup — the cleanup comes after the capture.
    let types: Vec<&str> = requests.iter().filter_map(|r| r["type"].as_str()).collect();
    let evals: Vec<usize> = types
        .iter()
        .enumerate()
        .filter(|(_, t)| **t == "evaluateJSAsync")
        .map(|(i, _)| i)
        .collect();
    let capture_at = types.iter().position(|t| *t == "capture").expect("capture");
    assert_eq!(evals.len(), 4, "{types:?}");
    let probe = requests[evals[0]]["text"].as_str().unwrap_or_default();
    assert!(probe.contains("ff-rdp:consent-overlay-probe"), "{probe}");
    assert!(evals[2] < capture_at && capture_at < evals[3], "{types:?}");
    let cleanup = requests[evals[3]]["text"].as_str().unwrap_or_default();
    assert!(cleanup.contains("data-ffrdp-viewport"), "{cleanup}");
}

/// A Firefox that ignores the capture rect returns the whole window; that must
/// not be labelled a 390x844 capture, and the wrapper is still removed.
#[test]
fn screenshot_viewport_refuses_a_capture_of_the_wrong_size() {
    let (output, requests) = run_emulated(
        viewport_server(&png_data_url(1366, 683)),
        &["--viewport", "390x844"],
    );
    assert!(!output.status.success(), "must fail");
    let note = support::output_note(&output);
    assert!(note.contains("1366x683"), "{note}");
    assert!(note.contains("does not honour the capture rect"), "{note}");
    let last_eval = requests
        .iter()
        .rev()
        .find(|r| r["type"] == "evaluateJSAsync")
        .expect("an eval");
    let text = last_eval["text"].as_str().unwrap_or_default();
    assert!(text.contains("e.remove()"), "the cleanup runs last: {text}");
}

/// Argument errors are reported before any connection (port 1 is unroutable).
#[test]
fn screenshot_viewport_rejects_bad_values_and_conflicts() {
    for (extra, needle) in [
        (&["--viewport", "390"][..], "WxH"),
        (&["--viewport", "0x844"][..], "between 1 and"),
        (
            &["--viewport", "390x844", "--window-size", "390x844"][..],
            "--window-size",
        ),
        (&["--viewport", "390x844", "--full-page"][..], "--full-page"),
    ] {
        let output = std::process::Command::new(ff_rdp_bin())
            .args(["--host", "127.0.0.1", "--port", "1", "screenshot"])
            .args(extra)
            .output()
            .expect("failed to spawn ff-rdp");
        assert!(!output.status.success(), "{extra:?} must be refused");
        let note = support::output_note(&output);
        assert!(note.contains(needle), "{extra:?}: {note}");
    }
}

#[test]
fn screenshot_help_documents_viewport_limits() {
    let output = std::process::Command::new(ff_rdp_bin())
        .args(["screenshot", "--help"])
        .output()
        .expect("failed to spawn ff-rdp");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("--viewport <WxH>"), "{stdout}");
    assert!(stdout.contains("NOT"), "{stdout}");
    assert!(stdout.contains("X-Frame-Options"), "{stdout}");
}

// ---------------------------------------------------------------------------
// Consent-overlay warning (2026-10-26 consent follow-up)
// ---------------------------------------------------------------------------

/// A screenshot of a page a known consent banner covers still succeeds, and
/// says so: `results.warnings[]` names the CMP and `ff-rdp consent accept`,
/// and that command leads the hints.
#[test]
fn screenshot_warns_and_hints_when_a_consent_banner_covers_the_page() {
    // The recorded string `evaluationResult`, its value swapped for the
    // probe's JSON reading (the reply shape is the recorded one).
    let mut probe = load_fixture("eval_result_string.json");
    probe["result"] = serde_json::Value::String(
        serde_json::json!({"cmp": "consentmanager", "visible": true, "scroll_locked": false})
            .to_string(),
    );
    let server = screenshot_server().on_with_followup(
        "evaluateJSAsync",
        load_fixture("eval_immediate_response.json"),
        probe,
    );
    let (output, requests) = run_emulated(server, &["--hints"]);
    assert!(output.status.success(), "{}", support::output_note(&output));
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).expect("JSON");
    let warning = &json["results"]["warnings"][0];
    assert_eq!(warning["type"], "consent_overlay", "{json}");
    assert_eq!(warning["consent_overlay"], "consentmanager", "{json}");
    assert_eq!(warning["visible"], true, "{json}");
    assert_eq!(warning["hint"], "ff-rdp consent accept", "{json}");
    assert_eq!(json["hints"][0]["cmd"], "ff-rdp consent accept", "{json}");
    assert_eq!(json["results"]["width"], 1, "the capture still ran: {json}");
    // One probe, before the capture.
    let evals: Vec<_> = requests
        .iter()
        .filter(|r| r["type"] == "evaluateJSAsync")
        .collect();
    assert_eq!(evals.len(), 1, "{requests:?}");
    assert!(
        evals[0]["text"]
            .as_str()
            .is_some_and(|t| t.contains("ff-rdp:consent-overlay-probe")),
        "{requests:?}"
    );
}

/// When the probe cannot run (here the mock has no `evaluateJSAsync`), the
/// screenshot is unaffected: no warning, no failure.
#[test]
fn screenshot_without_a_probe_reply_adds_no_warning() {
    let (output, _) = run_emulated(screenshot_server(), &[]);
    assert!(output.status.success(), "{}", support::output_note(&output));
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).expect("JSON");
    assert!(json["results"].get("warnings").is_none(), "{json}");
}
