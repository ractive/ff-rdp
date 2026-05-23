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

fn eval_server(eval_result_fixture: &str) -> MockRdpServer {
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
fn eval_string_result() {
    let server = eval_server("eval_result_string.json");
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend(["eval".to_owned(), "document.title".to_owned()]);

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

    assert_eq!(json["results"], "Example Domain");
    assert_eq!(json["total"], 1);
}

#[test]
fn eval_number_result() {
    let server = eval_server("eval_result_number.json");
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend(["eval".to_owned(), "1 + 41".to_owned()]);

    let output = std::process::Command::new(ff_rdp_bin())
        .args(&args)
        .output()
        .expect("failed to spawn ff-rdp");

    handle.join().unwrap();

    assert!(output.status.success());

    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["results"], 42);
}

#[test]
fn eval_undefined_result() {
    let server = eval_server("eval_result_undefined.json");
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend(["eval".to_owned(), "undefined".to_owned()]);

    let output = std::process::Command::new(ff_rdp_bin())
        .args(&args)
        .output()
        .expect("failed to spawn ff-rdp");

    handle.join().unwrap();

    assert!(output.status.success());

    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["results"]["type"], "undefined");
}

#[test]
fn eval_object_result() {
    let server = eval_server("eval_result_object.json");
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend(["eval".to_owned(), "({a: 1})".to_owned()]);

    let output = std::process::Command::new(ff_rdp_bin())
        .args(&args)
        .output()
        .expect("failed to spawn ff-rdp");

    handle.join().unwrap();

    assert!(output.status.success());

    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["results"]["type"], "object");
    assert_eq!(json["results"]["class"], "Object");
}

#[test]
fn eval_with_jq_filter() {
    let server = eval_server("eval_result_string.json");
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend([
        "eval".to_owned(),
        "document.title".to_owned(),
        "--jq".to_owned(),
        ".results".to_owned(),
    ]);

    let output = std::process::Command::new(ff_rdp_bin())
        .args(&args)
        .output()
        .expect("failed to spawn ff-rdp");

    handle.join().unwrap();

    assert!(output.status.success());

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(stdout.trim(), r#""Example Domain""#);
}

// ---------------------------------------------------------------------------
// Error-path tests
// ---------------------------------------------------------------------------

#[test]
fn eval_exception_exits_nonzero() {
    let server = eval_server("eval_result_exception.json");
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend([
        "eval".to_owned(),
        "throw new Error('test error')".to_owned(),
    ]);

    let output = std::process::Command::new(ff_rdp_bin())
        .args(&args)
        .output()
        .expect("failed to spawn ff-rdp");

    handle.join().unwrap();

    assert!(!output.status.success(), "expected failure for exception");
    assert_eq!(output.status.code(), Some(1));

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("test error"),
        "stderr should mention the error: {stderr}"
    );
}

// ---------------------------------------------------------------------------
// --file / --stdin input modes (iter-43)
// ---------------------------------------------------------------------------

#[test]
fn eval_from_file() {
    let server = eval_server("eval_result_string.json");
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let tmp = std::env::temp_dir().join(format!(
        "ff_rdp_eval_file_{}.js",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    // Contains optional chaining (?.) which the dogfooder couldn't pass as a shell arg.
    std::fs::write(&tmp, "getComputedStyle(document.body)?.display").unwrap();

    let mut args = base_args(port);
    args.extend([
        "eval".to_owned(),
        "--file".to_owned(),
        tmp.to_string_lossy().into_owned(),
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

    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["total"], 1);

    let _ = std::fs::remove_file(&tmp);
}

#[test]
fn eval_from_stdin() {
    use std::io::Write as _;
    use std::process::Stdio;

    let server = eval_server("eval_result_string.json");
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend(["eval".to_owned(), "--stdin".to_owned()]);

    let mut child = std::process::Command::new(ff_rdp_bin())
        .args(&args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn ff-rdp");

    // Multi-line script with optional chaining — would be mangled by the shell.
    {
        let stdin = child.stdin.as_mut().expect("stdin pipe");
        stdin
            .write_all(b"getComputedStyle(document.body)?.display\n")
            .expect("write stdin");
    }

    let output = child.wait_with_output().expect("wait for child");

    handle.join().unwrap();

    assert!(
        output.status.success(),
        "expected success, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["total"], 1);
}

#[test]
fn eval_missing_source_errors_cleanly() {
    // clap's ArgGroup rejects missing sources before we connect.
    let output = std::process::Command::new(ff_rdp_bin())
        .args(["eval"])
        .output()
        .expect("failed to spawn ff-rdp");

    assert!(!output.status.success(), "expected non-zero exit");
    let stderr = String::from_utf8_lossy(&output.stderr);
    // clap error should mention one of the required args.
    assert!(
        stderr.contains("script") || stderr.contains("--file") || stderr.contains("--stdin"),
        "expected clap error mentioning required args, got: {stderr}"
    );
}

#[test]
fn eval_conflicting_sources_errors() {
    // Supplying both positional and --stdin must fail at arg parsing time.
    let tmp = std::env::temp_dir().join("ff_rdp_eval_conflict.js");
    std::fs::write(&tmp, "1").unwrap();

    let output = std::process::Command::new(ff_rdp_bin())
        .args(["eval", "document.title", "--file", tmp.to_str().unwrap()])
        .output()
        .expect("failed to spawn ff-rdp");

    assert!(
        !output.status.success(),
        "expected failure when multiple eval sources provided"
    );

    let _ = std::fs::remove_file(&tmp);
}

#[test]
fn eval_long_string_result() {
    let server = eval_server("eval_result_long_string.json");
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend(["eval".to_owned(), "'x'.repeat(50000)".to_owned()]);

    let output = std::process::Command::new(ff_rdp_bin())
        .args(&args)
        .output()
        .expect("failed to spawn ff-rdp");

    handle.join().unwrap();

    assert!(output.status.success());

    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["results"]["type"], "longString");
    assert_eq!(json["results"]["length"], 50000);
}

// ---------------------------------------------------------------------------
// --no-isolate flag (iter-52)
// ---------------------------------------------------------------------------

#[test]
fn eval_no_isolate_flag_is_accepted() {
    // --no-isolate opts out of the default IIFE wrapping; the result fixture
    // is the same — we just verify the flag parses and the command succeeds.
    let server = eval_server("eval_result_string.json");
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend([
        "eval".to_owned(),
        "--no-isolate".to_owned(),
        "document.title".to_owned(),
    ]);

    let output = std::process::Command::new(ff_rdp_bin())
        .args(&args)
        .output()
        .expect("failed to spawn ff-rdp");

    handle.join().unwrap();
    assert!(
        output.status.success(),
        "expected success with --no-isolate, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn eval_default_isolation_succeeds_with_const_declaration() {
    // The default IIFE wrapping must not break expression evaluation; the
    // mock returns the configured fixture regardless of script contents,
    // so this asserts the wrapping doesn't trip the CLI's own logic.
    let server = eval_server("eval_result_number.json");
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend(["eval".to_owned(), "const x = 1; x".to_owned()]);

    let output = std::process::Command::new(ff_rdp_bin())
        .args(&args)
        .output()
        .expect("failed to spawn ff-rdp");

    handle.join().unwrap();
    assert!(
        output.status.success(),
        "default isolate must accept `const x = 1; x`, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

// ---------------------------------------------------------------------------
// --stringify flag
// ---------------------------------------------------------------------------

/// `eval --stringify "'foo'"` must return `"results": "foo"` — a plain string,
/// not double-encoded as `"\"foo\""`.  The page-side helper skips JSON.stringify
/// when the value is already a string.
#[test]
fn eval_stringify_string_no_double_encoding() {
    let server = eval_server("eval_result_stringify_string.json");
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend([
        "eval".to_owned(),
        "--stringify".to_owned(),
        "'foo'".to_owned(),
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

    // Must be the plain string, not a JSON-encoded string-within-a-string.
    assert_eq!(
        json["results"],
        serde_json::Value::String("foo".to_owned()),
        "string results must not be double-encoded; got: {}",
        json["results"]
    );
}

/// `eval --stringify "({a:1})"` must still return `"results": "{\"a\":1}"` —
/// objects are passed through JSON.stringify as before.
#[test]
fn eval_stringify_object_still_stringified() {
    let server = eval_server("eval_result_stringify.json");
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend([
        "eval".to_owned(),
        "--stringify".to_owned(),
        "({a:1})".to_owned(),
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

    // Theme B (iter-61j): --stringify now auto-parses the JSON string so
    // `results` holds a real JSON value (array/object), not a raw string.
    // The fixture encodes a NodeList-style array — after parsing it becomes
    // an array value in results.
    assert!(
        json["results"].is_array(),
        "--stringify array result must be parsed to a JSON array; got: {}",
        json["results"]
    );
}

/// `eval --stringify "42"` must return `"results": "42"` — numbers are
/// JSON.stringify'd to their string representation.
#[test]
fn eval_stringify_number_becomes_string() {
    let server = eval_server("eval_result_stringify_number.json");
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend(["eval".to_owned(), "--stringify".to_owned(), "42".to_owned()]);

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

    // Theme B (iter-61j): --stringify now auto-parses the JSON string.
    // "42" is valid JSON for the number 42, so results becomes Number(42).
    assert_eq!(
        json["results"],
        serde_json::Value::Number(serde_json::Number::from(42)),
        "number result must be parsed from JSON string \"42\" to number 42; got: {}",
        json["results"]
    );
}

#[test]
fn eval_stringify_returns_json_string() {
    let server = eval_server("eval_result_stringify.json");
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend([
        "eval".to_owned(),
        "--stringify".to_owned(),
        "document.querySelectorAll('a')".to_owned(),
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

    // Theme B (iter-61j): --stringify now auto-parses the JSON string.
    // The mock returns a JSON-encoded array string; after parsing, results
    // is a real JSON array.
    assert_eq!(
        json["results"],
        serde_json::json!([{"href": "https://example.com", "text": "Example"}])
    );
    assert_eq!(json["total"], 1);
}

// ---------------------------------------------------------------------------
// iter-61i theme D: --stringify auto-suppresses hints under --format text
// ---------------------------------------------------------------------------

#[test]
fn eval_stringify_text_suppresses_hints() {
    let server = eval_server("eval_result_stringify_string.json");
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend([
        "eval".to_owned(),
        "'hello'".to_owned(),
        "--stringify".to_owned(),
        "--format".to_owned(),
        "text".to_owned(),
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

    // The hint suffix that --stringify must now suppress:
    assert!(
        !stdout.contains("-> ff-rdp"),
        "stdout must not contain a `-> ff-rdp …` hint suffix when \
         --stringify is set (dogfood-49 #6); got: {stdout:?}"
    );
}
