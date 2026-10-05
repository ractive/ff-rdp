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
        "--timeout".to_owned(),
        "5000".to_owned(),
    ]
}

/// Build the evaluateJSAsync sequence for `n` URLs.
///
/// Each URL goes through `wait_for_navigation_commit` (the same commit wait
/// as `navigate`/`reload`, iter-295) and then the collection eval, so it makes
/// four `evaluateJSAsync` calls:
///   1. pre-nav epoch (value unused by any assertion)
///   2. pre-nav `location.href` baseline (value unused)
///   3. the `location.href` re-resolution at commit
///   4. perf collection → the full perf-data JSON string
fn eval_sequence_for_n_urls(n: usize) -> Vec<(serde_json::Value, Vec<serde_json::Value>)> {
    let immediate = load_fixture("eval_immediate_response.json");
    let ready_state = load_fixture("eval_result_ready_state_complete.json");
    let href_immediate = load_fixture("eval_immediate_response_location_href.json");
    let href = load_fixture("eval_result_location_href_example_com.json");
    let perf_data = load_fixture("eval_result_perf_compare_data.json");

    let mut entries = Vec::with_capacity(n * 4);
    for _ in 0..n {
        entries.push((immediate.clone(), vec![ready_state.clone()]));
        entries.push((immediate.clone(), vec![ready_state.clone()]));
        entries.push((href_immediate.clone(), vec![href.clone()]));
        entries.push((immediate.clone(), vec![perf_data.clone()]));
    }
    entries
}

/// Request order per URL: getWatcher → evaluateJSAsync ×2 (pre-nav epoch and
/// href) → watchTargets → watchResources → navigateTo (with
/// `dom-loading`/`dom-complete` document-event follow-ups) → evaluateJSAsync
/// (commit href) → unwatchResources → getTarget (console actor refresh) →
/// evaluateJSAsync (collection). See `nav_action_commit_server` in
/// `nav_action.rs` for the commit-wait half.
fn perf_compare_server(n_urls: usize) -> MockRdpServer {
    MockRdpServer::new()
        .on("listTabs", load_fixture("list_tabs_response.json"))
        .on("getTarget", load_fixture("get_target_response.json"))
        .on("getWatcher", load_fixture("get_watcher_response.json"))
        .on("watchTargets", load_fixture("watch_targets_response.json"))
        .on(
            "watchResources",
            load_fixture("watch_resources_response.json"),
        )
        .on(
            "unwatchResources",
            load_fixture("unwatch_resources_response.json"),
        )
        .on_with_followups(
            "navigateTo",
            load_fixture("navigate_response.json"),
            vec![
                load_fixture("resources_available_document_event_dom_loading.json"),
                load_fixture("resources_available_document_event_dom_complete.json"),
            ],
        )
        .on_sequence("evaluateJSAsync", eval_sequence_for_n_urls(n_urls))
}

// ---------------------------------------------------------------------------
// perf compare — two URLs
// ---------------------------------------------------------------------------

#[test]
fn perf_compare_two_urls() {
    let server = perf_compare_server(2);
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend([
        "perf".to_owned(),
        "compare".to_owned(),
        "https://example.com/".to_owned(),
        "https://example.com/other".to_owned(),
    ]);

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

    assert_eq!(json["total"], 2, "should have 2 results");

    let results = json["results"].as_array().expect("results is array");
    assert_eq!(results.len(), 2);

    // First result uses URL as label (no --label given).
    assert_eq!(results[0]["label"], "https://example.com/");
    assert_eq!(results[0]["url"], "https://example.com/");

    // Second result
    assert_eq!(results[1]["label"], "https://example.com/other");
    assert_eq!(results[1]["url"], "https://example.com/other");

    // Vitals section is present and contains expected keys.
    for result in results {
        let vitals = &result["vitals"];
        assert!(
            vitals.is_object(),
            "vitals must be an object for each result"
        );
        assert!(vitals["lcp_ms"].is_number(), "lcp_ms must be present");
        assert!(vitals["fcp_ms"].is_number(), "fcp_ms must be present");
        assert!(vitals["ttfb_ms"].is_number(), "ttfb_ms must be present");
        // iter-139 Theme A: the fixture's `supported_entry_types` (recorded
        // from real Firefox behavior) lists neither `layout-shift` nor
        // `longtask` — cls/tbt_ms must be null with a note, not the
        // computed 0.01/30.0 the raw (unreachable) fixture entries imply.
        assert!(
            vitals["cls"].is_null(),
            "cls must be null when unsupported: {vitals}"
        );
        assert!(
            vitals["cls_note"]
                .as_str()
                .is_some_and(|n| n.contains("layout-shift")),
            "cls_note must name the missing entry type: {vitals}"
        );
        assert!(
            vitals["tbt_ms"].is_null(),
            "tbt_ms must be null when unsupported: {vitals}"
        );
        assert!(
            vitals["tbt_note"]
                .as_str()
                .is_some_and(|n| n.contains("longtask")),
            "tbt_note must name the missing entry type: {vitals}"
        );

        // Navigation section is present.
        let navigation = &result["navigation"];
        assert!(navigation.is_object(), "navigation must be an object");

        // Resources section is present.
        let resources = &result["resources"];
        assert!(resources.is_object(), "resources must be an object");
        assert_eq!(resources["count"], 1, "one resource entry in fixture");
    }
}

// ---------------------------------------------------------------------------
// perf compare — with --label flag
// ---------------------------------------------------------------------------

#[test]
fn perf_compare_with_labels() {
    let server = perf_compare_server(2);
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend([
        "perf".to_owned(),
        "compare".to_owned(),
        "https://example.com/".to_owned(),
        "https://example.com/other".to_owned(),
        "--label".to_owned(),
        "Homepage,OtherPage".to_owned(),
    ]);

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

    let results = json["results"].as_array().expect("results is array");
    assert_eq!(results[0]["label"], "Homepage");
    assert_eq!(results[1]["label"], "OtherPage");
}

// ---------------------------------------------------------------------------
// perf compare — label count mismatch → non-zero exit before connecting
// ---------------------------------------------------------------------------

#[test]
fn perf_compare_label_mismatch_error() {
    // This test does not need a real server because the error is caught before
    // any connection is established.  We still bind a port so the CLI has a
    // valid --port argument and fails for the right reason, not a parse error.
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().unwrap().port();
    // Don't accept — the CLI should exit before connecting.
    drop(listener);

    let mut args = base_args(port);
    args.extend([
        "perf".to_owned(),
        "compare".to_owned(),
        "https://example.com/".to_owned(),
        "https://example.com/other".to_owned(),
        "--label".to_owned(),
        "OnlyOne".to_owned(),
    ]);

    let output = std::process::Command::new(ff_rdp_bin())
        .args(&args)
        .output()
        .expect("failed to spawn ff-rdp");

    assert!(
        !output.status.success(),
        "expected non-zero exit for label mismatch"
    );
    assert_eq!(output.status.code(), Some(1));

    // The mismatch error is emitted as the JSON error envelope on stdout
    // (iter-98 Theme D removed the duplicate human `error:` stderr line).
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let combined = format!("{stderr}{stdout}");
    assert!(
        combined.contains('1') && combined.contains('2'),
        "error should mention label count (1) and URL count (2): stderr={stderr:?} stdout={stdout:?}"
    );
}

// ---------------------------------------------------------------------------
// perf compare — --jq filter applied to output
// ---------------------------------------------------------------------------

#[test]
fn perf_compare_with_jq_filter() {
    let server = perf_compare_server(2);
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend([
        "perf".to_owned(),
        "compare".to_owned(),
        "https://example.com/".to_owned(),
        "https://example.com/other".to_owned(),
        "--jq".to_owned(),
        ".results[].label".to_owned(),
    ]);

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

    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<&str> = stdout.trim().lines().collect();
    assert_eq!(lines.len(), 2, "jq should emit one line per result");
    assert_eq!(lines[0], r#""https://example.com/""#);
    assert_eq!(lines[1], r#""https://example.com/other""#);
}

// ---------------------------------------------------------------------------
// perf compare — single URL is rejected (clap enforces num_args = 2..)
// ---------------------------------------------------------------------------

#[test]
fn perf_compare_single_url_error() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().unwrap().port();
    drop(listener);

    let mut args = base_args(port);
    args.extend([
        "perf".to_owned(),
        "compare".to_owned(),
        "https://example.com/".to_owned(),
    ]);

    let output = std::process::Command::new(ff_rdp_bin())
        .args(&args)
        .output()
        .expect("failed to spawn ff-rdp");

    assert!(
        !output.status.success(),
        "expected non-zero exit when only one URL is given"
    );
    // clap exits with code 2 for argument parse errors.
    assert_eq!(output.status.code(), Some(2));
}

// ---------------------------------------------------------------------------
// perf compare --cold (iter-295): cacheDisabled on this command's connection
// ---------------------------------------------------------------------------

/// `perf_compare_server` plus the `--cold` prelude: `getWatcher` →
/// `getTargetConfigurationActor` → `updateConfiguration` (replying with
/// `update_fixture`).
fn cold_server(update_fixture: &str) -> MockRdpServer {
    perf_compare_server(2)
        .on(
            "getTargetConfigurationActor",
            load_fixture("get_target_configuration_actor_response.json"),
        )
        .on("updateConfiguration", load_fixture(update_fixture))
}

/// Run `perf compare --cold <a> <b>` against `server`, returning the output
/// and every request the CLI sent.
fn run_cold(server: MockRdpServer) -> (std::process::Output, Vec<serde_json::Value>) {
    let port = server.port();
    let log = server.request_log();
    let handle = std::thread::spawn(move || server.serve_one());
    let mut args = base_args(port);
    args.extend(
        [
            "perf",
            "compare",
            "--cold",
            "https://example.com/",
            "https://example.com/other",
        ]
        .map(str::to_owned),
    );
    let output = std::process::Command::new(ff_rdp_bin())
        .args(&args)
        .output()
        .expect("failed to spawn ff-rdp");
    handle.join().unwrap();
    let requests = log.lock().unwrap().clone();
    (output, requests)
}

#[test]
fn perf_compare_cold_sends_cache_disabled_before_navigating() {
    let (output, requests) = run_cold(cold_server(
        "update_configuration_cache_disabled_response.json",
    ));
    assert!(
        output.status.success(),
        "expected success: {}",
        support::output_note(&output)
    );
    let sent: Vec<&serde_json::Value> = requests
        .iter()
        .filter(|r| r["type"] == "updateConfiguration")
        .map(|r| &r["configuration"])
        .collect();
    assert_eq!(sent, vec![&serde_json::json!({"cacheDisabled": true})]);
    let types: Vec<&str> = requests.iter().filter_map(|r| r["type"].as_str()).collect();
    let update = types.iter().position(|t| *t == "updateConfiguration");
    let navigate = types.iter().position(|t| *t == "navigateTo");
    assert!(
        update.is_some() && update < navigate,
        "cacheDisabled must be set before the first navigation: {types:?}"
    );
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["meta"]["cache"], "bypassed", "{json}");
}

/// Firefox echoes only the options it applied; an echo without
/// `cacheDisabled` fails the command instead of measuring a warm cache.
#[test]
fn perf_compare_cold_fails_when_firefox_drops_cache_disabled() {
    let (output, requests) = run_cold(cold_server(
        "update_configuration_color_scheme_response.json",
    ));
    assert!(!output.status.success(), "a dropped setting must fail");
    let note = support::output_note(&output);
    assert!(note.contains("cacheDisabled"), "{note}");
    assert!(
        !requests.iter().any(|r| r["type"] == "navigateTo"),
        "nothing may be measured after a dropped setting"
    );
}

#[test]
fn perf_compare_without_cold_sends_no_configuration() {
    let server = perf_compare_server(2);
    let port = server.port();
    let log = server.request_log();
    let handle = std::thread::spawn(move || server.serve_one());
    let mut args = base_args(port);
    args.extend(
        [
            "perf",
            "compare",
            "https://example.com/",
            "https://example.com/other",
        ]
        .map(str::to_owned),
    );
    let output = std::process::Command::new(ff_rdp_bin())
        .args(&args)
        .output()
        .expect("failed to spawn ff-rdp");
    handle.join().unwrap();
    assert!(output.status.success(), "{}", support::output_note(&output));
    let requests = log.lock().unwrap().clone();
    assert!(!requests.iter().any(|r| r["type"] == "updateConfiguration"));
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(json["meta"].get("cache").is_none(), "{json}");
}
