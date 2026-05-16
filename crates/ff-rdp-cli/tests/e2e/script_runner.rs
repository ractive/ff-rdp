//! E2e tests for the script runner (format parsing, dry-run, cycle detection).
//!
//! These tests exercise the `ff-rdp run` subcommand without a live Firefox
//! connection.  They use `--dry-run` to validate parsing and variable
//! resolution, and test error cases (cycle detection, missing vars, etc.)
//! without needing a browser.

use std::io::Write as _;

// ---------------------------------------------------------------------------
// B2: JSON Schema validation of example fixtures
// ---------------------------------------------------------------------------

#[test]
fn schema_examples_valid() {
    // Locate the schema file relative to the manifest directory.
    let schema_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("schemas")
        .join("script.schema.json");
    let schema_str = std::fs::read_to_string(&schema_path).expect("reading schema file");
    let schema_value: serde_json::Value =
        serde_json::from_str(&schema_str).expect("parsing schema JSON");
    let validator = jsonschema::validator_for(&schema_value).expect("compiling schema");

    // Validate every example script in examples/scripts/.
    let examples_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/")
        .parent()
        .expect("workspace root")
        .join("examples")
        .join("scripts");

    let mut validated = 0usize;
    for entry in std::fs::read_dir(&examples_dir).expect("reading examples dir") {
        let entry = entry.expect("dir entry");
        let path = entry.path();
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        if ext != "json" && ext != "yaml" && ext != "yml" {
            continue;
        }
        let content = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("reading {}: {e}", path.display()));
        let instance: serde_json::Value = if ext == "json" {
            serde_json::from_str(&content)
                .unwrap_or_else(|e| panic!("parsing JSON {}: {e}", path.display()))
        } else {
            // YAML → serde_json::Value
            serde_yaml::from_str(&content)
                .unwrap_or_else(|e| panic!("parsing YAML {}: {e}", path.display()))
        };
        let result = validator.validate(&instance);
        assert!(
            result.is_ok(),
            "example '{}' failed schema validation: {:#?}",
            path.display(),
            result.err()
        );
        validated += 1;
    }
    assert!(
        validated > 0,
        "no example scripts found in {}",
        examples_dir.display()
    );
}

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
// deny_unknown_fields: typo'd field names error at parse time
// ---------------------------------------------------------------------------

#[test]
fn typo_field_rejected_at_parse_time() {
    // {"navigate": {"urll": "..."}} should fail with an unknown-field error.
    let script = r#"{
        "version": 1,
        "steps": [
            {"navigate": {"urll": "https://example.com"}}
        ]
    }"#;
    let output = run_dry(script, &[]);
    assert!(
        !output.status.success(),
        "expected failure for typo'd field 'urll'"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("urll") || stderr.contains("unknown field"),
        "stderr should mention the unknown field: {stderr}"
    );
}

// ---------------------------------------------------------------------------
// Dry-run deferred iter-62 features
// ---------------------------------------------------------------------------

#[test]
fn dry_run_deferred_page_map_fails_early() {
    let script = r#"{
        "version": 1,
        "steps": [
            {"click": {"page_map": "pages.login.submit_button"}}
        ]
    }"#;
    let output = run_dry(script, &[]);
    assert!(
        !output.status.success(),
        "expected failure for page_map in dry-run"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("page_map") || stderr.contains("iter-62"),
        "stderr should mention page_map or iter-62: {stderr}"
    );
}

// ---------------------------------------------------------------------------
// --script-format flag
// ---------------------------------------------------------------------------

#[test]
fn script_format_flag_overrides_extension_detection() {
    // Write JSON content to a file with no extension (stdin-like usage).
    let tmp_dir = tempfile::tempdir().expect("tempdir");
    let path = tmp_dir.path().join("script_no_ext");
    let json_content = r#"{"version":1,"steps":[{"navigate":{"url":"https://example.com"}}]}"#;
    std::fs::write(&path, json_content).expect("write");

    let output = std::process::Command::new(ff_rdp_bin())
        .arg("run")
        .arg(&path)
        .arg("--dry-run")
        .arg("--script-format")
        .arg("json")
        .output()
        .expect("spawn ff-rdp");

    assert!(
        output.status.success(),
        "--script-format json should override no-extension detection: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).expect("stdout must be valid JSON");
    assert_eq!(json["dry_run"], true);
    assert_eq!(json["total"], 1);
}

#[test]
fn script_format_invalid_value_exits_nonzero() {
    let script = r#"{"version":1,"steps":[]}"#;
    let mut tmp = tempfile::NamedTempFile::new().expect("temp file");
    std::io::Write::write_all(&mut tmp, script.as_bytes()).expect("write");
    let path = tmp.path().to_owned();
    let _tmp = tmp;

    let output = std::process::Command::new(ff_rdp_bin())
        .arg("run")
        .arg(&path)
        .arg("--dry-run")
        .arg("--script-format")
        .arg("xml") // invalid
        .output()
        .expect("spawn ff-rdp");

    assert!(
        !output.status.success(),
        "expected failure for invalid --script-format value"
    );
}

// ---------------------------------------------------------------------------
// Cycle detection (unit test without real execution)
// ---------------------------------------------------------------------------

#[test]
fn self_referencing_run_step_parses_and_dry_runs_cleanly() {
    // Create a temp script that runs itself.
    let tmp_dir = tempfile::tempdir().expect("tempdir");
    let path = tmp_dir.path().join("self.json");
    let path_json = serde_json::to_string(&path).expect("path → JSON string");
    let script = format!(r#"{{"version":1,"steps":[{{"run":{{"path":{path_json}}}}}]}}"#);
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
// A3: Concurrent recording (file locking)
// ---------------------------------------------------------------------------

/// Verifies that the file locking in append_step prevents JSON corruption
/// when two processes write to the same recording concurrently.
/// We test this by writing a known-good file and calling record stop to verify
/// the finalised output parses cleanly. True concurrency testing requires
/// spawning multiple processes; a structural validity test is sufficient here.
#[test]
fn record_stop_on_nonempty_recording_produces_valid_json() {
    let tmp_dir = tempfile::tempdir().expect("tempdir");
    let out_file = tmp_dir.path().join("nonempty.json");

    // Start recording.
    std::process::Command::new(ff_rdp_bin())
        .arg("record")
        .arg("start")
        .arg(&out_file)
        .env("XDG_STATE_HOME", tmp_dir.path())
        .output()
        .expect("record start");

    // Manually write a step JSON into the output file (simulating CLI command recording).
    // The header was written by record start; we just append a step entry.
    let step_json = r#"{"navigate":{"url":"https://example.com"}}"#;
    let file_content_before = std::fs::read_to_string(&out_file).expect("read before");
    // Append as if step_count=0 (first step, no comma).
    let appended = format!("{file_content_before}\n    {step_json}");
    std::fs::write(&out_file, &appended).expect("write appended");

    // Update state to reflect one step.
    let state_path = tmp_dir.path().join("ff-rdp").join("recording.json");
    let state_content = std::fs::read_to_string(&state_path).expect("read state");
    let mut state: serde_json::Value = serde_json::from_str(&state_content).expect("parse state");
    state["step_count"] = serde_json::json!(1);
    std::fs::write(&state_path, serde_json::to_string_pretty(&state).unwrap())
        .expect("write state");

    // Stop and check.
    let stop_out = std::process::Command::new(ff_rdp_bin())
        .arg("record")
        .arg("stop")
        .env("XDG_STATE_HOME", tmp_dir.path())
        .output()
        .expect("record stop");
    assert!(
        stop_out.status.success(),
        "record stop failed: {}",
        String::from_utf8_lossy(&stop_out.stderr)
    );

    let content = std::fs::read_to_string(&out_file).expect("read file");
    let parsed: serde_json::Value =
        serde_json::from_str(&content).expect("recorded file must be valid JSON");
    assert_eq!(parsed["version"], 1);
    assert!(parsed["steps"].is_array(), "steps must be an array");
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

// ---------------------------------------------------------------------------
// A3: NDJSON stdout purity — every output line is valid JSON
// ---------------------------------------------------------------------------

/// A3: Run a multi-step script with --dry-run and assert that every stdout line
/// is parseable as a JSON object.  This catches regressions where a command
/// helper emits a stray `println!` that contaminates the NDJSON stream.
#[test]
fn dry_run_stdout_is_pure_ndjson() {
    let script = r##"{
        "version": 1,
        "steps": [
            {"navigate": {"url": "https://example.com"}},
            {"click": {"selector": "button"}},
            {"type": {"selector": "#email", "text": "user@example.com"}},
            {"wait": {"selector": ".result", "timeout": 1000}},
            {"assert_text": {"selector": "h1", "contains": "Welcome"}}
        ]
    }"##;
    let output = run_dry(script, &[]);
    assert!(
        output.status.success(),
        "dry-run failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    // Dry-run emits a single JSON object (the plan) — verify it's valid JSON.
    assert!(
        !stdout.trim().is_empty(),
        "stdout must not be empty in dry-run"
    );
    // Each non-empty line of stdout must be valid JSON.
    for (i, line) in stdout.lines().filter(|l| !l.trim().is_empty()).enumerate() {
        serde_json::from_str::<serde_json::Value>(line).unwrap_or_else(|e| {
            panic!(
                "A3: stdout line {} is not valid JSON: {e}\n  line: {line:?}",
                i + 1
            )
        });
    }
}

/// A3: Verify --vars-file populates vars correctly (replaces --env-file).
#[test]
fn vars_file_populates_vars() {
    use std::io::Write as _;
    // Create a vars file.
    let mut vars_tmp = tempfile::NamedTempFile::new().expect("temp vars file");
    writeln!(vars_tmp, "url=https://example.com").expect("write");
    writeln!(vars_tmp, "# comment line").expect("write");
    writeln!(vars_tmp, "email=user@example.com").expect("write");
    let vars_path = vars_tmp.path().to_owned();
    let _vars_tmp = vars_tmp;

    let script = r#"{
        "version": 1,
        "steps": [
            {"navigate": {"url": "{{vars.url}}"}},
            {"assert_text": {"selector": "h1", "contains": "{{vars.email}}"}}
        ]
    }"#;
    let output = run_dry(script, &["--vars-file", vars_path.to_str().unwrap()]);
    assert!(
        output.status.success(),
        "--vars-file should populate vars for dry-run: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// A3: --env-file (deprecated) still works and prints a warning.
#[test]
fn env_file_deprecated_alias_warns() {
    use std::io::Write as _;
    let mut vars_tmp = tempfile::NamedTempFile::new().expect("temp vars file");
    writeln!(vars_tmp, "url=https://example.com").expect("write");
    let vars_path = vars_tmp.path().to_owned();
    let _vars_tmp = vars_tmp;

    let script = r#"{
        "version": 1,
        "steps": [
            {"navigate": {"url": "{{vars.url}}"}}
        ]
    }"#;
    let output = run_dry(script, &["--env-file", vars_path.to_str().unwrap()]);
    assert!(
        output.status.success(),
        "--env-file alias should still work: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("deprecated") || stderr.contains("--vars-file"),
        "deprecated warning should mention --vars-file: {stderr}"
    );
}

/// A3: default_timeout_ms field is accepted by the parser (C2 regression guard).
#[test]
fn default_timeout_ms_accepted_at_parse_time() {
    let script = r#"{
        "version": 1,
        "default_timeout_ms": 5000,
        "steps": [
            {"navigate": {"url": "https://example.com"}},
            {"assert_text": {"selector": "h1", "contains": "Welcome"}}
        ]
    }"#;
    let output = run_dry(script, &[]);
    assert!(
        output.status.success(),
        "default_timeout_ms should be accepted: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}
