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

fn navigate_server() -> MockRdpServer {
    // Since iter-61v Theme A, navigate subscribes to document-event resources
    // and waits for dom-complete (not JS readyState polling).  The flow (iter-79):
    //   listTabs → getTarget → getWatcher → watchTargets → watchResources →
    //   navigateTo (with dom-loading + dom-complete followups) →
    //   unwatchResources → getTarget (refresh_console_actor after navigate)
    //
    // `evaluateJSAsync` is registered defensively (iter-96): the `Both`
    // wait-strategy readystate fallback calls it twice (readyState condition
    // poll, then `window.location.href`) if the events wait above ever times
    // out. Without a handler here that path is guaranteed-fatal — the mock
    // would reply with an `unknownMethod` error instead of a real timeout,
    // masking flakiness instead of degrading gracefully. Reuses recorded
    // fixtures from the `eval` e2e suite; the specific values aren't asserted
    // on here since the events path above should always win.
    MockRdpServer::new()
        .on("listTabs", load_fixture("list_tabs_response.json"))
        .on("getTarget", load_fixture("get_target_response.json"))
        .on("getWatcher", load_fixture("get_watcher_response.json"))
        .on("watchTargets", load_fixture("watch_targets_response.json"))
        .on(
            "watchResources",
            load_fixture("watch_resources_response.json"),
        )
        .on_with_followups(
            "navigateTo",
            load_fixture("navigate_response.json"),
            vec![
                load_fixture("resources_available_document_event_dom_loading.json"),
                load_fixture("resources_available_document_event_dom_complete.json"),
            ],
        )
        .on(
            "unwatchResources",
            load_fixture("watch_resources_response.json"),
        )
        // refresh_console_actor calls getTarget after the navigate completes.
        .on("getTarget", load_fixture("get_target_response.json"))
        .on_sequence(
            "evaluateJSAsync",
            vec![
                (
                    load_fixture("eval_immediate_response.json"),
                    vec![load_fixture("eval_result_ready_state_complete.json")],
                ),
                (
                    load_fixture("eval_immediate_response.json"),
                    vec![load_fixture("eval_result_string.json")],
                ),
            ],
        )
}

fn navigate_with_network_server() -> MockRdpServer {
    // Resource events are sent as followups to navigateTo, simulating Firefox
    // emitting network events triggered by the navigation. They must arrive
    // after the navigateTo response so the drain loop can pick them up (the
    // actor_request for navigateTo discards messages from other actors).
    MockRdpServer::new()
        .on("listTabs", load_fixture("list_tabs_response.json"))
        .on("getTarget", load_fixture("get_target_response.json"))
        .on("getWatcher", load_fixture("get_watcher_response.json"))
        .on("watchTargets", load_fixture("watch_targets_response.json"))
        .on(
            "watchResources",
            load_fixture("watch_resources_response.json"),
        )
        .on_with_followups(
            "navigateTo",
            load_fixture("navigate_response.json"),
            vec![
                load_fixture("resources_available_network.json"),
                load_fixture("resources_updated_network.json"),
            ],
        )
        .on(
            "unwatchResources",
            load_fixture("watch_resources_response.json"),
        )
}

#[test]
fn navigate_outputs_json_envelope() {
    let server = navigate_server();
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend(["navigate".to_owned(), "https://example.com".to_owned()]);

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

    assert_eq!(json["results"]["navigated"], "https://example.com");
    assert_eq!(json["total"], 1);
}

#[test]
fn navigate_with_jq_extracts_url() {
    let server = navigate_server();
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend([
        "navigate".to_owned(),
        "https://example.com".to_owned(),
        "--jq".to_owned(),
        ".results.navigated".to_owned(),
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
    assert_eq!(stdout.trim(), r#""https://example.com""#);
}

// ---------------------------------------------------------------------------
// --with-network tests
// ---------------------------------------------------------------------------

#[test]
fn navigate_with_network_captures_requests() {
    let server = navigate_with_network_server();
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend([
        "navigate".to_owned(),
        "https://example.com".to_owned(),
        "--with-network".to_owned(),
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

    // The navigated field is present.
    assert_eq!(json["results"]["navigated"], "https://example.com");

    // iter-126: the canonical shape is ONE object on every path, never a bare
    // array. Default mode carries both the summary fields and an `entries` array.
    let network = &json["results"]["network"];
    assert!(network.is_object(), "network should be a canonical object");
    assert_eq!(network["total_requests"], 2, "expected 2 network entries");

    // total reflects the outer envelope (single navigate result).
    assert_eq!(json["total"], 1);

    // Summary contains expected fields.
    assert!(network["total_transfer_bytes"].is_number());
    assert!(network["by_cause_type"].is_object());
    assert!(network["slowest"].is_array());

    // iter-126: `.entries` is reachable (array) even in default/summary mode —
    // no more "cannot index array" when a consumer probes .entries.
    assert!(
        network["entries"].is_array(),
        "network.entries must be an array in default mode, got: {}",
        network["entries"]
    );
    assert_eq!(network["entries"].as_array().unwrap().len(), 2);
    assert_eq!(network["shown"], 2);
    assert_eq!(network["total"], 2);
    assert_eq!(network["truncated"], false);
}

#[test]
fn navigate_with_network_detail_mode_is_object_not_array() {
    // iter-126 regression: --detail (a detail-mode trigger) previously returned
    // a bare array on quiet pages (≤20 entries), so `.results.network.entries`
    // and `.results.network.total_requests` threw "cannot index array". Assert
    // the canonical object shape with both entries and summary fields present.
    let server = navigate_with_network_server();
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend([
        "navigate".to_owned(),
        "https://example.com".to_owned(),
        "--with-network".to_owned(),
        "--detail".to_owned(),
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

    let network = &json["results"]["network"];
    assert!(
        network.is_object(),
        "detail-mode network must be a canonical object, not a bare array, got: {network}"
    );
    // Both entry-level and summary keys are present in detail mode.
    assert!(network["entries"].is_array(), "entries must be an array");
    assert_eq!(network["entries"].as_array().unwrap().len(), 2);
    assert_eq!(network["total_requests"], 2);
    assert!(network["total_transfer_bytes"].is_number());
    assert!(network["slowest"].is_array());
    assert_eq!(network["truncated"], false);
}

#[test]
fn navigate_with_network_all_keeps_object_shape() {
    // iter-126 AC (live_navigate_with_network_all_keeps_summary equivalent under
    // the mock server): --all is a detail-mode trigger that previously produced
    // a bare array dump. Assert it now keeps the object shape with summary fields.
    let server = navigate_with_network_server();
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend([
        "navigate".to_owned(),
        "https://example.com".to_owned(),
        "--with-network".to_owned(),
        "--all".to_owned(),
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

    let network = &json["results"]["network"];
    assert!(
        network.is_object(),
        "--all network must stay an object, not a bare array, got: {network}"
    );
    assert!(network["entries"].is_array());
    assert_eq!(network["entries"].as_array().unwrap().len(), 2);
    assert_eq!(network["total_requests"], 2);
    assert_eq!(network["truncated"], false);
}

#[test]
fn navigate_with_network_respects_network_timeout_flag() {
    // Same fixture setup as navigate_with_network_captures_requests, but we
    // explicitly pass a short --network-timeout and verify the output is the
    // same — the flag is wired through correctly and the drain still collects
    // events that arrive before the timeout.
    let server = navigate_with_network_server();
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend([
        "navigate".to_owned(),
        "https://example.com".to_owned(),
        "--with-network".to_owned(),
        "--network-timeout".to_owned(),
        "500".to_owned(), // 500 ms idle timeout
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

    assert_eq!(json["results"]["navigated"], "https://example.com");

    let network = &json["results"]["network"];
    assert!(network.is_object(), "network should be a summary object");
    assert_eq!(network["total_requests"], 2, "expected 2 network entries");
}

#[test]
fn navigate_with_network_empty_when_no_events() {
    // Server handles the protocol sequence but sends no resource event followups.
    let server = MockRdpServer::new()
        .on("listTabs", load_fixture("list_tabs_response.json"))
        .on("getTarget", load_fixture("get_target_response.json"))
        .on("getWatcher", load_fixture("get_watcher_response.json"))
        .on("watchTargets", load_fixture("watch_targets_response.json"))
        .on(
            "watchResources",
            load_fixture("watch_resources_response.json"),
        )
        .on("navigateTo", load_fixture("navigate_response.json"))
        .on(
            "unwatchResources",
            load_fixture("watch_resources_response.json"),
        );

    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend([
        "navigate".to_owned(),
        "https://example.com".to_owned(),
        "--with-network".to_owned(),
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

    assert_eq!(json["results"]["navigated"], "https://example.com");

    // iter-126: even a zero-request page carries the canonical object with
    // `entries: []` and `total_requests: 0` — keys are present, not omitted.
    let network = &json["results"]["network"];
    assert!(network.is_object(), "network should be a canonical object");
    assert_eq!(network["total_requests"], 0, "expected no network entries");
    assert!(
        network["entries"].is_array(),
        "entries must be [] not absent on a zero-request page, got: {}",
        network["entries"]
    );
    assert_eq!(network["entries"].as_array().unwrap().len(), 0);
    assert_eq!(network["shown"], 0);
    assert_eq!(network["total"], 0);
    assert_eq!(network["truncated"], false);

    assert_eq!(json["total"], 1);
}

// ---------------------------------------------------------------------------
// --wait-text tests (regression: re-resolve console actor after navigation)
// ---------------------------------------------------------------------------

/// Regression test for iter-53 task 1.
///
/// On the very first `navigate --wait-text` after a fresh launch the previous
/// implementation reused the console actor from the pre-navigate `getTarget`,
/// which Firefox invalidates when navigation tears down the docshell.  The
/// fix re-resolves the target after navigation so wait-text uses a fresh
/// console actor.  The mock here records every `getTarget` call and asserts
/// the second one (post-navigate) is observed.
#[test]
fn navigate_wait_text_reresolves_console_actor_after_navigate() {
    use std::sync::atomic::Ordering;

    let mut server = MockRdpServer::new()
        .on("listTabs", load_fixture("list_tabs_response.json"))
        .on("getTarget", load_fixture("get_target_response.json"))
        // Theme A (iter-61v): document-event subscription replaces readyState polling.
        // iter-79 Theme A: watchTargets("frame") is issued before watchResources.
        .on("getWatcher", load_fixture("get_watcher_response.json"))
        .on("watchTargets", load_fixture("watch_targets_response.json"))
        .on(
            "watchResources",
            load_fixture("watch_resources_response.json"),
        )
        .on_with_followups(
            "navigateTo",
            load_fixture("navigate_response.json"),
            vec![
                load_fixture("resources_available_document_event_dom_loading.json"),
                load_fixture("resources_available_document_event_dom_complete.json"),
            ],
        )
        .on(
            "unwatchResources",
            load_fixture("watch_resources_response.json"),
        )
        // refresh_console_actor calls getTarget after the navigate completes.
        .on("getTarget", load_fixture("get_target_response.json"))
        // wait_after_navigate (--wait-text) re-resolves actors and polls once.
        .on("getTarget", load_fixture("get_target_response.json"))
        .on_with_followup(
            "evaluateJSAsync",
            load_fixture("eval_immediate_response.json"),
            load_fixture("eval_result_wait_true.json"),
        );
    let get_target_calls = server.call_counter("getTarget");

    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend([
        "navigate".to_owned(),
        "https://example.com".to_owned(),
        "--wait-text".to_owned(),
        "Success".to_owned(),
        "--wait-timeout".to_owned(),
        "5000".to_owned(),
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
    assert_eq!(json["results"]["navigated"], "https://example.com");
    assert_eq!(json["results"]["wait"]["waited"], true);

    // The crucial assertion: getTarget must be called at least twice — once
    // before navigation (in connect_and_get_target) and once after (in
    // wait_after_navigate). If the fix regresses to caching the pre-navigate
    // console actor, this counter stays at 1.
    assert!(
        get_target_calls.load(Ordering::SeqCst) >= 2,
        "expected getTarget to be re-resolved after navigation; got {} calls",
        get_target_calls.load(Ordering::SeqCst)
    );
}
