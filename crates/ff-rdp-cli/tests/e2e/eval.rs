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

    // iter-141 Theme E: a JS exception thrown by the evaluated script is
    // routed through the standard JSON error envelope on stdout — the
    // single emission per the JSON-only output convention — rather than a
    // bare `error: ...` line on stderr.
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value = serde_json::from_str(stdout.trim())
        .unwrap_or_else(|e| panic!("stdout must be a JSON error envelope: {e}\nstdout: {stdout}"));
    assert_eq!(json["error_type"], "User", "got: {json}");
    assert!(
        json["error"]
            .as_str()
            .is_some_and(|s| s.contains("test error")),
        "envelope error should mention the error: {json}"
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

/// iter-161 Theme C: a result over Firefox's ~1000-char inline limit is
/// fetched in full through the `longString` actor's `substring` protocol.
///
/// This test previously asserted the defect — `results.type == "longString"`
/// and `results.length == 50000`, i.e. the caller receiving a preview grip
/// with no way to reach the other 49 000 characters (the grip is released
/// immediately after printing, and no command speaks `substring`).
#[test]
fn eval_long_string_result_is_fetched_in_full() {
    let server = eval_server("eval_result_long_string.json").on(
        "substring",
        load_fixture("substring_eval_long_string_response.json"),
    );
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend(["eval".to_owned(), "'x'.repeat(50000)".to_owned()]);

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
    let s = json["results"]
        .as_str()
        .unwrap_or_else(|| panic!("results must be the full string, not a grip: {json}"));
    assert_eq!(s.len(), 50000, "expected all 50000 chars, got {}", s.len());
    assert!(
        s.chars().all(|c| c == 'x'),
        "the fetched string must be the recorded payload"
    );
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

/// `eval --stringify "({a:1})"` returns `results` as a parsed JSON value
/// (iter-61j theme B: auto-parse the server-side JSON.stringify output).
#[test]
fn eval_stringify_object_parsed_to_json_value() {
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

/// `eval --stringify "42"` returns `results: 42` — iter-61j theme B parses
/// the JSON-stringified value back into a real JSON number.
#[test]
fn eval_stringify_number_parsed_to_json_number() {
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

/// `eval --stringify` on an array-returning expression yields `results` as a
/// real parsed JSON array (iter-61j theme B).
#[test]
fn eval_stringify_returns_parsed_json_array() {
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

// ---------------------------------------------------------------------------
// iter-161 Theme E: meta.eval_path is gone
// ---------------------------------------------------------------------------

/// iter-61r Theme C added `meta.eval_path`, which could be `"page-await"` or
/// `"chrome"`. iter-93 deleted the chrome path and DEC-020 confirmed it stays
/// deleted, leaving a constant in the envelope that discriminated nothing.
/// iter-161 Theme E removes the field; this test pins its absence so it does
/// not creep back.
#[test]
fn eval_meta_has_no_eval_path() {
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

    assert!(
        json["meta"].get("eval_path").is_none(),
        "meta.eval_path was removed in iter-161 Theme E; envelope: {json}"
    );
}

// ---------------------------------------------------------------------------
// iter-142 Theme E: ASI-separated top-level-await scripts
// ---------------------------------------------------------------------------

/// AC: `e2e_eval_asi_await_script` — an ASI-separated (no `;` anywhere)
/// two-line top-level-await script must succeed end-to-end through the real
/// CLI, not just at the `build_script` unit level. Pre-iter-142, the
/// generated wrapper for this exact script
/// (`await Promise.resolve(1)\n42`) was itself invalid JS — Firefox would
/// have rejected it with a `SyntaxError` before ever reaching
/// `evaluateJSAsync`'s result path. This test can't observe Firefox's
/// parser (the mock server returns a canned fixture regardless of the sent
/// script), but it does prove the CLI's full request/response plumbing
/// (connect → target → evaluateJSAsync → envelope) still exits 0 and
/// produces a normal result envelope for this script shape — combined with
/// `crates/ff-rdp-cli/src/commands/eval.rs`'s
/// `build_script_asi_separated_await_script_wraps_without_leaking_and_returns_tail`
/// unit test (which does inspect the generated wrapper text), the two
/// together cover both "the wrapper is valid JS" and "the CLI path works".
#[test]
fn e2e_eval_asi_await_script() {
    let server = eval_server("eval_result_number.json");
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend(["eval".to_owned(), "await Promise.resolve(1)\n42".to_owned()]);

    let output = std::process::Command::new(ff_rdp_bin())
        .args(&args)
        .output()
        .expect("failed to spawn ff-rdp");

    handle.join().unwrap();

    assert!(
        output.status.success(),
        "ASI-separated await script must exit 0, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("stdout must be valid JSON");
    assert_eq!(json["results"], 42);
}

/// AC: `e2e_help_viewport_pointers` — `eval --help` documents the headless
/// `resizeTo()` no-op and points at `launch --window-size` for a real
/// window size.
#[test]
fn eval_help_mentions_resize_to_no_op() {
    let output = std::process::Command::new(ff_rdp_bin())
        .args(["eval", "--help"])
        .output()
        .expect("failed to spawn ff-rdp");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("resizeTo"),
        "eval --help must document the headless resizeTo() no-op: {stdout}"
    );
    assert!(
        stdout.contains("window-size"),
        "eval --help must point at launch --window-size: {stdout}"
    );
}
