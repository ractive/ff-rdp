use super::support::{MockRdpServer, load_fixture};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// JSON mode hint tests
// ---------------------------------------------------------------------------

/// In default JSON mode, the envelope must NOT include a `"hints"` key —
/// hints are off by default for JSON output.
#[test]
fn json_output_has_hints_key() {
    let fixture = load_fixture("list_tabs_response.json");

    let server = MockRdpServer::new().on("listTabs", fixture);
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.push("tabs".to_owned());

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

    assert!(
        json.get("hints").is_none(),
        "envelope must not contain 'hints' key when hints are off; got: {json}"
    );
}

/// With `--hints` forced on, the JSON envelope must include a non-empty
/// `"hints"` array where every entry has `"description"` and `"cmd"` keys.
#[test]
fn json_with_hints_flag_has_populated_hints() {
    let fixture = load_fixture("list_tabs_response.json");

    let server = MockRdpServer::new().on("listTabs", fixture);
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend(["tabs".to_owned(), "--hints".to_owned()]);

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

    let hints = json["hints"].as_array().expect("'hints' must be an array");
    assert!(
        !hints.is_empty(),
        "hints must be non-empty when --hints is passed"
    );

    for hint in hints {
        assert!(
            hint.get("description").is_some(),
            "each hint must have a 'description' key; got: {hint}"
        );
        assert!(
            hint.get("cmd").is_some(),
            "each hint must have a 'cmd' key; got: {hint}"
        );
    }
}

// ---------------------------------------------------------------------------
// Text mode hint tests
// ---------------------------------------------------------------------------

/// In `--format text` mode, hints are shown by default as `  -> ff-rdp ...  # description` lines.
#[test]
fn text_output_includes_hint_lines() {
    let fixture = load_fixture("list_tabs_response.json");

    let server = MockRdpServer::new().on("listTabs", fixture);
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend(["tabs".to_owned(), "--format".to_owned(), "text".to_owned()]);

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
        stdout.contains("  -> ff-rdp"),
        "stdout must contain hint lines starting with '  -> ff-rdp'; got:\n{stdout}"
    );
    assert!(
        stdout.contains('#'),
        "stdout must contain '#' (the description separator in hint lines); got:\n{stdout}"
    );
}

/// `--no-hints` in text mode must suppress all hint lines.
#[test]
fn text_no_hints_suppresses_hints() {
    let fixture = load_fixture("list_tabs_response.json");

    let server = MockRdpServer::new().on("listTabs", fixture);
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend([
        "tabs".to_owned(),
        "--format".to_owned(),
        "text".to_owned(),
        "--no-hints".to_owned(),
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
        !stdout.contains("  -> "),
        "stdout must not contain hint lines when --no-hints is passed; got:\n{stdout}"
    );
}

// ---------------------------------------------------------------------------
// jq filter hint suppression test
// ---------------------------------------------------------------------------

/// `--jq` pipes through jq and must suppress hints entirely — the output is
/// a raw jq value, not a JSON envelope, and contains no hint lines.
#[test]
fn jq_suppresses_hints() {
    let fixture = load_fixture("list_tabs_response.json");

    let server = MockRdpServer::new().on("listTabs", fixture);
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend([
        "tabs".to_owned(),
        "--jq".to_owned(),
        ".results | length".to_owned(),
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
    let trimmed = stdout.trim();

    // jq emits the raw numeric value, not a JSON envelope.
    assert_eq!(
        trimmed, "2",
        "jq output must be the raw count '2'; got: {trimmed}"
    );

    // No hint lines must appear in jq output.
    assert!(
        !stdout.contains("  -> "),
        "jq output must not contain hint lines; got:\n{stdout}"
    );

    // The output must not be a JSON object with a hints key.
    if let Ok(json) = serde_json::from_str::<serde_json::Value>(trimmed) {
        assert!(
            json.get("hints").is_none(),
            "jq output must not be an envelope with 'hints'; got: {json}"
        );
    }
}
