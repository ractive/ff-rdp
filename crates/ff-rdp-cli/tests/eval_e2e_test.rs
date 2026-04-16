mod support;

use support::{MockRdpServer, load_fixture};

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
