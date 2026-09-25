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
    //   unwatchResources → unwatchTargets → return (plain committed navigate)
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
        support::output_note(&output)
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
        support::output_note(&output)
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
        support::output_note(&output)
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
        support::output_note(&output)
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
        support::output_note(&output)
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
        support::output_note(&output)
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
        support::output_note(&output)
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
        support::output_note(&output)
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

/// Same-operation records must survive a real CLI/mock-RDP invocation and
/// identify its PID, ordering and monotonic offsets without changing JSON.
#[test]
fn e2e_279_navigation_timing_records_same_command() {
    let server = navigate_server();
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());
    let child = std::process::Command::new(ff_rdp_bin())
        .args(base_args(port))
        .args(["navigate", "https://example.com"])
        .env("RUST_LOG", "ff_rdp_cli::navigation_timing=debug")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("spawn navigate");
    let pid = child.id();
    let output = child.wait_with_output().expect("wait navigate");
    handle.join().unwrap();
    assert!(output.status.success(), "{}", support::output_note(&output));
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["results"]["navigated"], "https://example.com");
    assert!(json["results"]["elapsed_ms"].is_number());
    let stderr = String::from_utf8_lossy(&output.stderr);
    for (scope, stages) in [
        (
            "core",
            &[
                "entry",
                "connected",
                "dispatch",
                "commit_resolved",
                "return_before_drop",
            ][..],
        ),
        (
            "run",
            &[
                "entry",
                "core_call",
                "core_return_after_drop",
                "output_begin",
                "output_end",
            ][..],
        ),
        (
            "connect",
            &["entry", "route_resolved", "greeted", "attached"][..],
        ),
        ("list_tabs", &["entry", "parsed"][..]),
        (
            "attach",
            &[
                "entry",
                "version_resolved",
                "tab_selected",
                "target_acquired",
                "registered",
            ][..],
        ),
        (
            "setup",
            &[
                "connected",
                "watcher_ready",
                "epoch_sampled",
                "href_sampled",
                "targets_watched",
                "subscribed",
                "ready_to_dispatch",
            ][..],
        ),
        (
            "postcore",
            &["connection_meta_begin", "connection_meta_end"][..],
        ),
    ] {
        let prefix = format!("NAV_TIMING pid={pid} scope={scope} stage=");
        let rows: Vec<_> = stderr
            .lines()
            .filter_map(|line| line.split_once(&prefix).map(|(_, row)| row))
            .collect();
        assert_eq!(
            rows.len(),
            stages.len(),
            "{}",
            support::output_note(&output)
        );
        let mut previous = 0;
        for (row, stage) in rows.iter().zip(stages) {
            let (actual, ns) = row.split_once(" elapsed_ns=").expect("timing fields");
            assert_eq!(actual, *stage);
            let ns: u128 = ns.trim().parse().expect("monotonic nanoseconds");
            assert!(ns >= previous);
            previous = ns;
        }
    }
}

/// A completed plain navigation owns no further target consumer. Preserve the
/// subscription and commit path, but do not fetch a target after tearing it down.
#[test]
fn e2e_279_plain_commit_finishes_without_unused_target_refresh() {
    let server = navigate_server();
    let requests = server.request_log();
    let port = server.port();
    let peer = std::thread::spawn(move || server.serve_one());
    let output = std::process::Command::new(ff_rdp_bin())
        .args(base_args(port))
        .args(["navigate", "https://example.com"])
        .output()
        .expect("spawn navigate");
    peer.join().expect("mock peer returned");
    assert!(output.status.success(), "{}", support::output_note(&output));
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["results"]["navigated"], "https://example.com");
    assert_eq!(json["results"]["ready_state"], "complete");
    assert!(json["results"]["elapsed_ms"].is_number());
    assert!(json["results"].get("status").is_some());
    let requests = requests.lock().unwrap();
    let methods: Vec<_> = requests
        .iter()
        .map(|r| r["type"].as_str().unwrap())
        .collect();
    let first = |method| methods.iter().position(|m| *m == method).unwrap();
    let teardown = methods
        .iter()
        .rposition(|m| *m == "unwatchTargets")
        .unwrap();
    assert!(first("getTarget") < first("navigateTo"), "{methods:?}");
    assert!(
        first("watchTargets") < first("watchResources"),
        "{methods:?}"
    );
    assert!(first("watchResources") < first("navigateTo"), "{methods:?}");
    assert!(
        first("navigateTo") < first("unwatchResources"),
        "{methods:?}"
    );
    assert!(first("unwatchResources") < teardown, "{methods:?}");
    assert!(
        !methods[teardown + 1..].contains(&"getTarget"),
        "unused target fetch after subscription teardown: {methods:?}"
    );
}

/// Replay recorded packets with an actor change at navigateTo. Unlike a total
/// getTarget count, the wire log identifies which document receives each eval.
fn actor_change_peer(
    connections: usize,
) -> (
    u16,
    std::thread::JoinHandle<Vec<(usize, serde_json::Value)>>,
) {
    use ff_rdp_core::transport::{encode_frame, recv_from};
    use std::io::{BufReader, Write as _};
    use std::time::{Duration, Instant};

    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    listener.set_nonblocking(true).unwrap();
    let peer = std::thread::spawn(move || {
        let mut requests = Vec::new();
        let mut navigated = false;
        for connection in 0..connections {
            let deadline = Instant::now() + Duration::from_secs(5);
            let mut stream = loop {
                match listener.accept() {
                    Ok((stream, _)) => break stream,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(Instant::now() < deadline, "missing connection {connection}");
                        std::thread::sleep(Duration::from_millis(5));
                    }
                    Err(e) => panic!("accept: {e}"),
                }
            };
            stream.set_nonblocking(false).unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            stream
                .set_write_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let send = |stream: &mut std::net::TcpStream, value: &serde_json::Value| {
                stream
                    .write_all(encode_frame(&value.to_string()).as_bytes())
                    .unwrap();
            };
            send(
                &mut stream,
                &serde_json::json!({"from":"root", "applicationType":"browser", "traits":{}}),
            );
            loop {
                let request = match recv_from(&mut reader) {
                    Ok(request) => request,
                    Err(ff_rdp_core::ProtocolError::RecvFailed(error))
                        if matches!(
                            error.kind(),
                            std::io::ErrorKind::UnexpectedEof | std::io::ErrorKind::ConnectionReset
                        ) =>
                    {
                        break;
                    }
                    Err(error) => panic!("mock receive failed: {error}"),
                };
                requests.push((connection, request.clone()));
                let method = request["type"].as_str().unwrap();
                let mut followups = Vec::new();
                let mut reply = match method {
                    "listTabs" => load_fixture("list_tabs_response.json"),
                    "getRoot" => serde_json::json!({"error":"unknownMethod"}),
                    "getTarget" => {
                        let mut target = load_fixture("get_target_response.json");
                        if navigated {
                            target["frame"]["consoleActor"] =
                                serde_json::json!("new-document/console");
                            target["frame"]["innerWindowId"] = serde_json::json!(2);
                        }
                        target
                    }
                    "getWatcher" => load_fixture("get_watcher_response.json"),
                    "watchTargets" => load_fixture("watch_targets_response.json"),
                    "watchResources" | "unwatchResources" | "unwatchTargets" => {
                        load_fixture("watch_resources_response.json")
                    }
                    "navigateTo" => {
                        navigated = true;
                        followups = vec![
                            load_fixture("resources_available_document_event_dom_loading.json"),
                            load_fixture("resources_available_document_event_dom_complete.json"),
                        ];
                        load_fixture("navigate_response.json")
                    }
                    "evaluateJSAsync" => {
                        let js = request["text"].as_str().unwrap();
                        let mut result = if js.len() > 10_000 {
                            load_fixture("eval_result_a11y_summary.json")
                        } else {
                            load_fixture("eval_result_wait_true.json")
                        };
                        if js.contains("window.location.href")
                            || js.contains("document.location.href")
                        {
                            result["result"] = serde_json::json!("https://example.com/");
                        } else if js.contains("document.readyState") && js.len() < 10_000 {
                            result["result"] = serde_json::json!("complete");
                        }
                        result["from"] = request["to"].clone();
                        followups.push(result);
                        load_fixture("eval_immediate_response.json")
                    }
                    _ => panic!("unexpected request: {request}"),
                };
                reply["from"] = request["to"].clone();
                send(&mut stream, &reply);
                for followup in followups {
                    send(&mut stream, &followup);
                }
            }
        }
        requests
    });
    (port, peer)
}

#[test]
fn e2e_279_postcommit_consumers_use_new_document_actor() {
    for flags in [
        vec!["--wait-text", "Success"],
        vec!["--wait-selector", ".results"],
        vec!["--wait-for", "text:Success"],
        vec!["--with-page"],
        vec!["--no-wait"],
    ] {
        let (port, peer) = actor_change_peer(1);
        let output = std::process::Command::new(ff_rdp_bin())
            .args(base_args(port))
            .args(["navigate", "https://example.com"])
            .args(&flags)
            .output()
            .expect("spawn navigate consumer");
        let requests = peer.join().expect("mock peer returned");
        assert!(
            output.status.success(),
            "{flags:?}: {}",
            support::output_note(&output)
        );
        let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(json["results"]["navigated"], "https://example.com");
        let navigation = requests
            .iter()
            .position(|(_, r)| r["type"] == "navigateTo")
            .unwrap();
        let refresh = requests
            .iter()
            .rposition(|(_, r)| r["type"] == "getTarget")
            .unwrap();
        assert!(refresh > navigation, "{flags:?}: {requests:?}");
        if flags == ["--no-wait"] {
            assert!(json["results"].get("elapsed_ms").is_none());
        } else {
            let eval = requests
                .iter()
                .rposition(|(_, r)| r["type"] == "evaluateJSAsync")
                .unwrap();
            assert!(eval > refresh, "{flags:?}: {requests:?}");
            assert_eq!(requests[eval].1["to"], "new-document/console", "{flags:?}");
            if flags == ["--with-page"] {
                assert_eq!(
                    json["results"]["page"]["headings"][0]["text"],
                    "Example Domain"
                );
            } else if flags[0] == "--wait-for" {
                assert_eq!(json["results"]["wait_for"]["waited"], true);
            } else {
                assert_eq!(json["results"]["wait"]["waited"], true);
            }
        }
    }
}

#[test]
fn e2e_279_script_navigate_then_eval_connects_to_new_document() {
    let (port, peer) = actor_change_peer(2);
    let dir = tempfile::tempdir().unwrap();
    let script = dir.path().join("navigate-eval.json");
    std::fs::write(&script, r#"{"version":1,"steps":[{"navigate":{"url":"https://example.com"}},{"eval":{"script":"true"}}]}"#).unwrap();
    let output = std::process::Command::new(ff_rdp_bin())
        .args(base_args(port))
        .arg("run")
        .arg(script)
        .output()
        .expect("spawn script");
    let requests = peer.join().expect("both mock connections returned");
    assert!(output.status.success(), "{}", support::output_note(&output));
    let rows: Vec<serde_json::Value> = String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let eval_result = rows.iter().find(|row| row["verb"] == "eval").unwrap();
    assert_eq!(eval_result["ok"], true);
    assert_eq!(eval_result["results"]["eval"], true);
    let eval_connection: Vec<_> = requests
        .iter()
        .filter(|(connection, _)| *connection == 1)
        .map(|(_, r)| r)
        .collect();
    let refresh = eval_connection
        .iter()
        .position(|r| r["type"] == "getTarget")
        .unwrap();
    let eval = eval_connection
        .iter()
        .position(|r| r["type"] == "evaluateJSAsync")
        .unwrap();
    assert!(refresh < eval, "{requests:?}");
    assert_eq!(eval_connection[eval]["to"], "new-document/console");
}
