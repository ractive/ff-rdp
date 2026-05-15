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

fn type_server(eval_result_fixture: &str) -> MockRdpServer {
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
// Happy-path tests
// ---------------------------------------------------------------------------

#[test]
fn type_text_returns_confirmation_json() {
    let server = type_server("eval_result_type.json");
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend([
        "type".to_owned(),
        "input[name=email]".to_owned(),
        "test@example.com".to_owned(),
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

    // The type command now returns a flat JSON object (same pattern as click).
    assert_eq!(json["results"]["typed"], true);
    assert_eq!(json["results"]["tag"], "INPUT");
    assert_eq!(json["results"]["value"], "test@example.com");
    assert_eq!(json["meta"]["selector"], "input[name=email]");
}

#[test]
fn type_text_with_clear_flag() {
    let server = type_server("eval_result_type.json");
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend([
        "type".to_owned(),
        "input[name=email]".to_owned(),
        "test@example.com".to_owned(),
        "--clear".to_owned(),
    ]);

    let output = std::process::Command::new(ff_rdp_bin())
        .args(&args)
        .output()
        .expect("failed to spawn ff-rdp");

    handle.join().unwrap();

    assert!(
        output.status.success(),
        "expected success with --clear, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("stdout must be valid JSON");

    assert_eq!(json["results"]["typed"], true);
    assert_eq!(json["results"]["tag"], "INPUT");
}

// ---------------------------------------------------------------------------
// Error-path tests
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Flag-vs-positional ergonomics (iter-52)
// ---------------------------------------------------------------------------

#[test]
fn type_text_named_flags_work() {
    let server = type_server("eval_result_type.json");
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend([
        "type".to_owned(),
        "--selector".to_owned(),
        "input[name=email]".to_owned(),
        "--text".to_owned(),
        "test@example.com".to_owned(),
    ]);

    let output = std::process::Command::new(ff_rdp_bin())
        .args(&args)
        .output()
        .expect("failed to spawn ff-rdp");

    handle.join().unwrap();

    assert!(
        output.status.success(),
        "expected --selector/--text form to succeed, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("stdout must be valid JSON");
    assert_eq!(json["results"]["typed"], true);
    assert_eq!(json["meta"]["selector"], "input[name=email]");
}

#[test]
fn type_text_named_with_clear_works() {
    let server = type_server("eval_result_type.json");
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend([
        "type".to_owned(),
        "--selector".to_owned(),
        "input[name=email]".to_owned(),
        "--text".to_owned(),
        "test@example.com".to_owned(),
        "--clear".to_owned(),
    ]);

    let output = std::process::Command::new(ff_rdp_bin())
        .args(&args)
        .output()
        .expect("failed to spawn ff-rdp");

    handle.join().unwrap();

    assert!(
        output.status.success(),
        "expected --selector/--text/--clear form to succeed, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn type_text_conflict_positional_and_flag_selector_errors() {
    // No server needed — the conflict is detected before connecting.
    let output = std::process::Command::new(ff_rdp_bin())
        .args([
            "type",
            "input[name=email]",
            "test@example.com",
            "--selector",
            "input[name=other]",
        ])
        .output()
        .expect("failed to spawn ff-rdp");

    assert!(
        !output.status.success(),
        "expected failure when both positional selector and --selector provided"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("--selector") || stderr.contains("cannot be used with"),
        "expected conflict message mentioning --selector, got: {stderr}"
    );
}

#[test]
fn type_text_conflict_positional_and_flag_text_errors() {
    let output = std::process::Command::new(ff_rdp_bin())
        .args([
            "type",
            "input[name=email]",
            "test@example.com",
            "--text",
            "other",
        ])
        .output()
        .expect("failed to spawn ff-rdp");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("--text") && stderr.contains("not both"),
        "expected explicit conflict message, got: {stderr}"
    );
}

#[test]
fn type_text_unknown_flag_emits_tailored_hint() {
    // Generic clap error must be augmented with the type-specific hint.
    let output = std::process::Command::new(ff_rdp_bin())
        .args(["type", "--bogus-flag"])
        .output()
        .expect("failed to spawn ff-rdp");

    assert!(!output.status.success(), "unknown flag should fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("hint:") && stderr.contains("--selector/--text"),
        "expected tailored hint after clap error, got: {stderr}"
    );
}

// JS payload contains the framework-aware setter logic.  We can't drive a real
// React tracker in a mock test, but we can assert the payload includes the
// pieces that make framework support work.
#[test]
fn type_text_js_uses_native_prototype_setter() {
    // The mock server captures requests in serve_one but doesn't expose them.
    // Instead, verify the source: the type_text command file embeds the marker
    // literals we depend on for framework compatibility.
    let src = include_str!("../../src/commands/type_text.rs");
    assert!(
        src.contains("HTMLInputElement.prototype"),
        "type JS should invoke native HTMLInputElement.prototype setter for React/Vue/Svelte trackers"
    );
    assert!(
        src.contains("HTMLTextAreaElement.prototype"),
        "type JS should also handle HTMLTextAreaElement"
    );
    assert!(
        src.contains("HTMLSelectElement.prototype"),
        "type JS should also handle HTMLSelectElement"
    );
    assert!(
        src.contains("'input'") && src.contains("'change'"),
        "type JS should dispatch input and change events"
    );
}

// ---------------------------------------------------------------------------
// Error-path tests
// ---------------------------------------------------------------------------

#[test]
fn type_text_element_not_found_exits_nonzero() {
    // Use --no-wait to bypass auto-wait and test the immediate "not found" path.
    // Auto-wait would turn this into a timeout (exit 124); --no-wait preserves
    // the pre-iter-59 fire-and-forget behaviour that this test exercises.
    let server = type_server("eval_result_element_not_found.json");
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend([
        "type".to_owned(),
        "--no-wait".to_owned(),
        "input.missing".to_owned(),
        "hello".to_owned(),
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

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Element not found"),
        "stderr should mention element not found: {stderr}"
    );
}
