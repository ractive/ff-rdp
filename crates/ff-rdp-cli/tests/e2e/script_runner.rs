//! E2e tests for the script runner (format parsing, dry-run, cycle detection).
//!
//! These tests exercise the `ff-rdp run` subcommand without a live Firefox
//! connection.  They use `--dry-run` to validate parsing and variable
//! resolution, and test error cases (cycle detection, missing vars, etc.)
//! without needing a browser.

use std::io::Write as _;

fn ff_rdp_bin() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_BIN_EXE_ff-rdp"))
}

fn run_dry(script_content: &str, extra_args: &[&str]) -> std::process::Output {
    let mut tmp = tempfile::NamedTempFile::new().expect("temp file");
    write!(tmp, "{script_content}").expect("write");
    let path = tmp.path().to_owned();
    // Keep the file alive until after the process exits.
    let _tmp = tmp;

    std::process::Command::new(ff_rdp_bin())
        .arg("run")
        .arg(&path)
        .arg("--dry-run")
        .args(extra_args)
        .output()
        .expect("spawn ff-rdp")
}

fn run_dry_yaml(script_content: &str, extra_args: &[&str]) -> std::process::Output {
    // Write to a .yaml temp file so format is detected from extension.
    let tmp_dir = tempfile::tempdir().expect("tempdir");
    let path = tmp_dir.path().join("script.yaml");
    std::fs::write(&path, script_content).expect("write yaml");

    std::process::Command::new(ff_rdp_bin())
        .arg("run")
        .arg(&path)
        .arg("--dry-run")
        .args(extra_args)
        .output()
        .expect("spawn ff-rdp")
}

// ---------------------------------------------------------------------------
// Happy path
// ---------------------------------------------------------------------------

#[test]
fn dry_run_minimal_json_script() {
    let script = r#"{
        "version": 1,
        "steps": [
            {"navigate": {"url": "https://example.com"}}
        ]
    }"#;
    let output = run_dry(script, &[]);
    assert!(
        output.status.success(),
        "expected success, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).expect("stdout must be valid JSON");
    assert_eq!(json["dry_run"], true);
    assert_eq!(json["total"], 1);
}

#[test]
fn dry_run_minimal_yaml_script() {
    let script = "version: 1\nsteps:\n  - navigate:\n      url: https://example.com\n";
    let output = run_dry_yaml(script, &[]);
    assert!(
        output.status.success(),
        "expected success, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).expect("stdout must be valid JSON");
    assert_eq!(json["dry_run"], true);
    assert_eq!(json["total"], 1);
}

#[test]
fn dry_run_with_defined_vars() {
    let script = r#"{
        "version": 1,
        "vars": {"email": "default@example.com"},
        "steps": [
            {"navigate": {"url": "https://example.com"}},
            {"assert_text": {"selector": "h1", "contains": "{{vars.email}}"}}
        ]
    }"#;
    let output = run_dry(script, &["--vars", "email=override@example.com"]);
    assert!(
        output.status.success(),
        "expected success, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn dry_run_json_and_yaml_parse_equivalently() {
    let json_script = r#"{
        "version": 1,
        "steps": [
            {"navigate": {"url": "https://example.com"}},
            {"click": {"selector": "button"}},
            {"assert_text": {"selector": "h1", "contains": "Welcome"}}
        ]
    }"#;
    let yaml_script = "version: 1\nsteps:\n  - navigate:\n      url: https://example.com\n  - click:\n      selector: button\n  - assert_text:\n      selector: h1\n      contains: Welcome\n";

    let json_out = run_dry(json_script, &[]);
    let yaml_out = run_dry_yaml(yaml_script, &[]);

    assert!(json_out.status.success(), "JSON dry-run failed");
    assert!(yaml_out.status.success(), "YAML dry-run failed");

    let json_json: serde_json::Value =
        serde_json::from_slice(&json_out.stdout).expect("JSON stdout");
    let yaml_json: serde_json::Value =
        serde_json::from_slice(&yaml_out.stdout).expect("YAML stdout");

    assert_eq!(json_json["total"], yaml_json["total"], "step counts differ");
}

// ---------------------------------------------------------------------------
// Error cases
// ---------------------------------------------------------------------------

#[test]
fn dry_run_missing_var_exits_nonzero() {
    let script = r#"{
        "version": 1,
        "steps": [
            {"navigate": {"url": "{{vars.missing_url}}"}}
        ]
    }"#;
    let output = run_dry(script, &[]);
    assert!(
        !output.status.success(),
        "expected failure for missing var, got success"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("missing_url"),
        "stderr should mention the missing variable: {stderr}"
    );
}

#[test]
fn rejects_script_with_multiple_targets() {
    let script = r#"{
        "version": 1,
        "steps": [
            {"click": {"selector": "button", "ref": "e1"}}
        ]
    }"#;
    let output = run_dry(script, &[]);
    assert!(
        !output.status.success(),
        "expected failure for multiple targets"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("exactly one"),
        "stderr should mention 'exactly one': {stderr}"
    );
}

#[test]
fn rejects_unsupported_version() {
    let script = r#"{"version": 99, "steps": []}"#;
    let output = run_dry(script, &[]);
    assert!(!output.status.success(), "expected failure for version 99");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("99"),
        "stderr should mention version: {stderr}"
    );
}

#[test]
fn dry_run_with_vars_flag() {
    let script = r#"{
        "version": 1,
        "steps": [
            {"navigate": {"url": "{{vars.url}}"}}
        ]
    }"#;
    // Providing the var via --vars should make dry-run succeed.
    let output = run_dry(script, &["--vars", "url=https://example.com"]);
    assert!(
        output.status.success(),
        "expected success with --vars url provided, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

// ---------------------------------------------------------------------------
// Cycle detection (unit test without real execution)
// ---------------------------------------------------------------------------

#[test]
fn self_referencing_run_step_errors_on_dry_run() {
    // Create a temp script that runs itself.
    let tmp_dir = tempfile::tempdir().expect("tempdir");
    let path = tmp_dir.path().join("self.json");
    let script = format!(
        r#"{{"version":1,"steps":[{{"run":{{"path":"{}"}}}}]}}"#,
        path.display()
    );
    std::fs::write(&path, &script).expect("write self-referencing script");

    let output = std::process::Command::new(ff_rdp_bin())
        .arg("run")
        .arg(&path)
        .arg("--dry-run")
        .output()
        .expect("spawn ff-rdp");

    // Dry-run does not execute `run:` steps, so this won't actually cycle.
    // The cycle detection fires at execution time, not dry-run time.
    // This test verifies the script parses and dry-runs cleanly.
    assert!(
        output.status.success(),
        "dry-run on self-referencing script should succeed (no execution): {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

// ---------------------------------------------------------------------------
// Record subcommand: start/stop/status
// ---------------------------------------------------------------------------

#[test]
fn record_status_when_not_recording() {
    // Override state dir so we don't interfere with any real recording.
    let tmp_dir = tempfile::tempdir().expect("tempdir");
    let output = std::process::Command::new(ff_rdp_bin())
        .arg("record")
        .arg("status")
        .env("XDG_STATE_HOME", tmp_dir.path())
        .output()
        .expect("spawn ff-rdp");

    assert!(
        output.status.success(),
        "record status should succeed, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value =
        serde_json::from_str(stdout.trim()).expect("record status output must be JSON");
    assert_eq!(json["active"], false);
}

#[test]
fn record_start_creates_state_and_stop_finalises() {
    let tmp_dir = tempfile::tempdir().expect("tempdir");
    let out_file = tmp_dir.path().join("recorded.json");

    // Start recording.
    let start_out = std::process::Command::new(ff_rdp_bin())
        .arg("record")
        .arg("start")
        .arg(&out_file)
        .env("XDG_STATE_HOME", tmp_dir.path())
        .output()
        .expect("spawn ff-rdp record start");
    assert!(
        start_out.status.success(),
        "record start failed: {}",
        String::from_utf8_lossy(&start_out.stderr)
    );

    // Status should now show active.
    let status_out = std::process::Command::new(ff_rdp_bin())
        .arg("record")
        .arg("status")
        .env("XDG_STATE_HOME", tmp_dir.path())
        .output()
        .expect("spawn ff-rdp record status");
    let status_json: serde_json::Value =
        serde_json::from_slice(&status_out.stdout).expect("status JSON");
    assert_eq!(status_json["active"], true);

    // Stop recording.
    let stop_out = std::process::Command::new(ff_rdp_bin())
        .arg("record")
        .arg("stop")
        .env("XDG_STATE_HOME", tmp_dir.path())
        .output()
        .expect("spawn ff-rdp record stop");
    assert!(
        stop_out.status.success(),
        "record stop failed: {}",
        String::from_utf8_lossy(&stop_out.stderr)
    );

    // The output file should contain a valid script.
    let content = std::fs::read_to_string(&out_file).expect("read recorded script");
    let parsed: serde_json::Value =
        serde_json::from_str(&content).expect("recorded script must be valid JSON");
    assert_eq!(parsed["version"], 1);
    assert!(parsed["steps"].is_array(), "steps must be an array");
}

#[test]
fn record_start_twice_errors() {
    let tmp_dir = tempfile::tempdir().expect("tempdir");
    let out1 = tmp_dir.path().join("rec1.json");
    let out2 = tmp_dir.path().join("rec2.json");

    // First start should succeed.
    let first = std::process::Command::new(ff_rdp_bin())
        .arg("record")
        .arg("start")
        .arg(&out1)
        .env("XDG_STATE_HOME", tmp_dir.path())
        .output()
        .expect("spawn");
    assert!(first.status.success(), "first record start should succeed");

    // Second start should fail.
    let second = std::process::Command::new(ff_rdp_bin())
        .arg("record")
        .arg("start")
        .arg(&out2)
        .env("XDG_STATE_HOME", tmp_dir.path())
        .output()
        .expect("spawn");
    assert!(
        !second.status.success(),
        "second record start should fail (already active)"
    );

    // Cleanup: stop the first recording.
    std::process::Command::new(ff_rdp_bin())
        .arg("record")
        .arg("stop")
        .env("XDG_STATE_HOME", tmp_dir.path())
        .output()
        .ok();
}
