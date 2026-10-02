use super::support::{self, MockRdpServer, load_fixture};

fn ff_rdp_bin() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_BIN_EXE_ff-rdp"))
}

/// Recorded `attach` reply (`live_thread_attach`), addressed from the thread
/// actor of the recorded target. Actor indices differ between recording
/// sessions, so `from` is bound to `get_target_response.json` explicitly.
fn attach_reply() -> serde_json::Value {
    let mut reply = load_fixture("thread_attach_response.json");
    reply["from"] = load_fixture("get_target_response.json")["frame"]["threadActor"].clone();
    reply
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

fn sources_server() -> MockRdpServer {
    MockRdpServer::new()
        .on("listTabs", load_fixture("list_tabs_response.json"))
        .on("getTarget", load_fixture("get_target_response.json"))
        .on("attach", attach_reply())
        .on("sources", load_fixture("sources_response.json"))
}

#[test]
fn sources_lists_all_scripts() {
    let server = sources_server();
    let requests = server.request_log();
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.push("sources".to_owned());

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

    assert_eq!(json["total"], 3);
    assert!(json["meta"].get("fallback").is_none());
    let requests = requests.lock().unwrap();
    let thread_requests: Vec<_> = requests
        .iter()
        .filter(|packet| packet["to"] == "server1.conn0.child2/thread1")
        .collect();
    assert_eq!(thread_requests.len(), 2, "no resume/detach cleanup");
    assert_eq!(thread_requests[0]["type"], "attach");
    assert_eq!(thread_requests[0]["options"], serde_json::json!({}));
    assert_eq!(thread_requests[1]["type"], "sources");
    let results = json["results"].as_array().expect("results is array");
    assert_eq!(results[0]["url"], "https://example.com/app.js");
    assert_eq!(results[1]["url"], "https://example.com/vendor.min.js");
    assert_eq!(results[2]["url"], "https://cdn.example.com/analytics.js");
    assert!(results[2]["isBlackBoxed"].as_bool().unwrap());
}

#[test]
fn sources_filter_by_substring() {
    let server = sources_server();
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend([
        "sources".to_owned(),
        "--filter".to_owned(),
        "vendor".to_owned(),
    ]);

    let output = std::process::Command::new(ff_rdp_bin())
        .args(&args)
        .output()
        .expect("failed to spawn ff-rdp");

    handle.join().unwrap();

    assert!(output.status.success());

    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["total"], 1);
    let results = json["results"].as_array().unwrap();
    assert_eq!(results[0]["url"], "https://example.com/vendor.min.js");
}

#[test]
fn sources_filter_by_pattern() {
    let server = sources_server();
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend([
        "sources".to_owned(),
        "--pattern".to_owned(),
        r"cdn\.example\.com".to_owned(),
    ]);

    let output = std::process::Command::new(ff_rdp_bin())
        .args(&args)
        .output()
        .expect("failed to spawn ff-rdp");

    handle.join().unwrap();

    assert!(output.status.success());

    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["total"], 1);
    let results = json["results"].as_array().unwrap();
    assert_eq!(results[0]["url"], "https://cdn.example.com/analytics.js");
}

#[test]
fn sources_with_jq_filter() {
    let server = sources_server();
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend([
        "sources".to_owned(),
        "--jq".to_owned(),
        ".results[].url".to_owned(),
    ]);

    let output = std::process::Command::new(ff_rdp_bin())
        .args(&args)
        .output()
        .expect("failed to spawn ff-rdp");

    handle.join().unwrap();

    assert!(output.status.success());

    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<&str> = stdout.trim().lines().collect();
    assert_eq!(lines.len(), 3);
    assert_eq!(lines[0], r#""https://example.com/app.js""#);
}

#[test]
fn sources_handles_null_entries_in_response() {
    // Verify that null entries in the sources array are silently skipped.
    let response_with_nulls = serde_json::json!({
        "from": "server1.conn0.child2/thread1",
        "sources": [
            {
                "actor": "server1.conn0.child2/source42",
                "url": "https://example.com/app.js",
                "isBlackBoxed": false
            },
            null,
            {
                "actor": "server1.conn0.child2/source43",
                "url": "https://example.com/lib.js",
                "isBlackBoxed": false
            }
        ]
    });

    let server = MockRdpServer::new()
        .on("listTabs", load_fixture("list_tabs_response.json"))
        .on("getTarget", load_fixture("get_target_response.json"))
        .on("attach", attach_reply())
        .on("sources", response_with_nulls);

    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.push("sources".to_owned());

    let output = std::process::Command::new(ff_rdp_bin())
        .args(&args)
        .output()
        .expect("failed to spawn ff-rdp");

    handle.join().unwrap();

    assert!(
        output.status.success(),
        "stderr: {}",
        support::output_note(&output)
    );

    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["total"], 2);
    let results = json["results"].as_array().unwrap();
    assert_eq!(results[0]["url"], "https://example.com/app.js");
    assert_eq!(results[1]["url"], "https://example.com/lib.js");
}

// ---------------------------------------------------------------------------
// Fallback-path stderr gating tests
//
// When the thread actor returns `unrecognizedPacketType`, the command falls
// back to the JS DOM/Performance API path.  The `debug:` message describing
// the fallback must be suppressed by default and emitted only under --verbose.
// ---------------------------------------------------------------------------

/// Build a mock that returns `unrecognizedPacketType` for `sources` and then
/// serves a successful JS eval fallback returning an empty sources array.
fn fallback_server() -> MockRdpServer {
    // The thread-actor sources request triggers the fallback.
    let sources_error = serde_json::json!({
        "from": "server1.conn0.child2/thread1",
        "error": "unrecognizedPacketType",
        "message": "sources"
    });

    // The JS fallback calls evaluateJSAsync on the console actor.
    // The immediate response is an ack; the follow-up carries the result.
    let eval_ack = serde_json::json!({
        "from": "server1.conn0.child2/consoleActor3",
        "resultID": "test-fallback-0"
    });
    let eval_result = serde_json::json!({
        "from": "server1.conn0.child2/consoleActor3",
        "type": "evaluationResult",
        "resultID": "test-fallback-0",
        "hasException": false,
        "input": "",
        "result": "__FF_RDP_JSON__[]",
        "startTime": 0.0,
        "timestamp": 1.0
    });

    MockRdpServer::new()
        .on("listTabs", load_fixture("list_tabs_response.json"))
        .on("getTarget", load_fixture("get_target_response.json"))
        .on("attach", attach_reply())
        .on("sources", sources_error)
        .on_with_followup("evaluateJSAsync", eval_ack, eval_result)
}

/// Without `--verbose`, the fallback `debug:` message must not appear on stderr.
#[test]
fn sources_fallback_is_silent_by_default() {
    let server = fallback_server();
    let requests = server.request_log();
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.push("sources".to_owned());

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

    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["meta"]["fallback"], true);
    assert_eq!(json["meta"]["fallback_method"], "js-eval");
    let requests = requests.lock().unwrap();
    assert!(
        !requests
            .iter()
            .any(|p| p["type"] == "resume" || p["type"] == "detach")
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("debug:"),
        "debug: message must be suppressed without --verbose, got: {stderr} ({})",
        support::output_note(&output)
    );
}

/// With `--verbose`, the fallback `debug:` message must appear on stderr.
#[test]
fn sources_fallback_emits_debug_with_verbose() {
    let server = fallback_server();
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend(["--verbose".to_owned(), "sources".to_owned()]);

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

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("debug:"),
        "--verbose must cause the debug: fallback message to appear on stderr; got: {}",
        support::output_note(&output)
    );
}

#[test]
fn sources_wrong_state_is_an_error_not_fallback() {
    let server = MockRdpServer::new()
        .on("listTabs", load_fixture("list_tabs_response.json"))
        .on("getTarget", load_fixture("get_target_response.json"))
        .on(
            "attach",
            serde_json::json!({
                "from": "server1.conn0.child2/thread1", "error": "wrongState", "message": "exited"
            }),
        );
    let requests = server.request_log();
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());
    let mut args = base_args(port);
    args.push("sources".to_owned());
    let output = std::process::Command::new(ff_rdp_bin())
        .args(&args)
        .output()
        .unwrap();
    handle.join().unwrap();
    assert!(!output.status.success());
    let requests = requests.lock().unwrap();
    assert!(!requests.iter().any(|p| matches!(
        p["type"].as_str(),
        Some("sources" | "resume" | "detach" | "evaluateJSAsync")
    )));
    let response: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(response["meta"].get("fallback").is_none());
}

/// Exercise the actual native CLI/output pipeline, not a copy of its width math.
fn native_sources_output(sources: &serde_json::Value, flags: &[&str]) -> std::process::Output {
    let server = MockRdpServer::new()
        .on("listTabs", load_fixture("list_tabs_response.json"))
        .on("getTarget", load_fixture("get_target_response.json"))
        .on("attach", attach_reply())
        .on(
            "sources",
            serde_json::json!({"from": "server1.conn0.child2/thread1", "sources": sources}),
        );
    let requests = server.request_log();
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());
    let output = std::process::Command::new(ff_rdp_bin())
        .args(base_args(port))
        .arg("sources")
        .args(flags)
        .output()
        .expect("native sources CLI");
    handle.join().unwrap();
    assert!(output.status.success(), "{}", support::output_note(&output));
    let requests = requests.lock().unwrap();
    let thread_requests: Vec<_> = requests
        .iter()
        .filter(|packet| packet["to"] == "server1.conn0.child2/thread1")
        .cloned()
        .collect();
    assert_eq!(
        thread_requests,
        vec![
            serde_json::json!({"to": "server1.conn0.child2/thread1", "type": "attach", "options": {}}),
            serde_json::json!({"to": "server1.conn0.child2/thread1", "type": "sources"}),
        ]
    );
    assert!(
        !requests
            .iter()
            .any(|packet| packet["type"] == "evaluateJSAsync")
    );
    output
}

#[test]
fn sources_native_text_table_stays_within_120_characters() {
    for (actor, url) in [
        (
            "server1.conn22.child31/source108".to_owned(),
            format!("https://example.com/{}tail.js", "tracking/".repeat(50)),
        ),
        (
            format!("server1.{}source108", "connection.".repeat(30)),
            format!("https://example.com/{}tail.js", "é界/".repeat(80)),
        ),
    ] {
        let output = native_sources_output(
            &serde_json::json!([
                {"url": url, "actor": actor, "isBlackBoxed": false},
                {"url": "https://short.test/a.js", "actor": "source2", "isBlackBoxed": true},
            ]),
            &["--format", "text"],
        );
        let stdout = String::from_utf8(output.stdout).unwrap();
        for line in stdout.lines() {
            assert!(
                line.chars().count() <= 120,
                "native sources text exceeds 120 characters: {line:?}"
            );
        }
        let lines: Vec<_> = stdout.lines().collect();
        assert_eq!(
            lines[0].split_whitespace().collect::<Vec<_>>(),
            ["url", "actor", "isBlackBoxed"]
        );
        assert!(lines[1].chars().all(|c| c == '-' || c == ' '));
        assert!(lines[2].contains("https://example.com"));
        assert!(lines[2].contains("tail.js"));
        assert!(lines[2].contains("server1."));
        assert!(lines[2].contains("source108"));
        assert!(lines[2].contains('…'));
        assert!(lines[3].contains("https://short.test/a.js"));
        assert!(lines[3].contains("source2"));
    }
}

#[test]
fn sources_native_json_and_jq_keep_complete_identities() {
    let actor = format!("server1.{}source108", "connection.".repeat(30));
    let url = format!("https://example.com/{}tail.js", "é界/".repeat(80));
    let sources = serde_json::json!([{"url": url, "actor": actor, "isBlackBoxed": false}]);
    let output = native_sources_output(&sources, &[]);
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["results"], sources);
    assert_eq!(json["meta"]["route"], "direct");
    assert!(json["meta"].get("fallback").is_none());
    let output = native_sources_output(&sources, &["--jq", ".results"]);
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json, sources);
}

#[test]
fn sources_native_text_preserves_field_projection_and_sorting() {
    let sources = serde_json::json!([
        {"url": "https://z.test/z.js", "actor": "source2", "isBlackBoxed": false},
        {"url": "https://a.test/a.js", "actor": "source1", "isBlackBoxed": true},
    ]);
    let output = native_sources_output(
        &sources,
        &[
            "--format",
            "text",
            "--no-hints",
            "--sort",
            "url",
            "--fields",
            "url,actor",
        ],
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    let lines: Vec<_> = stdout.lines().collect();
    assert_eq!(lines.len(), 4);
    assert_eq!(
        lines[0].split_whitespace().collect::<Vec<_>>(),
        ["url", "actor"]
    );
    assert_eq!(
        lines[2].split_whitespace().collect::<Vec<_>>(),
        ["https://a.test/a.js", "source1"]
    );
    assert_eq!(
        lines[3].split_whitespace().collect::<Vec<_>>(),
        ["https://z.test/z.js", "source2"]
    );
}
