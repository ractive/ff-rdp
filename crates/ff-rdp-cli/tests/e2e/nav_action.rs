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

/// Build a mock server for the iter-130 Theme B `back`/`forward`/plain-`reload`
/// flow, which now waits on `document-event` resources for the same
/// `{committed_url, ready_state, elapsed_ms}` envelope `navigate` produces
/// (see `wait_for_navigation_commit` in `navigate.rs`) instead of returning
/// immediately after dispatch.
///
/// Request order: listTabs → getTarget → getWatcher → evaluateJSAsync
/// (pre-nav epoch, best-effort) → evaluateJSAsync (pre-nav `location.href`
/// baseline for the iter-138 Theme B/C same-document check) → watchTargets →
/// watchResources → `method` (with `dom-loading`/`dom-complete` document-event
/// followups; `dom-loading` triggers a `getTarget` refresh — iter-138 Theme F
/// distrusts the event's own URL unconditionally for these three verbs) →
/// evaluateJSAsync (the forced `location.href` re-resolution at commit,
/// iter-138 Theme F) → unwatchResources → getTarget (refresh_console_actor
/// after commit).
///
/// `back`/`forward` make exactly three `evaluateJSAsync` calls (pre-nav
/// epoch, pre-nav href, and the Theme F commit re-resolution); use
/// [`nav_action_commit_server_reload`] for `reload`, which makes a fourth
/// (its own pre-dispatch `location.href` capture for `needs_href_fallback`).
/// The first two calls' *values* don't matter to any assertion — only the
/// third (`eval_result_location_href_example_com.json`, a real Firefox
/// recording of `window.location.href` on example.com) does, since iter-138
/// Theme F means the CLI now always trusts that eval over the
/// document-event's own `url` field.
fn nav_action_commit_server(method: &str) -> MockRdpServer {
    MockRdpServer::new()
        .on("listTabs", load_fixture("list_tabs_response.json"))
        .on("getTarget", load_fixture("get_target_response.json"))
        .on("getWatcher", load_fixture("get_watcher_response.json"))
        .on_sequence(
            "evaluateJSAsync",
            vec![
                (
                    load_fixture("eval_immediate_response.json"),
                    vec![load_fixture("eval_result_ready_state_complete.json")],
                ),
                (
                    load_fixture("eval_immediate_response.json"),
                    vec![load_fixture("eval_result_ready_state_complete.json")],
                ),
                (
                    load_fixture("eval_immediate_response_location_href.json"),
                    vec![load_fixture("eval_result_location_href_example_com.json")],
                ),
            ],
        )
        .on("watchTargets", load_fixture("watch_targets_response.json"))
        .on(
            "watchResources",
            load_fixture("watch_resources_response.json"),
        )
        .on_with_followups(
            method,
            load_fixture("reload_response.json"),
            vec![
                load_fixture("resources_available_document_event_dom_loading.json"),
                load_fixture("resources_available_document_event_dom_complete.json"),
            ],
        )
        .on(
            "unwatchResources",
            load_fixture("unwatch_resources_response.json"),
        )
}

/// Like [`nav_action_commit_server`] but for `reload`, which makes one extra
/// leading `evaluateJSAsync` call (`location.href`, captured before dispatch
/// as the `requested_url` fed to `needs_href_fallback`) ahead of the three
/// `wait_for_navigation_commit` makes for all three verbs (see
/// [`nav_action_commit_server`]'s doc comment).
fn nav_action_commit_server_reload() -> MockRdpServer {
    MockRdpServer::new()
        .on("listTabs", load_fixture("list_tabs_response.json"))
        .on("getTarget", load_fixture("get_target_response.json"))
        .on("getWatcher", load_fixture("get_watcher_response.json"))
        .on_sequence(
            "evaluateJSAsync",
            vec![
                (
                    load_fixture("eval_immediate_response.json"),
                    vec![load_fixture("eval_result_string.json")],
                ),
                (
                    load_fixture("eval_immediate_response.json"),
                    vec![load_fixture("eval_result_ready_state_complete.json")],
                ),
                (
                    load_fixture("eval_immediate_response.json"),
                    vec![load_fixture("eval_result_ready_state_complete.json")],
                ),
                (
                    load_fixture("eval_immediate_response_location_href.json"),
                    vec![load_fixture("eval_result_location_href_example_com.json")],
                ),
            ],
        )
        .on("watchTargets", load_fixture("watch_targets_response.json"))
        .on(
            "watchResources",
            load_fixture("watch_resources_response.json"),
        )
        .on_with_followups(
            "reload",
            load_fixture("reload_response.json"),
            vec![
                load_fixture("resources_available_document_event_dom_loading.json"),
                load_fixture("resources_available_document_event_dom_complete.json"),
            ],
        )
        .on(
            "unwatchResources",
            load_fixture("unwatch_resources_response.json"),
        )
}

/// `live_130_reload_envelope`'s mock-server sibling: `reload` now returns the
/// navigate-style envelope (`committed_url`, `ready_state`, `elapsed_ms`)
/// instead of a bare `{"action": "reload"}` (iter-130 Theme B).
#[test]
fn reload_outputs_json_envelope() {
    let server = nav_action_commit_server_reload();
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.push("reload".to_owned());

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

    assert_eq!(json["results"]["action"], "reload");
    assert_eq!(json["results"]["committed_url"], "https://example.com/");
    assert_eq!(json["results"]["ready_state"], "complete");
    assert!(
        json["results"]["elapsed_ms"].is_u64(),
        "elapsed_ms must be present: {json}"
    );
}

/// iter-174 regression guard: the direct route's `getWatcher` must carry
/// `isServerTargetSwitchingEnabled: true`.
///
/// Without it Firefox instantiates no watcher-owned target for the top-level
/// window global, so the content-process watcher that emits `dom-loading` /
/// `dom-interactive` / `dom-complete` never runs. Everything still *looks*
/// connected — `watchTargets("frame")` and `watchResources` are acked, and the
/// parent-process resources (`will-navigate`, `network-event`) keep arriving —
/// which is why `reload --no-daemon` spent 21 011 ms of a 30 000 ms budget
/// waiting for an event that could not come, then answered from the
/// `document.readyState` fallback with a correct-looking envelope.
///
/// This asserts the argument rather than the timing, because a mock server
/// cannot reproduce Firefox's target lifecycle: `live_174_*` owns the
/// behavioural half. Together they pin both ends — this one runs in CI without
/// a browser, so the flag cannot be dropped silently again.
#[test]
fn reload_get_watcher_enables_server_target_switching() {
    let server = nav_action_commit_server_reload();
    let port = server.port();
    let requests = server.request_log();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.push("reload".to_owned());

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

    let requests = requests
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let get_watcher: Vec<&serde_json::Value> = requests
        .iter()
        .filter(|r| r.get("type").and_then(|t| t.as_str()) == Some("getWatcher"))
        .collect();
    assert_eq!(
        get_watcher.len(),
        1,
        "reload must issue exactly one getWatcher, got {get_watcher:?}"
    );
    assert_eq!(
        get_watcher[0]["isServerTargetSwitchingEnabled"],
        serde_json::Value::Bool(true),
        "iter-174: without this flag the three dom-* document events never \
         arrive on a direct connection and every navigation verb burns its \
         whole events budget. Packet: {}",
        get_watcher[0]
    );
}

// ---------------------------------------------------------------------------
// reload --wait-idle
// ---------------------------------------------------------------------------

/// Build a mock server that:
/// 1. Responds to listTabs, getTarget, getWatcher, watchResources
/// 2. After watchResources, pushes a network event batch (simulating page reload traffic)
/// 3. Closes the connection after sending followups so the idle loop gets EOF
///    and returns cleanly (simulates the "idle" condition).
fn reload_wait_idle_server(network_events: Vec<serde_json::Value>) -> MockRdpServer {
    MockRdpServer::new()
        .on("listTabs", load_fixture("list_tabs_response.json"))
        .on("getTarget", load_fixture("get_target_response.json"))
        .on("getWatcher", load_fixture("get_watcher_response.json"))
        .on_with_followups(
            "watchResources",
            load_fixture("watch_resources_response.json"),
            network_events,
        )
        // No reload handler needed — the raw reload send gets an "unknownMethod"
        // error back from the mock, which is harmlessly ignored by the idle loop.
        .on(
            "unwatchResources",
            load_fixture("unwatch_resources_response.json"),
        )
        .close_after_followups()
}

#[test]
fn reload_wait_idle_observes_network_events() {
    let network_event = json!({
        "type": "resources-available-array",
        "from": "server1.conn0.watcher4",
        "array": [
            ["network-event", [
                {
                    "resourceType": "network-event",
                    "actor": "server1.conn0.netActor1",
                    "startedDateTime": "2026-01-01T00:00:00.000Z",
                    "url": "https://example.com/style.css",
                    "method": "GET",
                    "isXHR": false,
                    "cause": {"type": "stylesheet"},
                    "fromCache": false,
                    "private": false
                },
                {
                    "resourceType": "network-event",
                    "actor": "server1.conn0.netActor2",
                    "startedDateTime": "2026-01-01T00:00:00.010Z",
                    "url": "https://example.com/app.js",
                    "method": "GET",
                    "isXHR": false,
                    "cause": {"type": "script"},
                    "fromCache": false,
                    "private": false
                }
            ]]
        ]
    });

    let server = reload_wait_idle_server(vec![network_event]);
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend([
        "reload".to_owned(),
        "--wait-idle".to_owned(),
        "--idle-ms".to_owned(),
        "500".to_owned(),
        "--reload-timeout".to_owned(),
        "5000".to_owned(),
    ]);

    let output = std::process::Command::new(ff_rdp_bin())
        .args(&args)
        .output()
        .expect("failed to spawn ff-rdp");

    handle.join().expect("server thread panicked");

    // On some CI runners (macOS ARM64) the mock TCP connection can fail with
    // EINVAL. Skip rather than fail — the Linux CI job covers this reliably.
    let stderr = String::from_utf8_lossy(&output.stderr);
    if !output.status.success() && stderr.contains("Invalid argument") {
        eprintln!("skipping: mock TCP connection failed on this platform");
        return;
    }

    assert!(
        output.status.success(),
        "expected success, {}",
        support::output_note(&output)
    );

    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("stdout must be valid JSON");

    assert_eq!(json["results"]["reloaded"], true);
    // Should have observed 2 network resources from the batch.
    assert_eq!(
        json["results"]["requests_observed"], 2,
        "should count 2 network events from the batch"
    );
    // idle_at_ms is present (may be 0 if connection closed immediately)
    assert!(
        !json["results"]["idle_at_ms"].is_null(),
        "idle_at_ms must be present in output"
    );
}

#[test]
fn reload_wait_idle_no_traffic_returns_idle_quickly() {
    // With no network events the loop exits when the mock server closes the
    // connection (EOF path in the idle-drain loop).  Since `last_event_at` is
    // only set once a non-empty network-event batch arrives, the total timeout
    // would govern on a live server with zero traffic — but in the mock the
    // connection closes after the followup batch is delivered, which triggers
    // the EOF break and returns before any timeout fires.
    // We use a single dummy empty followup batch to trigger the
    // close_after_followups behaviour.
    let empty_batch = json!({
        "type": "resources-available-array",
        "from": "server1.conn0.watcher4",
        "array": [["network-event", []]]
    });

    let server = reload_wait_idle_server(vec![empty_batch]);
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend([
        "reload".to_owned(),
        "--wait-idle".to_owned(),
        "--idle-ms".to_owned(),
        "100".to_owned(),
        "--reload-timeout".to_owned(),
        "5000".to_owned(),
    ]);

    let output = std::process::Command::new(ff_rdp_bin())
        .args(&args)
        .output()
        .expect("failed to spawn ff-rdp");

    handle.join().expect("server thread panicked");

    let stderr = String::from_utf8_lossy(&output.stderr);
    if !output.status.success() && stderr.contains("Invalid argument") {
        eprintln!("skipping: mock TCP connection failed on this platform");
        return;
    }

    assert!(
        output.status.success(),
        "expected success, {}",
        support::output_note(&output)
    );

    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("stdout must be valid JSON");
    assert_eq!(json["results"]["reloaded"], true);
    assert_eq!(json["results"]["requests_observed"], 0);
}

/// `live_130_back_forward_envelope`'s mock-server sibling (back half).
#[test]
fn back_outputs_json_envelope() {
    let server = nav_action_commit_server("goBack");
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.push("back".to_owned());

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
    assert_eq!(json["results"]["action"], "back");
    assert_eq!(json["results"]["committed_url"], "https://example.com/");
    assert_eq!(json["results"]["ready_state"], "complete");
    assert!(
        json["results"]["elapsed_ms"].is_u64(),
        "elapsed_ms must be present: {json}"
    );
}

/// `live_130_back_forward_envelope`'s mock-server sibling (forward half).
#[test]
fn forward_outputs_json_envelope() {
    let server = nav_action_commit_server("goForward");
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.push("forward".to_owned());

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
    assert_eq!(json["results"]["action"], "forward");
    assert_eq!(json["results"]["committed_url"], "https://example.com/");
    assert_eq!(json["results"]["ready_state"], "complete");
    assert!(
        json["results"]["elapsed_ms"].is_u64(),
        "elapsed_ms must be present: {json}"
    );
}

// ---------------------------------------------------------------------------
// iter-138 Theme E — `--no-wait` escape hatch for back/forward/reload
// ---------------------------------------------------------------------------

/// AC: `e2e_no_wait_flag_consistency` — the `--no-wait` flag exists on every
/// command whose commit-wait timeout message recommends it.
///
/// Pre-fix: the Theme B/C timeout error text told the caller to "use
/// --no-wait to skip or increase --timeout", but `back`/`forward`/`reload`
/// didn't accept the flag at all (`error: unexpected argument '--no-wait'
/// found`) — there was no escape hatch whatsoever for the readiness wait on
/// history commands. This asserts the flag now exists on all three (chosen
/// over removing the recommendation, since it's a genuinely useful escape
/// hatch — see the iteration plan's Theme E notes).
#[test]
fn e2e_no_wait_flag_consistency() {
    for (subcommand, needs_positional) in [("back", false), ("forward", false), ("reload", false)] {
        let _ = needs_positional; // none of these three take a positional arg
        let out = std::process::Command::new(ff_rdp_bin())
            .args([subcommand, "--help"])
            .output()
            .unwrap_or_else(|e| panic!("{subcommand} --help: {e}"));
        assert!(
            out.status.success(),
            "{subcommand} --help must exit 0; stderr: {}",
            support::output_note(&out)
        );
        let help = String::from_utf8_lossy(&out.stdout);
        assert!(
            help.contains("--no-wait"),
            "{subcommand} --help must document --no-wait (the flag its own \
             timeout error recommends): {help}"
        );
    }

    // `navigate --help` already had --no-wait before this iteration — assert
    // it still does, so the four navigation verbs stay consistent.
    let nav_help = std::process::Command::new(ff_rdp_bin())
        .args(["navigate", "--help"])
        .output()
        .expect("navigate --help");
    assert!(
        String::from_utf8_lossy(&nav_help.stdout).contains("--no-wait"),
        "navigate --help must still document --no-wait"
    );
}

/// `back --no-wait` dispatches `goBack` and returns immediately without
/// waiting for a commit — the bare pre-iter-130 envelope, not the
/// `{committed_url, ready_state, elapsed_ms}` shape.
#[test]
fn back_no_wait_returns_bare_envelope_immediately() {
    // Only listTabs/getTarget/getWatcher are needed to resolve the target;
    // goBack itself is a raw fire-and-forget send that this mock never has to
    // answer, and no document-event wait is ever started.
    let server = MockRdpServer::new()
        .on("listTabs", load_fixture("list_tabs_response.json"))
        .on("getTarget", load_fixture("get_target_response.json"));
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend(["back".to_owned(), "--no-wait".to_owned()]);

    let output = std::process::Command::new(ff_rdp_bin())
        .args(&args)
        .output()
        .expect("failed to spawn ff-rdp");

    handle.join().unwrap();

    assert!(
        output.status.success(),
        "back --no-wait must succeed without a document-event reply: {}",
        support::output_note(&output)
    );
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["results"]["action"], "back");
    assert!(
        json["results"].get("committed_url").is_none(),
        "back --no-wait must return the bare envelope, not the commit-wait \
         one: {json}"
    );
}

// ---------------------------------------------------------------------------
// iter-169 Theme B — status / status_reason parity across all four verbs
// ---------------------------------------------------------------------------

/// Assert the iter-169 Theme B envelope invariant on a `results` object:
/// both keys are always present, and exactly one of them is non-`null`.
fn assert_status_pair_present(results: &serde_json::Value, label: &str) {
    assert!(
        results.get("status").is_some(),
        "{label}: `status` must always be present, got {results}"
    );
    assert!(
        results.get("status_reason").is_some(),
        "{label}: `status_reason` must always be present, got {results}"
    );
    let has_status = !results["status"].is_null();
    let has_reason = !results["status_reason"].is_null();
    assert!(
        has_status != has_reason,
        "{label}: exactly one of status/status_reason must be non-null, got {results}"
    );
}

/// `back`/`forward`/`reload` emit `status` and `status_reason` on the
/// commit-wait path (iter-169 Theme B). This mock delivers document-events
/// but no `network-event` resources at all, so the honest answer is
/// `no_document_request` — the point of the test is that both keys exist and
/// carry a *reason*, where before this iteration neither key was emitted and
/// `--jq '.results.status'` returned a bare `null`.
#[test]
fn nav_verbs_emit_status_and_reason_on_commit_path() {
    for (verb, method) in [("back", "goBack"), ("forward", "goForward")] {
        let server = nav_action_commit_server(method);
        let port = server.port();
        let handle = std::thread::spawn(move || server.serve_one());

        let mut args = base_args(port);
        args.push(verb.to_owned());

        let output = std::process::Command::new(ff_rdp_bin())
            .args(&args)
            .output()
            .expect("failed to spawn ff-rdp");
        handle.join().unwrap();

        assert!(
            output.status.success(),
            "{verb}: expected success, stderr: {}",
            support::output_note(&output)
        );
        let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_status_pair_present(&json["results"], verb);
    }

    // `reload` uses the four-eval mock (it captures its own pre-dispatch
    // `location.href`), so it needs the dedicated server builder.
    let server = nav_action_commit_server_reload();
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.push("reload".to_owned());

    let output = std::process::Command::new(ff_rdp_bin())
        .args(&args)
        .output()
        .expect("failed to spawn ff-rdp");
    handle.join().unwrap();

    assert!(
        output.status.success(),
        "reload: expected success, stderr: {}",
        support::output_note(&output)
    );
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_status_pair_present(&json["results"], "reload");
}

/// `--no-wait` returns before any resource can arrive, so it cannot have
/// observed a status — but it must still say so with `not_observed` rather
/// than omitting the keys (iter-169 Theme B: "on every path").
#[test]
fn nav_verbs_no_wait_report_not_observed() {
    let server = MockRdpServer::new()
        .on("listTabs", load_fixture("list_tabs_response.json"))
        .on("getTarget", load_fixture("get_target_response.json"));
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend(["back".to_owned(), "--no-wait".to_owned()]);

    let output = std::process::Command::new(ff_rdp_bin())
        .args(&args)
        .output()
        .expect("failed to spawn ff-rdp");
    handle.join().unwrap();

    assert!(
        output.status.success(),
        "back --no-wait must succeed: {}",
        support::output_note(&output)
    );
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["results"]["status"], serde_json::Value::Null);
    assert_eq!(
        json["results"]["status_reason"], "not_observed",
        "back --no-wait must name the reason it saw no status: {json}"
    );
}
