use super::support::{self, MockRdpServer, load_fixture};
use serde_json::json;

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

fn console_server() -> MockRdpServer {
    MockRdpServer::new()
        .on("listTabs", load_fixture("list_tabs_response.json"))
        .on("getTarget", load_fixture("get_target_response.json"))
        .on(
            "startListeners",
            load_fixture("start_listeners_response.json"),
        )
        .on(
            "getCachedMessages",
            load_fixture("get_cached_messages_response.json"),
        )
}

// ---------------------------------------------------------------------------
// Happy-path tests
// ---------------------------------------------------------------------------

#[test]
fn console_shows_all_messages() {
    let server = console_server();
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.push("console".to_owned());

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
    let results = json["results"].as_array().expect("results is array");
    // Default sort is timestamp desc (newest first).
    assert_eq!(results[0]["level"], "error");
    assert_eq!(results[0]["message"], "error msg");
    assert_eq!(results[1]["level"], "warn");
    assert_eq!(results[1]["message"], "warning msg");
    assert_eq!(results[2]["level"], "log");
    assert_eq!(results[2]["message"], "hello from test");
}

#[test]
fn console_filter_by_level() {
    let server = console_server();
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend([
        "console".to_owned(),
        "--level".to_owned(),
        "error".to_owned(),
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
    assert_eq!(results[0]["level"], "error");
    assert_eq!(results[0]["message"], "error msg");
}

#[test]
fn console_filter_by_pattern() {
    let server = console_server();
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend([
        "console".to_owned(),
        "--pattern".to_owned(),
        "warn".to_owned(),
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
    assert_eq!(results[0]["message"], "warning msg");
}

#[test]
fn console_with_jq_filter() {
    let server = console_server();
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend([
        "console".to_owned(),
        "--jq".to_owned(),
        ".results[] | select(.level == \"error\") | .message".to_owned(),
    ]);

    let output = std::process::Command::new(ff_rdp_bin())
        .args(&args)
        .output()
        .expect("failed to spawn ff-rdp");

    handle.join().unwrap();

    assert!(output.status.success());

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(stdout.trim(), r#""error msg""#);
}

#[test]
fn console_level_and_pattern_combined() {
    let server = console_server();
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend([
        "console".to_owned(),
        "--level".to_owned(),
        "log".to_owned(),
        "--pattern".to_owned(),
        "hello".to_owned(),
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
    assert_eq!(results[0]["message"], "hello from test");
}

#[test]
fn console_no_match_returns_empty() {
    let server = console_server();
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend([
        "console".to_owned(),
        "--level".to_owned(),
        "debug".to_owned(),
    ]);

    let output = std::process::Command::new(ff_rdp_bin())
        .args(&args)
        .output()
        .expect("failed to spawn ff-rdp");

    handle.join().unwrap();

    assert!(output.status.success());

    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["total"], 0);
    assert_eq!(json["results"].as_array().unwrap().len(), 0);
}

#[test]
fn console_handles_page_error_messages() {
    // Verify that pageError-type messages are parsed alongside consoleAPICall messages.
    let response_with_page_errors = serde_json::json!({
        "from": "server1.conn0.child2/consoleActor3",
        "messages": [
            {
                "message": {
                    "arguments": ["hello"],
                    "level": "log",
                    "filename": "test.js",
                    "lineNumber": 1,
                    "columnNumber": 1,
                    "timeStamp": 1000.0
                },
                "type": "consoleAPICall"
            },
            {
                "pageError": {
                    "errorMessage": "ReferenceError: foo is not defined",
                    "sourceName": "https://example.com/app.js",
                    "lineNumber": 42,
                    "columnNumber": 5,
                    "timeStamp": 2000.0
                },
                "type": "pageError"
            }
        ]
    });

    let server = MockRdpServer::new()
        .on("listTabs", load_fixture("list_tabs_response.json"))
        .on("getTarget", load_fixture("get_target_response.json"))
        .on(
            "startListeners",
            load_fixture("start_listeners_response.json"),
        )
        .on("getCachedMessages", response_with_page_errors);

    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.push("console".to_owned());

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
    // Default sort is timestamp desc (newest first).
    // pageError has timeStamp=2000, consoleAPICall has timeStamp=1000.
    assert_eq!(results[0]["level"], "error");
    assert_eq!(results[0]["message"], "ReferenceError: foo is not defined");
    assert_eq!(results[1]["level"], "log");
    assert_eq!(results[1]["message"], "hello");
}

// ---------------------------------------------------------------------------
// summary field
// ---------------------------------------------------------------------------

fn large_console_server() -> MockRdpServer {
    MockRdpServer::new()
        .on("listTabs", load_fixture("list_tabs_response.json"))
        .on("getTarget", load_fixture("get_target_response.json"))
        .on(
            "startListeners",
            load_fixture("start_listeners_response.json"),
        )
        .on(
            "getCachedMessages",
            load_fixture("get_cached_messages_large_response.json"),
        )
}

#[test]
fn console_summary_present_in_output() {
    let server = console_server();
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.push("console".to_owned());

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
    let summary = &json["summary"];
    assert!(!summary.is_null(), "summary field must be present");
    assert_eq!(summary["total"], 3, "total = raw message count");
    assert_eq!(
        summary["matched"], 3,
        "matched = after filter, before limit"
    );
    assert_eq!(summary["shown"], 3, "shown = actual results length");
    let by_level = summary["by_level"].as_object().unwrap();
    assert_eq!(by_level["log"], 1);
    assert_eq!(by_level["warn"], 1);
    assert_eq!(by_level["error"], 1);
}

#[test]
fn console_limit_10_summary_reflects_true_total() {
    let server = large_console_server();
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend(["--limit".to_owned(), "10".to_owned(), "console".to_owned()]);

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

    // --limit 10 should only show 10 results.
    let results = json["results"].as_array().unwrap();
    assert_eq!(
        results.len(),
        10,
        "results.length must be 10 with --limit 10"
    );

    // summary.total reflects the raw fixture count (110), not the truncated count.
    let summary = &json["summary"];
    assert_eq!(
        summary["total"], 110,
        "summary.total must reflect all 110 messages in the fixture"
    );
    assert_eq!(summary["matched"], 110, "matched = all 110 (no filter)");
    assert_eq!(summary["shown"], 10, "shown = 10 after --limit");
}

#[test]
fn console_summary_matched_reflects_filter_count() {
    // With --level error only 22 of 110 messages match (110/5 = 22 error messages).
    let server = large_console_server();
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend([
        "console".to_owned(),
        "--level".to_owned(),
        "error".to_owned(),
    ]);

    let output = std::process::Command::new(ff_rdp_bin())
        .args(&args)
        .output()
        .expect("failed to spawn ff-rdp");

    handle.join().unwrap();

    assert!(output.status.success());

    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let summary = &json["summary"];
    assert_eq!(
        summary["total"], 110,
        "total is always the raw pre-filter count"
    );
    assert_eq!(
        summary["matched"], 22,
        "matched reflects count after --level filter"
    );
    assert_eq!(
        summary["shown"], 22,
        "all 22 matched fit within default limit"
    );
}

// ---------------------------------------------------------------------------
// --limit flag truncates results
// ---------------------------------------------------------------------------

#[test]
fn console_limit_truncates_results() {
    let server = console_server();
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend(["--limit".to_owned(), "2".to_owned(), "console".to_owned()]);

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
    // Total reflects actual count (3), but only 2 shown.
    assert_eq!(json["total"], 3);
    assert_eq!(json["results"].as_array().unwrap().len(), 2);
    assert_eq!(json["truncated"], true);
    assert!(json["hint"].as_str().unwrap().contains("--all"));
}

// ---------------------------------------------------------------------------
// --follow flag: streams messages as NDJSON until connection closes
// ---------------------------------------------------------------------------

fn follow_server_with_events(console_event: serde_json::Value) -> MockRdpServer {
    // `close_after_followups` causes the server to drop the connection
    // immediately after delivering the console event followup.  The client's
    // follow_loop then receives EOF and exits cleanly without blocking.
    MockRdpServer::new()
        .on("listTabs", load_fixture("list_tabs_response.json"))
        .on("getTarget", load_fixture("get_target_response.json"))
        .on("getWatcher", load_fixture("get_watcher_response.json"))
        // iter-252: `run_follow_direct` now issues `watchTargets("frame")`
        // before `watchResources`, because the content-process half of
        // `watchResources` only reaches targets the watcher itself created.
        // Without a handler here the mock answers `unknownMethod` and the
        // command exits 3 before any followup is delivered.
        .on("watchTargets", load_fixture("watch_targets_response.json"))
        .on_with_followups(
            "watchResources",
            load_fixture("watch_resources_response.json"),
            vec![console_event],
        )
        .on(
            "unwatchResources",
            load_fixture("unwatch_resources_response.json"),
        )
        .close_after_followups()
}

fn assert_follow_subscription_requests(requests: &[serde_json::Value]) {
    let watcher = requests.iter().find(|r| r["type"] == "getWatcher").unwrap();
    assert_eq!(
        watcher["isServerTargetSwitchingEnabled"], true,
        "direct follow needs server target switching: {watcher}"
    );
    let targets = requests.iter().position(|r| r["type"] == "watchTargets");
    let resources = requests
        .iter()
        .position(|r| r["type"] == "watchResources")
        .unwrap();
    assert!(
        targets.is_some_and(|targets| targets < resources),
        "watchTargets must precede watchResources: {requests:?}"
    );
    assert_eq!(requests[targets.unwrap()]["targetType"], "frame");
}

#[test]
fn console_follow_streams_messages_as_ndjson() {
    let console_event = json!({
        "type": "resources-available-array",
        "from": "server1.conn0.watcher4",
        "array": [
            ["console-message", [
                {
                    "resourceType": "console-message",
                    "message": {
                        "arguments": ["live message 1"],
                        "level": "log",
                        "filename": "test.js",
                        "lineNumber": 1,
                        "columnNumber": 1,
                        "timeStamp": 1000.0
                    }
                },
                {
                    "resourceType": "console-message",
                    "message": {
                        "arguments": ["live message 2"],
                        "level": "warn",
                        "filename": "test.js",
                        "lineNumber": 2,
                        "columnNumber": 1,
                        "timeStamp": 2000.0
                    }
                }
            ]]
        ]
    });

    let server = follow_server_with_events(console_event);
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend(["console".to_owned(), "--follow".to_owned()]);

    let output = std::process::Command::new(ff_rdp_bin())
        .args(&args)
        .output()
        .expect("failed to spawn ff-rdp");

    handle.join().expect("server thread panicked");

    assert!(
        output.status.success(),
        "expected success, stderr: {}",
        support::output_note(&output)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    // Each message is emitted as a separate JSON line (NDJSON).
    let lines: Vec<&str> = stdout.trim().lines().collect();
    assert_eq!(lines.len(), 2, "expected 2 NDJSON lines, got: {stdout}");

    let msg1: serde_json::Value =
        serde_json::from_str(lines[0]).expect("line 1 must be valid JSON");
    assert_eq!(msg1["level"], "log");
    assert_eq!(msg1["message"], "live message 1");
    assert_eq!(msg1["source"], "test.js");

    let msg2: serde_json::Value =
        serde_json::from_str(lines[1]).expect("line 2 must be valid JSON");
    assert_eq!(msg2["level"], "warn");
    assert_eq!(msg2["message"], "live message 2");
}

/// iter-252: the payload Firefox actually sends for a `console-message`
/// **resource** is flat — `resources/console-messages.js:55` hands
/// `prepareConsoleMessageForRemote`'s result straight to `onAvailable`, with no
/// `message` wrapper (the wrapper belongs to the legacy `consoleAPICall` push,
/// `webconsole.js:1453`). The event below is recorded verbatim off the wire on
/// Firefox 155.0.1.
///
/// Until iter-252 every such item parsed to `None`, so `console --follow`
/// printed nothing at all even on the daemon route, which received every frame.
/// That is why iteration 174's attempt to measure `console --follow` saw empty
/// stdout on both routes and could conclude nothing. The sibling tests above
/// all use the wrapped shape and therefore could not catch it.
#[test]
fn console_follow_streams_flat_console_message_resources() {
    // Recorded by live_252_record_preformatted_console_deliveries. Replay
    // both channels, including their distinct object/longString/symbol handles.
    let recording = load_fixture("console_follow_preformatted_events.json");
    let mut events = recording.as_array().unwrap().clone().into_iter();
    let console_event = events.next().unwrap();
    let mut followups: Vec<_> = events.collect();
    followups.push(load_fixture("watch_resources_response.json"));

    // A content-process resource can precede the watchResources ACK. Replay
    // this recorded shape before the ACK and ensure setup retains it.
    let server = MockRdpServer::new()
        .on("listTabs", load_fixture("list_tabs_response.json"))
        .on("getTarget", load_fixture("get_target_response.json"))
        .on("getWatcher", load_fixture("get_watcher_response.json"))
        .on("watchTargets", load_fixture("watch_targets_response.json"))
        .on_with_followups("watchResources", console_event, followups)
        .close_after_followups();
    let port = server.port();
    let requests = server.request_log();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend(["console".to_owned(), "--follow".to_owned()]);

    let output = std::process::Command::new(ff_rdp_bin())
        .args(&args)
        .output()
        .expect("failed to spawn ff-rdp");

    handle.join().expect("server thread panicked");
    assert_follow_subscription_requests(&requests.lock().unwrap());

    assert!(
        output.status.success(),
        "expected success, stderr: {}",
        support::output_note(&output)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<&str> = stdout.trim().lines().collect();
    assert_eq!(
        lines.len(),
        8,
        "flat console-message resources must stream like wrapped ones, got: {stdout}"
    );

    let msg1: serde_json::Value =
        serde_json::from_str(lines[0]).expect("line 1 must be valid JSON");
    assert_eq!(msg1["level"], "log");
    assert_eq!(msg1["message"], "iter252-record:literal:%s");
    assert_eq!(msg1["source"], "debugger eval code");
    assert_eq!(msg1["line"], 1);

    // Formatting already ran in Firefox; percent tokens must remain literal.
    let msg2: serde_json::Value =
        serde_json::from_str(lines[1]).expect("line 2 must be valid JSON");
    assert_eq!(msg2["level"], "log");
    assert_eq!(msg2["message"], "iter252-record:substituted:%s");
    let msg3: serde_json::Value = serde_json::from_str(lines[2]).unwrap();
    let grip: serde_json::Value = serde_json::from_str(msg3["message"].as_str().unwrap()).unwrap();
    assert_eq!(grip["type"], "longString");
    assert_eq!(grip["length"], 10020);
    assert!(
        grip["actor"].as_str().is_some(),
        "output must retain grip handles"
    );
    let emitted: Vec<serde_json::Value> = lines
        .iter()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let symbol: serde_json::Value =
        serde_json::from_str(emitted[3]["message"].as_str().unwrap()).unwrap();
    assert_eq!(symbol["type"], "symbol");
    assert_eq!(symbol["name"], "iter252-record:symbol");
    assert!(symbol["actor"].as_str().is_some());
    let object: serde_json::Value =
        serde_json::from_str(emitted[4]["message"].as_str().unwrap()).unwrap();
    assert!(object["actor"].as_str().is_some());
    let properties = &object["preview"]["ownProperties"];
    assert_eq!(properties["type"]["value"], "symbol");
    assert_eq!(properties["actor"]["value"], "user-data");
    assert_eq!(
        properties["nested"]["value"]["name"],
        "iter252-record:nested"
    );
    assert!(properties["nested"]["value"]["actor"].as_str().is_some());
    let named: serde_json::Value =
        serde_json::from_str(emitted[5]["message"].as_str().unwrap()).unwrap();
    assert_eq!(named["type"], "symbol");
    assert!(named["actor"].as_str().is_some());
    assert_eq!(named["name"]["type"], "longString");
    assert!(named["name"]["actor"].as_str().is_some());
    assert!(
        named["name"]["initial"]
            .as_str()
            .unwrap()
            .starts_with("iter252-record:long-name:")
    );
    assert!(
        emitted[6]["message"]
            .as_str()
            .unwrap()
            .contains("iter252-record:unnamed")
    );
    assert!(
        emitted[7]["message"]
            .as_str()
            .unwrap()
            .contains("12345678901234567890")
    );
}

#[test]
fn console_follow_releases_grips_with_interleaved_events_and_replies() {
    use ff_rdp_core::transport::{encode_frame, recv_from};
    use std::collections::BTreeSet;
    use std::io::{BufReader, Write};
    use std::net::TcpListener;

    // Each filter must release all fourteen separately allocated handles from
    // the recorded resource and legacy copies, including nested symbol names.
    for filter in [
        vec![],
        vec!["--level", "warn"],
        vec!["--pattern", "no-match"],
    ] {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            socket
                .set_read_timeout(Some(std::time::Duration::from_secs(4)))
                .unwrap();
            let mut reader = BufReader::new(socket.try_clone().unwrap());
            let send = |socket: &mut std::net::TcpStream, packet: &serde_json::Value| {
                socket
                    .write_all(encode_frame(&packet.to_string()).as_bytes())
                    .unwrap();
            };
            send(&mut socket, &load_fixture("handshake.json"));
            let recording = load_fixture("console_follow_preformatted_events.json");
            let events = recording.as_array().unwrap();
            let release = load_fixture("console_follow_release_replies.json");
            let mut released = BTreeSet::new();
            let mut injected = false;
            while let Ok(request) = recv_from(&mut reader) {
                let fixture = match request["type"].as_str().unwrap() {
                    "getRoot" => "get_root_screenshot_response.json",
                    "listTabs" => "list_tabs_response.json",
                    "getTarget" => "get_target_response.json",
                    "getWatcher" => "get_watcher_response.json",
                    "watchTargets" => "watch_targets_response.json",
                    "watchResources" => {
                        // An older catch-up record remains queued when the
                        // first grip release is sent. It must precede the
                        // further events interleaved with release replies.
                        for event in &events[..5] {
                            send(&mut socket, event);
                        }
                        "watch_resources_response.json"
                    }
                    "release" => {
                        let actor = request["to"].as_str().unwrap();
                        released.insert(actor.to_owned());
                        if !injected {
                            for event in &events[5..8] {
                                send(&mut socket, event);
                            }
                        }
                        // Firefox 155 symbols have no ACK. Object and string
                        // replies are actual recorded packets, with only the
                        // session actor ID substituted as in MockRdpServer.
                        if !actor.contains("/symbol") {
                            let kind = if actor.contains("/longstr") {
                                "longString"
                            } else {
                                "object"
                            };
                            let mut reply = release[kind][0].clone();
                            reply["from"] = request["to"].clone();
                            send(&mut socket, &reply);
                        }
                        if !injected {
                            for event in &events[8..] {
                                send(&mut socket, event);
                            }
                            injected = true;
                        }
                        if released.len() == 14 {
                            break;
                        }
                        continue;
                    }
                    method => panic!("unexpected request: {method}"),
                };
                let mut reply = load_fixture(fixture);
                reply["from"] = request["to"].clone();
                send(&mut socket, &reply);
            }
            released
        });
        let mut args = base_args(port);
        args.extend(["console".to_owned(), "--follow".to_owned()]);
        args.extend(filter.iter().map(|arg| (*arg).to_owned()));
        let output = std::process::Command::new(ff_rdp_bin())
            .args(args)
            .output()
            .unwrap();
        let released = server.join().unwrap();
        assert_eq!(
            released.len(),
            14,
            "all grips, including filtered/duplicate/nested: {released:?}"
        );
        assert!(output.status.success(), "{}", support::output_note(&output));
        let lines: Vec<serde_json::Value> = String::from_utf8(output.stdout)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(lines.len(), if filter.is_empty() { 8 } else { 0 });
        if filter.is_empty() {
            let expected = [
                "literal",
                "substituted",
                "long",
                "symbol",
                "nested",
                "long-name",
                "unnamed",
                "bigint",
            ];
            for (entry, label) in lines.iter().zip(expected) {
                assert!(
                    entry["message"]
                        .as_str()
                        .unwrap()
                        .contains(&format!("iter252-record:{label}")),
                    "wire ordering: {lines:?}"
                );
            }
        }
    }
}

#[test]
fn console_follow_level_filter_applies_to_stream() {
    // Only warn-level messages should be emitted when --level warn is given.
    let console_event = json!({
        "type": "resources-available-array",
        "from": "server1.conn0.watcher4",
        "array": [
            ["console-message", [
                {
                    "resourceType": "console-message",
                    "message": {
                        "arguments": ["debug noise"],
                        "level": "debug",
                        "filename": "app.js",
                        "lineNumber": 10,
                        "columnNumber": 1,
                        "timeStamp": 500.0
                    }
                },
                {
                    "resourceType": "console-message",
                    "message": {
                        "arguments": ["important warning"],
                        "level": "warn",
                        "filename": "app.js",
                        "lineNumber": 20,
                        "columnNumber": 1,
                        "timeStamp": 600.0
                    }
                }
            ]]
        ]
    });

    let server = follow_server_with_events(console_event);
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend([
        "console".to_owned(),
        "--follow".to_owned(),
        "--level".to_owned(),
        "warn".to_owned(),
    ]);

    let output = std::process::Command::new(ff_rdp_bin())
        .args(&args)
        .output()
        .expect("failed to spawn ff-rdp");

    handle.join().expect("server thread panicked");

    assert!(
        output.status.success(),
        "expected success, stderr: {}",
        support::output_note(&output)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<&str> = stdout.trim().lines().collect();
    assert_eq!(lines.len(), 1, "expected only 1 (warn) line, got: {stdout}");

    let msg: serde_json::Value = serde_json::from_str(lines[0]).expect("output must be valid JSON");
    assert_eq!(msg["level"], "warn");
    assert_eq!(msg["message"], "important warning");
}

// ---------------------------------------------------------------------------
// Direct consoleAPICall push (Firefox 149+ eval-triggered messages)
// ---------------------------------------------------------------------------

/// Build a server that delivers a direct `consoleAPICall` push notification
/// (the path taken by Firefox 149+ when `console.log()` is called via eval).
///
/// The `consoleAPICall` event arrives directly from the console actor rather
/// than via the Watcher's `resources-available-array` stream.
fn follow_server_with_direct_notification(notification: serde_json::Value) -> MockRdpServer {
    MockRdpServer::new()
        .on("listTabs", load_fixture("list_tabs_response.json"))
        .on("getTarget", load_fixture("get_target_response.json"))
        .on(
            "startListeners",
            load_fixture("start_listeners_response.json"),
        )
        .on("getWatcher", load_fixture("get_watcher_response.json"))
        // iter-252: see the note in `follow_server_with_events`.
        .on("watchTargets", load_fixture("watch_targets_response.json"))
        .on_with_followups(
            "watchResources",
            load_fixture("watch_resources_response.json"),
            vec![notification],
        )
        .on(
            "unwatchResources",
            load_fixture("unwatch_resources_response.json"),
        )
        .close_after_followups()
}

#[test]
fn console_follow_handles_direct_consoleapicall_notification() {
    // Simulate Firefox 149+ sending a `consoleAPICall` push directly on the
    // console actor (triggered by console.log() called from evaluateJSAsync).
    let notification = json!({
        "type": "consoleAPICall",
        "from": "server1.conn0.child2/consoleActor3",
        "message": {
            "arguments": ["eval log output"],
            "level": "log",
            "filename": "debugger eval code",
            "lineNumber": 1,
            "columnNumber": 9,
            "timeStamp": 1_775_439_071_165.699_f64
        }
    });

    let server = follow_server_with_direct_notification(notification);
    let port = server.port();
    let requests = server.request_log();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend(["console".to_owned(), "--follow".to_owned()]);

    let output = std::process::Command::new(ff_rdp_bin())
        .args(&args)
        .output()
        .expect("failed to spawn ff-rdp");

    handle.join().expect("server thread panicked");
    assert_follow_subscription_requests(&requests.lock().unwrap());

    assert!(
        output.status.success(),
        "expected success, stderr: {}",
        support::output_note(&output)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<&str> = stdout.trim().lines().collect();
    assert_eq!(
        lines.len(),
        1,
        "expected 1 NDJSON line from direct push, got: {stdout}"
    );

    let msg: serde_json::Value = serde_json::from_str(lines[0]).expect("output must be valid JSON");
    assert_eq!(msg["level"], "log");
    assert_eq!(msg["message"], "eval log output");
    assert_eq!(msg["source"], "debugger eval code");
}

#[test]
fn console_follow_handles_direct_pageerror_notification() {
    // Simulate Firefox 149+ sending a `pageError` push directly on the
    // console actor.
    let notification = json!({
        "type": "pageError",
        "from": "server1.conn0.child2/consoleActor3",
        "pageError": {
            "errorMessage": "ReferenceError: x is not defined",
            "sourceName": "https://example.com/app.js",
            "lineNumber": 42,
            "columnNumber": 5,
            "timeStamp": 1_775_439_071_200.0_f64
        }
    });

    let server = follow_server_with_direct_notification(notification);
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend(["console".to_owned(), "--follow".to_owned()]);

    let output = std::process::Command::new(ff_rdp_bin())
        .args(&args)
        .output()
        .expect("failed to spawn ff-rdp");

    handle.join().expect("server thread panicked");

    assert!(
        output.status.success(),
        "expected success, stderr: {}",
        support::output_note(&output)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<&str> = stdout.trim().lines().collect();
    assert_eq!(
        lines.len(),
        1,
        "expected 1 NDJSON line from direct pageError push, got: {stdout}"
    );

    let msg: serde_json::Value = serde_json::from_str(lines[0]).expect("output must be valid JSON");
    assert_eq!(msg["level"], "error");
    assert_eq!(msg["message"], "ReferenceError: x is not defined");
    assert_eq!(msg["source"], "https://example.com/app.js");
}
