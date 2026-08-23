use super::support::{self, MockRdpServer, load_fixture};

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

fn click_server(eval_result_fixture: &str) -> MockRdpServer {
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
fn click_returns_confirmation_json() {
    let server = click_server("eval_result_click.json");
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend(["click".to_owned(), "button.submit".to_owned()]);

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

    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("stdout must be valid JSON");

    // The result is a clean {clicked, tag, text} object — no raw RDP grip fields.
    assert_eq!(json["results"]["clicked"], true);
    assert_eq!(json["results"]["tag"], "BUTTON");
    assert_eq!(json["results"]["text"], "Submit");
    assert_eq!(json["meta"]["selector"], "button.submit");
}

/// AC: `e2e_click_frame_url_in_results` — `--help` documents
/// `{"results": {..., "frame_url": null}, "meta": {"frame_url": null, ...}}`
/// and says `frame_url` is "always present (never omitted)" so
/// `--jq '.results.frame_url'` never throws. Before iter-140 Theme E,
/// `run()` `.remove()`d `frame_url` from `results` when moving it into
/// `meta`, so the documented `results.frame_url` key never existed on the
/// top-frame path — this reproduces that exact path (no `--frame`, a plain
/// top-level click) against the mock server and checks both copies survive.
#[test]
fn click_frame_url_present_in_both_results_and_meta() {
    let server = click_server("eval_result_click.json");
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend(["click".to_owned(), "button.submit".to_owned()]);

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

    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("stdout must be valid JSON");

    // Both copies must exist — the top-frame path sets both to `null`.
    assert!(
        json.get("results")
            .is_some_and(|r| r.as_object().is_some_and(|o| o.contains_key("frame_url"))),
        "results.frame_url must be present (not omitted): {json}"
    );
    assert_eq!(json["results"]["frame_url"], serde_json::Value::Null);
    assert!(
        json.get("meta")
            .is_some_and(|m| m.as_object().is_some_and(|o| o.contains_key("frame_url"))),
        "meta.frame_url must be present (not omitted): {json}"
    );
    assert_eq!(json["meta"]["frame_url"], serde_json::Value::Null);
}

/// AC: `e2e_click_frame_url_in_results` — the `--jq '.results.frame_url'`
/// filter the `--help` text advertises as safe must not throw / exit
/// non-zero, on the exact path (`results.frame_url` present-but-null) the
/// previous test proves is now correct.
#[test]
fn click_jq_results_frame_url_does_not_throw() {
    let server = click_server("eval_result_click.json");
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend([
        "click".to_owned(),
        "button.submit".to_owned(),
        "--jq".to_owned(),
        ".results.frame_url".to_owned(),
    ]);

    let output = std::process::Command::new(ff_rdp_bin())
        .args(&args)
        .output()
        .expect("failed to spawn ff-rdp");

    handle.join().unwrap();

    assert!(
        output.status.success(),
        "--jq '.results.frame_url' must not throw: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

// A1: --selector flag is an alias for the positional selector argument.

#[test]
fn click_selector_flag_is_interchangeable_with_positional() {
    let server = click_server("eval_result_click.json");
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    // Use the --selector flag instead of a positional argument.
    args.extend([
        "click".to_owned(),
        "--selector".to_owned(),
        "button.submit".to_owned(),
    ]);

    let output = std::process::Command::new(ff_rdp_bin())
        .args(&args)
        .output()
        .expect("failed to spawn ff-rdp");

    handle.join().unwrap();

    assert!(
        output.status.success(),
        "click --selector should succeed just like positional: stderr={}",
        support::output_note(&output)
    );

    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("stdout must be valid JSON");

    assert_eq!(json["results"]["clicked"], true);
    assert_eq!(json["meta"]["selector"], "button.submit");
}

#[test]
fn click_both_positional_and_selector_flag_errors() {
    // No mock server needed — the error is caught before connecting.
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);

    let mut args = base_args(port);
    args.extend([
        "click".to_owned(),
        "button.submit".to_owned(),
        "--selector".to_owned(),
        "button.submit".to_owned(),
    ]);

    let output = std::process::Command::new(ff_rdp_bin())
        .args(&args)
        .output()
        .expect("failed to spawn ff-rdp");

    assert!(
        !output.status.success(),
        "expected failure when both positional and --selector are given"
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("not both") || stderr.contains("one") || stderr.contains("selector"),
        "stderr should explain the conflict: {stderr} ({})",
        support::output_note(&output)
    );
}

// ---------------------------------------------------------------------------
// Error-path tests
// ---------------------------------------------------------------------------

#[test]
fn click_element_not_found_exits_nonzero() {
    // Use --no-wait to bypass auto-wait and test the immediate "not found" path.
    // Auto-wait would turn this into a timeout (exit 124); --no-wait preserves
    // the pre-iter-59 fire-and-forget behaviour that this test exercises.
    //
    // iter-129: a top-level "Element not found" now always triggers the
    // frame-scan fallback (`click`'s Theme B) before giving up — even under
    // --no-wait — so the mock server must also answer `getWatcher` /
    // `watchTargets` / `watchResources` (empty acks, no target-available-form
    // pushed) for the scan to complete instead of hitting an unmocked-method
    // error. The final error message changes from the bare "Element not
    // found" to the frame-aware "matched in 0 of N frames" diagnostic — exit
    // code stays 1 (`AppError::User`) either way.
    let server = click_server("eval_result_element_not_found.json")
        .on(
            "getWatcher",
            serde_json::json!({"from": "server1.conn0.tabDescriptor1", "actor": "server1.conn0.watcher4"}),
        )
        .on("watchTargets", serde_json::json!({"from": "server1.conn0.watcher4"}))
        .on("watchResources", serde_json::json!({"from": "server1.conn0.watcher4"}));
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend([
        "click".to_owned(),
        "--no-wait".to_owned(),
        "button.missing".to_owned(),
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

    // `AppError::User` (the frame-scan's zero-match diagnostic) is emitted as
    // the JSON error envelope on **stdout** (main.rs's single-error-emission
    // convention, iter-98 Theme D) — not stderr, which `AppError::Exit`'s
    // callers use instead for a genuine JS failure.
    // iter-140 Theme D: the message now says "frame(s) tried (of N total)"
    // instead of a bare frame count, so it can distinguish "every frame was
    // tried" from "--frame narrowed the scan" (see click.rs's
    // `click_in_scanned_frame`).
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("matched in 0 of") && stdout.contains("frame(s) tried"),
        "stdout should carry the frame-aware not-found diagnostic: {stdout}"
    );
}
