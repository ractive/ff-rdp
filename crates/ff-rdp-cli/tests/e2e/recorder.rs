//! End-to-end tests for the `record` command.
//!
//! These tests exercise the full record-start → command → record-stop pipeline
//! using the real CLI binary against a mock RDP server so they can assert on
//! the on-disk recorded JSON without needing a live Firefox instance.
//!
//! Each test sets `XDG_STATE_HOME` to a unique temp directory so recording
//! state files don't bleed between tests or into the developer's real state.

use std::path::PathBuf;

use super::support::{MockRdpServer, load_fixture};

fn ff_rdp_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_ff-rdp"))
}

/// Create a unique temp directory for one test's state and output files.
///
/// Returns `(state_dir, output_path)`.  The state dir is passed as
/// `XDG_STATE_HOME` so the recording state file lives there instead of in the
/// developer's real `~/.local/state/ff-rdp/`.
fn temp_recording_env(label: &str) -> (PathBuf, PathBuf) {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let state_dir = std::env::temp_dir().join(format!("ff_rdp_rec_{label}_{ts}_state"));
    let output_path = std::env::temp_dir().join(format!("ff_rdp_rec_{label}_{ts}.json"));
    std::fs::create_dir_all(&state_dir).expect("create state dir");
    (state_dir, output_path)
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

/// Run a command with `XDG_STATE_HOME` set to `state_dir`.
///
/// Returns the `std::process::Output` of the command.
fn run_with_state(args: &[String], state_dir: &PathBuf) -> std::process::Output {
    std::process::Command::new(ff_rdp_bin())
        .args(args)
        .env("XDG_STATE_HOME", state_dir)
        .output()
        .expect("failed to spawn ff-rdp")
}

/// Build a mock server that responds to a single `wait --selector` poll
/// with `true` so the wait succeeds immediately.
fn wait_true_server() -> MockRdpServer {
    MockRdpServer::new()
        .on("listTabs", load_fixture("list_tabs_response.json"))
        .on("getTarget", load_fixture("get_target_response.json"))
        .on_with_followup(
            "evaluateJSAsync",
            load_fixture("eval_immediate_response.json"),
            load_fixture("eval_result_wait_true.json"),
        )
}

// ---------------------------------------------------------------------------
// Theme A: recorder captures --wait-timeout
// ---------------------------------------------------------------------------

/// Recording `wait --selector body --wait-timeout 5000` (the default value)
/// must NOT write the `timeout` field — the default is elided to keep the
/// recorded file terse.  See `recorder_captures_nondefault_wait_timeout` for
/// the positive case where a non-default value IS written.
#[test]
fn recorder_elides_explicit_default_wait_timeout() {
    let server = wait_true_server();
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let (state_dir, output_path) = temp_recording_env("captures_timeout");

    // record start
    let start_args = vec![
        "record".to_owned(),
        "start".to_owned(),
        output_path.to_string_lossy().into_owned(),
    ];
    let start_out = run_with_state(&start_args, &state_dir);
    // record start does not need a Firefox connection
    assert!(
        start_out.status.success(),
        "record start failed: {}",
        String::from_utf8_lossy(&start_out.stderr)
    );

    // wait --selector body --wait-timeout 5000
    let mut wait_args = base_args(port);
    wait_args.extend([
        "wait".to_owned(),
        "--selector".to_owned(),
        "body".to_owned(),
        "--wait-timeout".to_owned(),
        "5000".to_owned(),
    ]);
    let wait_out = run_with_state(&wait_args, &state_dir);
    assert!(
        wait_out.status.success(),
        "wait command failed: {}",
        String::from_utf8_lossy(&wait_out.stderr)
    );

    handle.join().unwrap();

    // record stop
    let stop_args: Vec<String> = vec!["record".to_owned(), "stop".to_owned()];
    // stop doesn't need --port
    let stop_out = run_with_state(&stop_args, &state_dir);
    assert!(
        stop_out.status.success(),
        "record stop failed: {}",
        String::from_utf8_lossy(&stop_out.stderr)
    );

    // Parse the recorded file and assert `timeout: 5000` is present.
    // The default wait timeout is 5000 ms.
    // NOTE: the plan says "record `--timeout 5000`" and asserts `timeout: 5000`
    // in the step.  The recorder elides the timeout when it equals the 5000 ms
    // default, so we use a NON-default value in the test that asserts presence.
    // This test uses the default (5000) to document the elision behaviour — see
    // `recorder_captures_nondefault_wait_timeout` for the positive assertion.
    let content = std::fs::read_to_string(&output_path).expect("recorded file must exist");
    let parsed: serde_json::Value =
        serde_json::from_str(&content).expect("recorded file must be valid JSON");

    let steps = parsed["steps"].as_array().expect("steps array");
    assert_eq!(steps.len(), 1, "expected exactly 1 recorded step");

    let step = &steps[0];
    assert!(step["wait"].is_object(), "step must be a wait step: {step}");
    assert_eq!(
        step["wait"]["selector"], "body",
        "wait selector must be recorded"
    );

    // Default timeout (5000) is ELIDED to keep recorded files terse.
    assert!(
        step["wait"]["timeout"].is_null()
            || !step["wait"].as_object().unwrap().contains_key("timeout"),
        "default timeout (5000) must NOT be written to keep the file terse; got: {step}"
    );

    let _ = std::fs::remove_file(&output_path);
}

/// Recording `wait --selector body --wait-timeout 1234` (non-default) must
/// produce a step with `"timeout": 1234` in the recorded JSON.
#[test]
fn recorder_captures_nondefault_wait_timeout() {
    let server = wait_true_server();
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let (state_dir, output_path) = temp_recording_env("nondefault_timeout");

    // record start
    let start_args = vec![
        "record".to_owned(),
        "start".to_owned(),
        output_path.to_string_lossy().into_owned(),
    ];
    let start_out = run_with_state(&start_args, &state_dir);
    assert!(
        start_out.status.success(),
        "record start failed: {}",
        String::from_utf8_lossy(&start_out.stderr)
    );

    // wait --selector body --wait-timeout 1234  (non-default)
    let mut wait_args = base_args(port);
    wait_args.extend([
        "wait".to_owned(),
        "--selector".to_owned(),
        "body".to_owned(),
        "--wait-timeout".to_owned(),
        "1234".to_owned(),
    ]);
    let wait_out = run_with_state(&wait_args, &state_dir);
    assert!(
        wait_out.status.success(),
        "wait command failed: {}",
        String::from_utf8_lossy(&wait_out.stderr)
    );

    handle.join().unwrap();

    // record stop
    let stop_args = vec!["record".to_owned(), "stop".to_owned()];
    let stop_out = run_with_state(&stop_args, &state_dir);
    assert!(
        stop_out.status.success(),
        "record stop failed: {}",
        String::from_utf8_lossy(&stop_out.stderr)
    );

    let content = std::fs::read_to_string(&output_path).expect("recorded file must exist");
    let parsed: serde_json::Value =
        serde_json::from_str(&content).expect("recorded file must be valid JSON");

    let steps = parsed["steps"].as_array().expect("steps array");
    assert_eq!(steps.len(), 1, "expected exactly 1 recorded step");

    let step = &steps[0];
    assert_eq!(step["wait"]["selector"], "body");
    assert_eq!(
        step["wait"]["timeout"],
        serde_json::json!(1234),
        "non-default timeout must be recorded; got: {step}"
    );

    let _ = std::fs::remove_file(&output_path);
}

/// Recording `wait --selector body` with no explicit `--wait-timeout` must
/// produce a step WITHOUT a `timeout` field (default is elided).
#[test]
fn recorder_omits_default_timeout() {
    let server = wait_true_server();
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let (state_dir, output_path) = temp_recording_env("omit_timeout");

    // record start
    let start_args = vec![
        "record".to_owned(),
        "start".to_owned(),
        output_path.to_string_lossy().into_owned(),
    ];
    let start_out = run_with_state(&start_args, &state_dir);
    assert!(
        start_out.status.success(),
        "record start failed: {}",
        String::from_utf8_lossy(&start_out.stderr)
    );

    // wait --selector body  (no --wait-timeout, default of 5000 applies)
    let mut wait_args = base_args(port);
    wait_args.extend([
        "wait".to_owned(),
        "--selector".to_owned(),
        "body".to_owned(),
    ]);
    let wait_out = run_with_state(&wait_args, &state_dir);
    assert!(
        wait_out.status.success(),
        "wait command failed: {}",
        String::from_utf8_lossy(&wait_out.stderr)
    );

    handle.join().unwrap();

    // record stop
    let stop_args = vec!["record".to_owned(), "stop".to_owned()];
    let stop_out = run_with_state(&stop_args, &state_dir);
    assert!(
        stop_out.status.success(),
        "record stop failed: {}",
        String::from_utf8_lossy(&stop_out.stderr)
    );

    let content = std::fs::read_to_string(&output_path).expect("recorded file must exist");
    let parsed: serde_json::Value =
        serde_json::from_str(&content).expect("recorded file must be valid JSON");

    let steps = parsed["steps"].as_array().expect("steps array");
    assert_eq!(steps.len(), 1, "expected exactly 1 recorded step");

    let step = &steps[0];
    assert_eq!(step["wait"]["selector"], "body");

    let has_timeout = step["wait"]
        .as_object()
        .is_some_and(|m| m.contains_key("timeout"));
    assert!(
        !has_timeout,
        "default timeout must be omitted from the recorded step; got: {step}"
    );

    let _ = std::fs::remove_file(&output_path);
}

/// Recording `wait --text Loaded --wait-timeout 2500` must preserve the
/// non-default timeout in the recorded step alongside the `text` condition.
#[test]
fn recorder_captures_nondefault_timeout_for_text_wait() {
    let server = wait_true_server();
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let (state_dir, output_path) = temp_recording_env("text_timeout");

    let start_args = vec![
        "record".to_owned(),
        "start".to_owned(),
        output_path.to_string_lossy().into_owned(),
    ];
    let start_out = run_with_state(&start_args, &state_dir);
    assert!(start_out.status.success());

    let mut wait_args = base_args(port);
    wait_args.extend([
        "wait".to_owned(),
        "--text".to_owned(),
        "Loaded".to_owned(),
        "--wait-timeout".to_owned(),
        "2500".to_owned(),
    ]);
    let wait_out = run_with_state(&wait_args, &state_dir);
    assert!(
        wait_out.status.success(),
        "wait --text failed: {}",
        String::from_utf8_lossy(&wait_out.stderr)
    );
    handle.join().unwrap();

    let stop_args = vec!["record".to_owned(), "stop".to_owned()];
    let stop_out = run_with_state(&stop_args, &state_dir);
    assert!(stop_out.status.success());

    let content = std::fs::read_to_string(&output_path).expect("recorded file must exist");
    let parsed: serde_json::Value = serde_json::from_str(&content).expect("valid JSON");
    let step = &parsed["steps"][0];
    assert_eq!(step["wait"]["text"], "Loaded");
    assert_eq!(step["wait"]["timeout"], serde_json::json!(2500));

    let _ = std::fs::remove_file(&output_path);
}

/// Recording `wait --eval EXPR --wait-timeout 7500` must preserve the
/// non-default timeout in the recorded step alongside the `eval` condition.
#[test]
fn recorder_captures_nondefault_timeout_for_eval_wait() {
    let server = wait_true_server();
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let (state_dir, output_path) = temp_recording_env("eval_timeout");

    let start_args = vec![
        "record".to_owned(),
        "start".to_owned(),
        output_path.to_string_lossy().into_owned(),
    ];
    let start_out = run_with_state(&start_args, &state_dir);
    assert!(start_out.status.success());

    let mut wait_args = base_args(port);
    wait_args.extend([
        "wait".to_owned(),
        "--eval".to_owned(),
        "document.readyState === 'complete'".to_owned(),
        "--wait-timeout".to_owned(),
        "7500".to_owned(),
    ]);
    let wait_out = run_with_state(&wait_args, &state_dir);
    assert!(
        wait_out.status.success(),
        "wait --eval failed: {}",
        String::from_utf8_lossy(&wait_out.stderr)
    );
    handle.join().unwrap();

    let stop_args = vec!["record".to_owned(), "stop".to_owned()];
    let stop_out = run_with_state(&stop_args, &state_dir);
    assert!(stop_out.status.success());

    let content = std::fs::read_to_string(&output_path).expect("recorded file must exist");
    let parsed: serde_json::Value = serde_json::from_str(&content).expect("valid JSON");
    let step = &parsed["steps"][0];
    assert_eq!(step["wait"]["eval"], "document.readyState === 'complete'");
    assert_eq!(step["wait"]["timeout"], serde_json::json!(7500));

    let _ = std::fs::remove_file(&output_path);
}

// ---------------------------------------------------------------------------
// Theme C: closing-bracket formatting
// ---------------------------------------------------------------------------

/// A 2-step recorded file must end with `}\n  ]\n}\n` — the same suffix that
/// `serde_json::to_string_pretty` produces, with a newline before the `]`.
#[test]
fn recorder_two_step_file_ends_with_correct_closing() {
    // Use two sequential servers (one per wait step).
    let server1 = wait_true_server();
    let port1 = server1.port();
    let handle1 = std::thread::spawn(move || server1.serve_one());

    let server2 = wait_true_server();
    let port2 = server2.port();
    let handle2 = std::thread::spawn(move || server2.serve_one());

    let (state_dir, output_path) = temp_recording_env("closing_bracket");

    // record start
    let start_args = vec![
        "record".to_owned(),
        "start".to_owned(),
        output_path.to_string_lossy().into_owned(),
    ];
    let start_out = run_with_state(&start_args, &state_dir);
    assert!(
        start_out.status.success(),
        "record start failed: {}",
        String::from_utf8_lossy(&start_out.stderr)
    );

    // Step 1
    let mut args1 = base_args(port1);
    args1.extend([
        "wait".to_owned(),
        "--selector".to_owned(),
        "body".to_owned(),
    ]);
    let out1 = run_with_state(&args1, &state_dir);
    assert!(
        out1.status.success(),
        "wait 1 failed: {}",
        String::from_utf8_lossy(&out1.stderr)
    );
    handle1.join().unwrap();

    // Step 2
    let mut args2 = base_args(port2);
    args2.extend([
        "wait".to_owned(),
        "--selector".to_owned(),
        ".content".to_owned(),
    ]);
    let out2 = run_with_state(&args2, &state_dir);
    assert!(
        out2.status.success(),
        "wait 2 failed: {}",
        String::from_utf8_lossy(&out2.stderr)
    );
    handle2.join().unwrap();

    // record stop
    let stop_args = vec!["record".to_owned(), "stop".to_owned()];
    let stop_out = run_with_state(&stop_args, &state_dir);
    assert!(
        stop_out.status.success(),
        "record stop failed: {}",
        String::from_utf8_lossy(&stop_out.stderr)
    );

    let content = std::fs::read_to_string(&output_path).expect("recorded file must exist");

    // Must be valid JSON.
    let parsed: serde_json::Value =
        serde_json::from_str(&content).expect("recorded file must be valid JSON");
    assert_eq!(parsed["steps"].as_array().unwrap().len(), 2);

    // Must end with the correct closing sequence: last step's `}`, then
    // a newline, then `  ]`, then `\n}\n`.
    assert!(
        content.ends_with("}\n  ]\n}\n"),
        "2-step file must end with `}}\\n  ]\\n}}\\n`; actual tail: {:?}",
        &content[content.len().saturating_sub(30)..]
    );

    let _ = std::fs::remove_file(&output_path);
}
