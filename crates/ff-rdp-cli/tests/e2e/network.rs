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
        // Short timeout so the event drain loop exits quickly.
        "--timeout".to_owned(),
        "1000".to_owned(),
        "--no-daemon".to_owned(),
    ]
}

fn network_server() -> MockRdpServer {
    MockRdpServer::new()
        .on("listTabs", load_fixture("list_tabs_response.json"))
        .on("getTarget", load_fixture("get_target_response.json"))
        .on("getWatcher", load_fixture("get_watcher_response.json"))
        .on_with_followups(
            "watchResources",
            load_fixture("watch_resources_response.json"),
            vec![
                load_fixture("resources_available_network.json"),
                load_fixture("resources_updated_network.json"),
            ],
        )
        // unwatchResources is called during cleanup; provide a response.
        .on(
            "unwatchResources",
            load_fixture("watch_resources_response.json"),
        )
}

// ---------------------------------------------------------------------------
// Summary mode (default)
// ---------------------------------------------------------------------------

#[test]
fn network_shows_summary_by_default() {
    let server = network_server();
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.push("network".to_owned());

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

    // Summary mode: results is an object, not an array.
    assert!(
        json["results"].is_object(),
        "default network output should be summary (object), got: {}",
        json["results"]
    );
    assert_eq!(json["results"]["total_requests"], 2);
    assert!(json["results"]["slowest"].is_array());
    assert!(json["results"]["by_cause_type"].is_object());
}

// ---------------------------------------------------------------------------
// iter-101 Theme D: `--since` parity — one-shot must fail loudly
// ---------------------------------------------------------------------------

/// AC: `e2e_network_since_no_daemon_explicit` — `network --since -1 --no-daemon`
/// exits non-zero with a stable `error_type: "since_requires_daemon"` and does
/// NOT emit an unfiltered result (the pre-101 silent no-op).
#[test]
fn e2e_network_since_no_daemon_explicit() {
    // The `--since` + `--no-daemon` refusal fires *before* any connection is
    // opened (iter-101 Theme D), so no mock server is needed — a port with
    // nothing listening is fine; the command must never reach it.
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind for free port");
    let port = listener.local_addr().expect("local_addr").port();
    drop(listener);

    let mut args = base_args(port);
    // `--since -1` works as space-separated thanks to `allow_hyphen_values`
    // on the arg (matches the plan's dogfood path exactly).
    args.extend(["network".to_owned(), "--since".to_owned(), "-1".to_owned()]);

    let output = std::process::Command::new(ff_rdp_bin())
        .args(&args)
        .output()
        .expect("failed to spawn ff-rdp");

    assert!(
        !output.status.success(),
        "network --since --no-daemon must exit non-zero; stdout: {}",
        String::from_utf8_lossy(&output.stdout)
    );

    // The structured error is emitted as a JSON envelope with the stable
    // discriminant at the top level (see `AppError::to_error_json`).
    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("stdout must be valid JSON error envelope");
    assert_eq!(
        json["error_type"], "since_requires_daemon",
        "expected since_requires_daemon error_type, got: {json}"
    );

    // No unfiltered network results must leak — the payload is an error
    // envelope only.
    assert!(
        json.get("results").is_none() || json["results"].is_null(),
        "no network results must be emitted on the since_requires_daemon error, got: {json}"
    );

    // Deterministic exit code 1 (runtime/user-error bucket), never clap's
    // usage exit code 2.
    assert_eq!(
        output.status.code(),
        Some(1),
        "since_requires_daemon must exit 1 (runtime/user error)"
    );
}

// ---------------------------------------------------------------------------
// Detail mode (--detail flag)
// ---------------------------------------------------------------------------

#[test]
fn network_detail_shows_requests() {
    let server = network_server();
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend(["--detail".to_owned(), "network".to_owned()]);

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

    assert_eq!(json["total"], 2);
    let results = json["results"].as_array().expect("results is array");

    // Results are sorted by duration_ms desc (default for detail mode).
    assert_eq!(results[0]["method"], "GET");
    assert_eq!(results[0]["url"], "https://example.com/");
    assert_eq!(results[0]["status"], 200);
    assert_eq!(results[0]["is_xhr"], false);

    assert_eq!(results[1]["method"], "GET");
    assert_eq!(results[1]["url"], "https://example.com/favicon.ico");
    assert_eq!(results[1]["status"], 404);

    // iter-126: detail mode (which --jq forces) now carries the summary fields
    // alongside `results`, so consumers are not cut off from total_requests etc.
    assert_eq!(
        json["total_requests"], 2,
        "detail envelope must carry total_requests, got: {json}"
    );
    assert!(
        json["total_transfer_bytes"].is_number(),
        "detail envelope must carry total_transfer_bytes"
    );
    assert!(json["by_cause_type"].is_object());
    assert!(json["slowest"].is_array());
    assert_eq!(json["timeout_reached"], false);
}

// ---------------------------------------------------------------------------
// --limit flag triggers detail mode and truncates
// ---------------------------------------------------------------------------

#[test]
fn network_limit_shows_detail_with_truncation() {
    let server = network_server();
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend(["--limit".to_owned(), "1".to_owned(), "network".to_owned()]);

    let output = std::process::Command::new(ff_rdp_bin())
        .args(&args)
        .output()
        .expect("failed to spawn ff-rdp");

    handle.join().unwrap();

    assert!(output.status.success());

    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    // Total should reflect the actual count before truncation.
    assert_eq!(json["total"], 2);
    // Only 1 result shown.
    assert_eq!(json["results"].as_array().unwrap().len(), 1);
    assert_eq!(json["truncated"], true);
    assert!(json["hint"].as_str().unwrap().contains("--all"));
}

// ---------------------------------------------------------------------------
// --all flag overrides default limit in detail mode
// ---------------------------------------------------------------------------

#[test]
fn network_all_overrides_limit() {
    let server = network_server();
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend(["--all".to_owned(), "network".to_owned()]);

    let output = std::process::Command::new(ff_rdp_bin())
        .args(&args)
        .output()
        .expect("failed to spawn ff-rdp");

    handle.join().unwrap();

    assert!(output.status.success());

    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["total"], 2);
    // All 2 results shown, no truncation.
    assert_eq!(json["results"].as_array().unwrap().len(), 2);
    assert!(json.get("truncated").is_none());
}

// ---------------------------------------------------------------------------
// e2e_network_truncation_flag (iter-141 Theme F)
// ---------------------------------------------------------------------------

/// AC `e2e_network_truncation_flag`: `slowest_truncated` is always present
/// on the summary/detail envelope (iter-128's always-present-nullable-key
/// convention) — plumbed end-to-end through the real CLI process, not just
/// `build_network_summary` in isolation.
///
/// The `>20 requests -> slowest_truncated: true` case is covered by
/// `build_network_summary_slowest_truncated_when_over_20_requests` (unit
/// test, `crates/ff-rdp-cli/src/commands/network.rs`) rather than here: this
/// repo's fixture policy requires `tests/fixtures/*.json` to be recorded
/// from a real Firefox instance, and the only recorded network-event
/// fixture captures 2 requests — nowhere near the 20-request threshold. This
/// test instead proves the field reaches the CLI's stdout unchanged (`false`
/// for a 2-request capture) — the plumbing the unit test above cannot cover
/// on its own.
#[test]
fn e2e_network_truncation_flag() {
    let server = network_server();
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.push("network".to_owned());

    let output = std::process::Command::new(ff_rdp_bin())
        .args(&args)
        .output()
        .expect("failed to spawn ff-rdp");

    handle.join().unwrap();
    assert!(output.status.success());

    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        json["results"]["slowest_truncated"], false,
        "slowest_truncated must be present and false for a 2-request capture, got: {json}"
    );
}

// ---------------------------------------------------------------------------
// --filter URL
// ---------------------------------------------------------------------------

#[test]
fn network_filter_by_url() {
    let server = network_server();
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend([
        "--detail".to_owned(),
        "network".to_owned(),
        "--filter".to_owned(),
        "favicon".to_owned(),
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
    assert_eq!(results[0]["url"], "https://example.com/favicon.ico");
}

// ---------------------------------------------------------------------------
// --method filter
// ---------------------------------------------------------------------------

#[test]
fn network_filter_by_method() {
    let server = network_server();
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend([
        "--detail".to_owned(),
        "network".to_owned(),
        "--method".to_owned(),
        "POST".to_owned(),
    ]);

    let output = std::process::Command::new(ff_rdp_bin())
        .args(&args)
        .output()
        .expect("failed to spawn ff-rdp");

    handle.join().unwrap();

    assert!(output.status.success());

    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    // No POST requests in our fixtures — should be empty.
    assert_eq!(json["total"], 0);
}

// ---------------------------------------------------------------------------
// --jq filter activates detail mode
// ---------------------------------------------------------------------------

#[test]
fn network_with_jq_filter() {
    let server = network_server();
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend([
        "--jq".to_owned(),
        ".results[] | select(.status >= 400)".to_owned(),
        "network".to_owned(),
    ]);

    let output = std::process::Command::new(ff_rdp_bin())
        .args(&args)
        .output()
        .expect("failed to spawn ff-rdp");

    handle.join().unwrap();

    assert!(output.status.success());

    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(json["status"], 404);
    assert_eq!(json["url"], "https://example.com/favicon.ico");
}

// ---------------------------------------------------------------------------
// Empty result set
// ---------------------------------------------------------------------------

#[test]
fn network_empty_when_no_events() {
    // Server without the watchResources followups — no events arrive.
    let server = MockRdpServer::new()
        .on("listTabs", load_fixture("list_tabs_response.json"))
        .on("getTarget", load_fixture("get_target_response.json"))
        .on("getWatcher", load_fixture("get_watcher_response.json"))
        .on(
            "watchResources",
            load_fixture("watch_resources_response.json"),
        )
        .on(
            "unwatchResources",
            load_fixture("watch_resources_response.json"),
        );

    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.push("network".to_owned());

    let output = std::process::Command::new(ff_rdp_bin())
        .args(&args)
        .output()
        .expect("failed to spawn ff-rdp");

    handle.join().unwrap();

    assert!(output.status.success());

    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    // Summary mode with no entries: total_requests = 0
    assert_eq!(json["results"]["total_requests"], 0);
}

// ---------------------------------------------------------------------------
// Performance API as an explicit opt-out (iter-159: never as a silent fallback)
// ---------------------------------------------------------------------------

/// iter-159: with an empty watcher buffer and no `--source`, `network` reports
/// zero **watcher** rows.  It does not substitute the Performance API.
///
/// The deleted `auto` rule did exactly that, and because the substitute dataset
/// has no `method`/`status`/`content_type`/`transfer_size`, a daemon whose
/// watcher had stopped delivering anything at all looked like a page with no
/// HTTP metadata rather than like a bug.
#[test]
fn network_empty_watcher_does_not_substitute_performance_api() {
    let server = MockRdpServer::new()
        .on("listTabs", load_fixture("list_tabs_response.json"))
        .on("getTarget", load_fixture("get_target_response.json"))
        .on("getWatcher", load_fixture("get_watcher_response.json"))
        .on(
            "watchResources",
            load_fixture("watch_resources_response.json"),
        )
        .on(
            "unwatchResources",
            load_fixture("watch_resources_response.json"),
        )
        .on_with_followup(
            "evaluateJSAsync",
            load_fixture("eval_immediate_response.json"),
            load_fixture("eval_result_network_perf_fallback.json"),
        );

    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend(["--detail".to_owned(), "network".to_owned()]);

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

    assert_eq!(json["total"], 0, "empty watcher must report zero rows");
    assert_eq!(
        json["meta"]["source"], "watcher",
        "meta.source must name where we looked, not a substitute"
    );
    assert!(
        json["meta"].get("source_reason").is_none(),
        "source_reason existed only to explain the deleted substitution"
    );
}

#[test]
fn network_source_performance_api_returns_perf_rows() {
    // Watcher returns no network events (no followups); Performance API eval
    // returns two resource entries as a plain JSON array.
    let server = MockRdpServer::new()
        .on("listTabs", load_fixture("list_tabs_response.json"))
        .on("getTarget", load_fixture("get_target_response.json"))
        .on("getWatcher", load_fixture("get_watcher_response.json"))
        .on(
            "watchResources",
            load_fixture("watch_resources_response.json"),
        )
        .on(
            "unwatchResources",
            load_fixture("watch_resources_response.json"),
        )
        .on_with_followup(
            "evaluateJSAsync",
            load_fixture("eval_immediate_response.json"),
            load_fixture("eval_result_network_perf_fallback.json"),
        );

    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend([
        "--detail".to_owned(),
        "network".to_owned(),
        "--source".to_owned(),
        "performance-api".to_owned(),
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

    // The explicit opt-out still returns Performance-API rows.
    assert_eq!(json["total"], 2, "expected 2 entries from --source performance-api");
    let results = json["results"].as_array().expect("results is array");
    // All entries should have source = "performance-api".
    for entry in results {
        assert_eq!(
            entry["source"], "performance-api",
            "expected performance-api source, got: {entry}"
        );
    }
    // Meta should advertise the performance-api source.
    assert_eq!(json["meta"]["source"], "performance-api");
}

#[test]
fn network_summary_source_performance_api_returns_perf_rows() {
    // Summary mode with the explicit opt-out: perf source returns 2 entries.
    let server = MockRdpServer::new()
        .on("listTabs", load_fixture("list_tabs_response.json"))
        .on("getTarget", load_fixture("get_target_response.json"))
        .on("getWatcher", load_fixture("get_watcher_response.json"))
        .on(
            "watchResources",
            load_fixture("watch_resources_response.json"),
        )
        .on(
            "unwatchResources",
            load_fixture("watch_resources_response.json"),
        )
        .on_with_followup(
            "evaluateJSAsync",
            load_fixture("eval_immediate_response.json"),
            load_fixture("eval_result_network_perf_fallback.json"),
        );

    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend([
        "network".to_owned(),
        "--source".to_owned(),
        "performance-api".to_owned(),
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

    // Summary mode: results is an object with total_requests = 2.
    assert!(json["results"].is_object(), "expected summary object");
    assert_eq!(json["results"]["total_requests"], 2);
    assert_eq!(json["meta"]["source"], "performance-api");
}

#[test]
fn network_prints_hint_when_both_sources_empty() {
    // Watcher has no events and Performance API returns an empty array.
    // The command should still succeed but print a hint to stderr.
    let server = MockRdpServer::new()
        .on("listTabs", load_fixture("list_tabs_response.json"))
        .on("getTarget", load_fixture("get_target_response.json"))
        .on("getWatcher", load_fixture("get_watcher_response.json"))
        .on(
            "watchResources",
            load_fixture("watch_resources_response.json"),
        )
        .on(
            "unwatchResources",
            load_fixture("watch_resources_response.json"),
        )
        .on_with_followup(
            "evaluateJSAsync",
            load_fixture("eval_immediate_response.json"),
            load_fixture("eval_result_empty_array.json"),
        );

    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.push("network".to_owned());

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

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("hint:"),
        "expected a hint on stderr when both sources empty, got: {stderr:?}"
    );
    assert!(
        stderr.contains("--follow") || stderr.contains("Navigate"),
        "hint should mention --follow or Navigate, got: {stderr:?}"
    );

    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("stdout must be valid JSON");
    // Summary mode with no entries: total_requests = 0.
    assert_eq!(json["results"]["total_requests"], 0);
}

// ---------------------------------------------------------------------------
// --follow: streaming mode
// ---------------------------------------------------------------------------

#[test]
fn network_follow_streams_request_and_response_events() {
    // --follow uses watchResources then loops until EOF.
    // close_after_followups causes the server to drop the connection after
    // delivering the followup events, which triggers a clean EOF in the
    // follow loop and allows the client to exit.
    let server = MockRdpServer::new()
        .on("listTabs", load_fixture("list_tabs_response.json"))
        .on("getTarget", load_fixture("get_target_response.json"))
        .on("getWatcher", load_fixture("get_watcher_response.json"))
        .on_with_followups(
            "watchResources",
            load_fixture("watch_resources_response.json"),
            vec![
                load_fixture("resources_available_network.json"),
                load_fixture("resources_updated_network.json"),
            ],
        )
        .on(
            "unwatchResources",
            load_fixture("watch_resources_response.json"),
        )
        .close_after_followups();

    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend(["network".to_owned(), "--follow".to_owned()]);

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
    let lines: Vec<&str> = stdout.trim().lines().collect();

    // Expect at least 2 request events (one per resource) plus response events.
    assert!(
        lines.len() >= 2,
        "expected multiple NDJSON lines, got: {stdout}"
    );

    // First lines should be "request" events.
    let first: serde_json::Value = serde_json::from_str(lines[0]).expect("valid JSON on line 0");
    assert_eq!(first["event"], "request");
    assert_eq!(first["method"], "GET");
    assert!(
        first["url"].as_str().is_some_and(|u| !u.is_empty()),
        "url should be present"
    );

    // There should also be "response" events in the output.
    let has_response = lines.iter().any(|line| {
        serde_json::from_str::<serde_json::Value>(line).is_ok_and(|v| v["event"] == "response")
    });
    assert!(has_response, "expected at least one response event");
}

#[test]
fn network_follow_filter_suppresses_non_matching_requests() {
    let server = MockRdpServer::new()
        .on("listTabs", load_fixture("list_tabs_response.json"))
        .on("getTarget", load_fixture("get_target_response.json"))
        .on("getWatcher", load_fixture("get_watcher_response.json"))
        .on_with_followups(
            "watchResources",
            load_fixture("watch_resources_response.json"),
            vec![
                load_fixture("resources_available_network.json"),
                load_fixture("resources_updated_network.json"),
            ],
        )
        .on(
            "unwatchResources",
            load_fixture("watch_resources_response.json"),
        )
        .close_after_followups();

    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend([
        "network".to_owned(),
        "--follow".to_owned(),
        "--filter".to_owned(),
        "favicon".to_owned(),
    ]);

    let output = std::process::Command::new(ff_rdp_bin())
        .args(&args)
        .output()
        .expect("failed to spawn ff-rdp");

    handle.join().unwrap();

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    // All output lines must reference the favicon URL only.
    for line in stdout.trim().lines() {
        let v: serde_json::Value = serde_json::from_str(line).expect("valid JSON");
        assert!(
            v["url"].as_str().unwrap_or("").contains("favicon"),
            "unexpected URL in filtered output: {v}"
        );
    }
}

// ---------------------------------------------------------------------------
// Theme C: watcher source shown in meta when daemon buffer has entries
// ---------------------------------------------------------------------------

#[test]
fn network_meta_source_watcher_when_watcher_has_entries() {
    // Watcher returns events → source should be "watcher" in meta, not "performance-api".
    let server = network_server();
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend(["--detail".to_owned(), "network".to_owned()]);

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

    // When watcher has entries, meta.source must be "watcher".
    assert_eq!(
        json["meta"]["source"], "watcher",
        "expected meta.source = watcher when watcher returned entries, got: {}",
        json["meta"]
    );
}

// ---------------------------------------------------------------------------
// Theme D: --detail --headers on performance-api source emits a note per entry
// ---------------------------------------------------------------------------

#[test]
fn network_detail_headers_on_perf_source_emits_note() {
    // Watcher returns no events; performance-api fallback returns 2 entries.
    // When --headers is requested, each entry should carry a note explaining
    // that headers are unavailable from performance-api.
    let server = MockRdpServer::new()
        .on("listTabs", load_fixture("list_tabs_response.json"))
        .on("getTarget", load_fixture("get_target_response.json"))
        .on("getWatcher", load_fixture("get_watcher_response.json"))
        .on(
            "watchResources",
            load_fixture("watch_resources_response.json"),
        )
        .on(
            "unwatchResources",
            load_fixture("watch_resources_response.json"),
        )
        .on_with_followup(
            "evaluateJSAsync",
            load_fixture("eval_immediate_response.json"),
            load_fixture("eval_result_network_perf_fallback.json"),
        );

    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend([
        "--detail".to_owned(),
        "network".to_owned(),
        "--headers".to_owned(),
        "--source".to_owned(),
        "performance-api".to_owned(),
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

    // Meta must advertise the actual source.
    assert_eq!(json["meta"]["source"], "performance-api");

    // Every entry must have a note mentioning --headers was ignored.
    let results = json["results"].as_array().expect("results is array");
    assert!(
        !results.is_empty(),
        "expected at least one result from perf fallback"
    );
    for entry in results {
        let note = entry["note"].as_str().unwrap_or("");
        assert!(
            note.contains("--headers ignored"),
            "expected '--headers ignored' note on performance-api entry, got: {note:?}"
        );
        assert!(
            note.contains("--with-network"),
            "note should mention --with-network, got: {note:?}"
        );
        // Entries must NOT have a headers field (since none were fetched).
        assert!(
            entry.get("headers").is_none(),
            "performance-api entries must not have a headers field"
        );
    }
}
