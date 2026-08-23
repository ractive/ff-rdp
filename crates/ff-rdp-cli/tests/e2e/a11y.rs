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

/// Build a mock server for a11y summary (uses JS eval path).
fn a11y_summary_server() -> MockRdpServer {
    MockRdpServer::new()
        .on("listTabs", load_fixture("list_tabs_response.json"))
        .on("getTarget", load_fixture("get_target_response.json"))
        .on_with_followup(
            "evaluateJSAsync",
            load_fixture("eval_immediate_response.json"),
            load_fixture("eval_result_a11y_summary.json"),
        )
}

/// Build a mock server that handles the full a11y protocol sequence.
///
/// Protocol flow (Firefox 153, iter-136):
///   listTabs → getTarget → bootstrap → getWalker
///   → children (no args, on the walker: yields the root document)
///   → children (on each accessible actor with childCount > 0)
///
/// The `children` replies are played back in order: the walker's root reply
/// first, the root document's children second, then an empty reply that
/// repeats so the traversal terminates.
fn a11y_server() -> MockRdpServer {
    MockRdpServer::new()
        .on("listTabs", load_fixture("list_tabs_response.json"))
        .on("getTarget", load_fixture("get_target_response.json"))
        .on("bootstrap", load_fixture("a11y_bootstrap_response.json"))
        .on("getWalker", load_fixture("a11y_get_walker_response.json"))
        .on_sequence(
            "children",
            vec![
                (load_fixture("a11y_walker_children_response.json"), vec![]),
                (load_fixture("a11y_children_response.json"), vec![]),
                (load_fixture("a11y_children_empty_response.json"), vec![]),
            ],
        )
}

/// Build a mock server for a Firefox build whose walker rejects the
/// argument-less `children` root accessor, exercising the legacy
/// `getDocument`/`getRootNode` fallback in `AccessibilityActor::get_root`.
fn a11y_legacy_root_server() -> MockRdpServer {
    MockRdpServer::new()
        .on("listTabs", load_fixture("list_tabs_response.json"))
        .on("getTarget", load_fixture("get_target_response.json"))
        .on("bootstrap", load_fixture("a11y_bootstrap_response.json"))
        .on("getWalker", load_fixture("a11y_get_walker_response.json"))
        .on_sequence(
            "children",
            vec![
                (
                    serde_json::json!({
                        "from": "server1.conn0.child2/accessibleWalkerActor13",
                        "error": "unrecognizedPacketType",
                        "message": "Actor accessibleWalkerActor13 does not recognize the packet type 'children'"
                    }),
                    vec![],
                ),
                (load_fixture("a11y_children_response.json"), vec![]),
                (load_fixture("a11y_children_empty_response.json"), vec![]),
            ],
        )
        .on("getDocument", load_fixture("a11y_get_root_response.json"))
}

/// Build a mock server for a `bootstrap`-disabled Firefox: `a11y` must take
/// the JS-eval fallback path (iter-143 Theme A).
fn a11y_disabled_service_server() -> MockRdpServer {
    MockRdpServer::new()
        .on("listTabs", load_fixture("list_tabs_response.json"))
        .on("getTarget", load_fixture("get_target_response.json"))
        .on(
            "bootstrap",
            serde_json::json!({
                "from": "server1.conn0.child2/accessibilityActor12",
                "state": {"enabled": false}
            }),
        )
        .on_with_followup(
            "evaluateJSAsync",
            load_fixture("eval_immediate_response.json"),
            serde_json::json!({
                "from": "server1.conn0.child2/consoleActor3",
                "type": "evaluationResult",
                "resultID": "1775437183977.373-0",
                "hasException": false,
                "result": "__FF_RDP_JSON__{\"role\":\"document\",\"children\":[{\"role\":\"generic\",\"name\":\"body\"}]}",
                "timestamp": 1_775_437_183_980.721
            }),
        )
}

/// Build a mock server for `a11y --native` where the service is already
/// enabled: same protocol flow as [`a11y_server`] but with `getRoot`
/// exposing `parentAccessibilityActor` (needed even when the service is
/// already on, since `run_native_opt_in` always locates it first).
fn a11y_native_already_enabled_server() -> MockRdpServer {
    MockRdpServer::new()
        .on("listTabs", load_fixture("list_tabs_response.json"))
        .on("getTarget", load_fixture("get_target_response.json"))
        .on(
            "getRoot",
            serde_json::json!({
                "from": "root",
                "parentAccessibilityActor": "server1.conn0.parentAccessibilityActor6"
            }),
        )
        .on("bootstrap", load_fixture("a11y_bootstrap_response.json"))
        .on("getWalker", load_fixture("a11y_get_walker_response.json"))
        .on_sequence(
            "children",
            vec![
                (load_fixture("a11y_walker_children_response.json"), vec![]),
                (load_fixture("a11y_children_response.json"), vec![]),
                (load_fixture("a11y_children_empty_response.json"), vec![]),
            ],
        )
}

/// Build a mock server for `a11y --native` where the service starts
/// *disabled*: `run_native_opt_in` must call `enable` on
/// `parentAccessibilityActor`, see `bootstrap` flip to enabled, walk the
/// tree, then call `disable` to restore the prior state.
fn a11y_native_opt_in_server() -> MockRdpServer {
    MockRdpServer::new()
        .on("listTabs", load_fixture("list_tabs_response.json"))
        .on("getTarget", load_fixture("get_target_response.json"))
        .on(
            "getRoot",
            serde_json::json!({
                "from": "root",
                "parentAccessibilityActor": "server1.conn0.parentAccessibilityActor6"
            }),
        )
        .on_sequence(
            "bootstrap",
            vec![
                (
                    serde_json::json!({
                        "from": "server1.conn0.child2/accessibilityActor12",
                        "state": {"enabled": false}
                    }),
                    vec![],
                ),
                (
                    serde_json::json!({
                        "from": "server1.conn0.child2/accessibilityActor12",
                        "state": {"enabled": true}
                    }),
                    vec![],
                ),
            ],
        )
        .on(
            "enable",
            serde_json::json!({"from": "server1.conn0.parentAccessibilityActor6"}),
        )
        .on(
            "disable",
            serde_json::json!({"from": "server1.conn0.parentAccessibilityActor6"}),
        )
        .on("getWalker", load_fixture("a11y_get_walker_response.json"))
        .on_sequence(
            "children",
            vec![
                (load_fixture("a11y_walker_children_response.json"), vec![]),
                (load_fixture("a11y_children_response.json"), vec![]),
                (load_fixture("a11y_children_empty_response.json"), vec![]),
            ],
        )
}

/// Build a mock server where `enable` succeeds but `bootstrap` keeps
/// reporting the service disabled afterward — the "enable didn't take"
/// failure branch, which must surface as an explicit error, never a silent
/// fallback.
fn a11y_native_enable_does_not_take_server() -> MockRdpServer {
    MockRdpServer::new()
        .on("listTabs", load_fixture("list_tabs_response.json"))
        .on("getTarget", load_fixture("get_target_response.json"))
        .on(
            "getRoot",
            serde_json::json!({
                "from": "root",
                "parentAccessibilityActor": "server1.conn0.parentAccessibilityActor6"
            }),
        )
        .on(
            "bootstrap",
            serde_json::json!({
                "from": "server1.conn0.child2/accessibilityActor12",
                "state": {"enabled": false}
            }),
        )
        .on(
            "enable",
            serde_json::json!({"from": "server1.conn0.parentAccessibilityActor6"}),
        )
}

/// Build a mock server for a11y contrast (uses JS eval path like snapshot).
fn a11y_contrast_server() -> MockRdpServer {
    MockRdpServer::new()
        .on("listTabs", load_fixture("list_tabs_response.json"))
        .on("getTarget", load_fixture("get_target_response.json"))
        .on_with_followup(
            "evaluateJSAsync",
            load_fixture("eval_immediate_response.json"),
            load_fixture("eval_result_contrast.json"),
        )
}

// ---------------------------------------------------------------------------
// a11y: basic output
// ---------------------------------------------------------------------------

#[test]
fn a11y_outputs_json_with_accessibility_tree() {
    let server = a11y_server();
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.push("a11y".to_owned());

    let output = std::process::Command::new(ff_rdp_bin())
        .args(&args)
        .output()
        .expect("failed to spawn ff-rdp");

    handle.join().unwrap();

    assert!(
        output.status.success(),
        "expected success, stderr: {} stdout: {}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );

    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("stdout must be valid JSON");

    // Root node should be the document role.
    assert_eq!(
        json["results"]["role"], "document",
        "root role should be document"
    );
    assert_eq!(json["total"], 1);

    // Should have children populated from the children fixture.
    let children = json["results"]["children"]
        .as_array()
        .expect("results should have a children array");
    assert!(!children.is_empty(), "tree should have at least one child");

    // Actor IDs must be stripped from output.
    assert!(
        json["results"].get("actor").is_none(),
        "actor IDs should be stripped from output"
    );
}

/// iter-136: current Firefox exposes the root document only through the
/// walker's argument-less `children`. Older builds answered that with
/// `unrecognizedPacketType`, and `AccessibilityActor::get_root` must still fall
/// back to `getDocument` for them.
#[test]
fn a11y_falls_back_to_get_document_when_walker_children_unrecognized() {
    let server = a11y_legacy_root_server();
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.push("a11y".to_owned());

    let output = std::process::Command::new(ff_rdp_bin())
        .args(&args)
        .output()
        .expect("failed to spawn ff-rdp");

    handle.join().unwrap();

    assert!(
        output.status.success(),
        "expected success, stderr: {} stdout: {}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );

    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("stdout must be valid JSON");

    assert_eq!(
        json["results"]["role"], "document",
        "legacy getDocument fallback must still yield the document root"
    );
    assert!(
        json["results"]["children"]
            .as_array()
            .is_some_and(|c| !c.is_empty()),
        "legacy fallback must still walk children: {json}"
    );
}

// ---------------------------------------------------------------------------
// a11y: interactive filter
// ---------------------------------------------------------------------------

#[test]
fn a11y_interactive_filters_to_interactive_elements() {
    let server = a11y_server();
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend(["a11y".to_owned(), "--interactive".to_owned()]);

    let output = std::process::Command::new(ff_rdp_bin())
        .args(&args)
        .output()
        .expect("failed to spawn ff-rdp");

    handle.join().unwrap();

    assert!(
        output.status.success(),
        "expected success, stderr: {} stdout: {}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );

    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("stdout must be valid JSON");

    // The fixture children include a "link" node which is interactive.
    // The root document is kept because it has an interactive descendant.
    let results = &json["results"];
    assert_eq!(json["total"], 1);

    // Interactive filter should retain only interactive roles in the subtree.
    // The link child must be present; the non-interactive heading must be absent.
    let children = results["children"]
        .as_array()
        .expect("filtered results should still have children");

    let link_present = children.iter().any(|c| c["role"] == "link");
    assert!(link_present, "interactive filter should retain link role");

    let heading_present = children.iter().any(|c| c["role"] == "heading");
    assert!(
        !heading_present,
        "interactive filter should remove heading role"
    );
}

// ---------------------------------------------------------------------------
// a11y: --jq filter
// ---------------------------------------------------------------------------

#[test]
fn a11y_with_jq_extracts_role() {
    let server = a11y_server();
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend([
        "--jq".to_owned(),
        ".results.role".to_owned(),
        "a11y".to_owned(),
    ]);

    let output = std::process::Command::new(ff_rdp_bin())
        .args(&args)
        .output()
        .expect("failed to spawn ff-rdp");

    handle.join().unwrap();

    assert!(
        output.status.success(),
        "expected success, stderr: {} stdout: {}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(stdout.trim(), "\"document\"");
}

// ---------------------------------------------------------------------------
// a11y: meta.source (iter-143 Theme A)
// ---------------------------------------------------------------------------

#[test]
fn a11y_reports_native_source_when_walker_succeeds() {
    let server = a11y_server();
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.push("a11y".to_owned());

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
    assert_eq!(
        json["meta"]["source"], "native",
        "a successful walker traversal must report meta.source = native: {json}"
    );
    assert!(
        json["meta"].get("source_reason").is_none(),
        "the native path must not carry a fallback reason: {json}"
    );
    assert!(
        json["meta"].get("fallback").is_none(),
        "the native path must not set the legacy fallback flag: {json}"
    );
}

#[test]
fn a11y_reports_js_fallback_source_when_service_disabled() {
    let server = a11y_disabled_service_server();
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.push("a11y".to_owned());

    let output = std::process::Command::new(ff_rdp_bin())
        .args(&args)
        .output()
        .expect("failed to spawn ff-rdp");

    handle.join().unwrap();
    assert!(
        output.status.success(),
        "expected success, stderr: {} stdout: {}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );

    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("stdout must be valid JSON");
    assert_eq!(
        json["meta"]["source"], "js-fallback",
        "a disabled accessibility service must report meta.source = js-fallback: {json}"
    );
    assert_eq!(
        json["meta"]["source_reason"], "accessibility-service-disabled",
        "the fallback reason must name why: {json}"
    );
    assert_eq!(
        json["meta"]["fallback"], true,
        "the legacy fallback flag is kept for existing consumers: {json}"
    );
}

#[test]
fn a11y_selector_mode_reports_js_fallback_without_legacy_fallback_flag() {
    // `--selector` always runs the JS-eval selector path directly — it never
    // touches `bootstrap`/the walker — so any mock exposing a role-shaped
    // `evaluateJSAsync` result works; reuse the disabled-service server's.
    let server = a11y_disabled_service_server();
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend([
        "a11y".to_owned(),
        "--selector".to_owned(),
        "main".to_owned(),
    ]);

    let output = std::process::Command::new(ff_rdp_bin())
        .args(&args)
        .output()
        .expect("failed to spawn ff-rdp");

    handle.join().unwrap();
    assert!(
        output.status.success(),
        "expected success, stderr: {} stdout: {}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );

    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("stdout must be valid JSON");
    assert_eq!(
        json["meta"]["source"], "js-fallback",
        "--selector is always JS-derived: {json}"
    );
    assert_eq!(json["meta"]["source_reason"], "selector-mode");
    assert!(
        json["meta"].get("fallback").is_none(),
        "--selector is a deliberate JS-only mode, not an automatic fallback \
         from a failed native attempt, so the legacy fallback flag must be \
         absent: {json}"
    );
}

// ---------------------------------------------------------------------------
// a11y --native (iter-143 Theme B)
// ---------------------------------------------------------------------------

#[test]
fn a11y_native_walks_platform_tree_when_service_already_enabled() {
    let server = a11y_native_already_enabled_server();
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend(["a11y".to_owned(), "--native".to_owned()]);

    let output = std::process::Command::new(ff_rdp_bin())
        .args(&args)
        .output()
        .expect("failed to spawn ff-rdp");

    handle.join().unwrap();
    assert!(
        output.status.success(),
        "expected success, stderr: {} stdout: {}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );

    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("stdout must be valid JSON");
    assert_eq!(json["meta"]["source"], "native");
    assert_eq!(json["results"]["role"], "document");
}

#[test]
fn a11y_native_enables_walks_and_restores_service() {
    let mut server = a11y_native_opt_in_server();
    let enable_calls = server.call_counter("enable");
    let disable_calls = server.call_counter("disable");
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend(["a11y".to_owned(), "--native".to_owned()]);

    let output = std::process::Command::new(ff_rdp_bin())
        .args(&args)
        .output()
        .expect("failed to spawn ff-rdp");

    handle.join().unwrap();
    assert!(
        output.status.success(),
        "expected success, stderr: {} stdout: {}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );

    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("stdout must be valid JSON");
    assert_eq!(json["meta"]["source"], "native");
    assert_eq!(json["results"]["role"], "document");

    assert_eq!(
        enable_calls.load(std::sync::atomic::Ordering::SeqCst),
        1,
        "the service must be enabled exactly once when it started off"
    );
    assert_eq!(
        disable_calls.load(std::sync::atomic::Ordering::SeqCst),
        1,
        "the service must be restored to disabled exactly once, since this \
         run was the one that turned it on"
    );
}

#[test]
fn a11y_native_conflicts_with_selector_at_cli_level() {
    // No mock server needed: clap must reject this combination before any
    // connection is attempted.
    let mut args = base_args(6000);
    args.extend([
        "a11y".to_owned(),
        "--native".to_owned(),
        "--selector".to_owned(),
        "main".to_owned(),
    ]);

    let output = std::process::Command::new(ff_rdp_bin())
        .args(&args)
        .output()
        .expect("failed to spawn ff-rdp");

    assert!(
        !output.status.success(),
        "--native and --selector must be rejected together"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("native") && stderr.contains("selector"),
        "clap's conflict error should name both flags: {stderr} ({})",
        support::output_note(&output)
    );
}

/// "unit/e2e: enable failure surfaces as an explicit error or an annotated
/// fallback, never a silent one" AC — the "enable() succeeded but bootstrap()
/// still reports disabled" branch.
#[test]
fn a11y_native_errors_explicitly_when_enable_does_not_take_effect() {
    let mut server = a11y_native_enable_does_not_take_server();
    let walker_calls = server.call_counter("getWalker");
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend(["a11y".to_owned(), "--native".to_owned()]);

    let output = std::process::Command::new(ff_rdp_bin())
        .args(&args)
        .output()
        .expect("failed to spawn ff-rdp");

    handle.join().unwrap();
    assert!(
        !output.status.success(),
        "must fail explicitly rather than silently substituting the JS tree"
    );
    // Per the JSON-only output convention, errors are emitted as a JSON
    // envelope on stdout, not a human line on stderr.
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("bootstrap") && stdout.contains("disabled"),
        "the error must explain that bootstrap still reports disabled: {stdout}"
    );
    assert_eq!(
        walker_calls.load(std::sync::atomic::Ordering::SeqCst),
        0,
        "must not attempt to walk the tree at all once enable is known to \
         not have taken effect"
    );
}

// ---------------------------------------------------------------------------
// a11y contrast: basic output
// ---------------------------------------------------------------------------

#[test]
fn a11y_contrast_outputs_json_with_checks() {
    let server = a11y_contrast_server();
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend(["a11y".to_owned(), "contrast".to_owned()]);

    let output = std::process::Command::new(ff_rdp_bin())
        .args(&args)
        .output()
        .expect("failed to spawn ff-rdp");

    handle.join().unwrap();

    assert!(
        output.status.success(),
        "expected success, stderr: {} stdout: {}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );

    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("stdout must be valid JSON");

    // Results should be an array of contrast check objects.
    let results = json["results"]
        .as_array()
        .expect("contrast results should be an array");

    assert!(
        !results.is_empty(),
        "should have at least one contrast check"
    );

    // Each check should have the expected WCAG fields.
    let first = &results[0];
    assert!(
        first.get("ratio").is_some(),
        "check must have a ratio field"
    );
    assert!(
        first.get("foreground").is_some(),
        "check must have foreground"
    );
    assert!(
        first.get("background").is_some(),
        "check must have background"
    );
    assert!(
        first.get("aa_normal").is_some(),
        "check must have aa_normal"
    );

    // Meta should include summary.
    assert!(
        json["meta"]["summary"].is_object(),
        "meta should contain summary"
    );
    assert!(
        json["meta"]["summary"]["total"].is_number(),
        "summary should have total"
    );

    // iter-143 Theme A: contrast checking is always DOM/computed-style
    // based, so meta.source is always js-fallback.
    assert_eq!(json["meta"]["source"], "js-fallback");
    assert_eq!(json["meta"]["source_reason"], "contrast-audit-js-only");
}

// ---------------------------------------------------------------------------
// a11y contrast: --fail-only flag
// ---------------------------------------------------------------------------

#[test]
fn a11y_contrast_fail_only_filters_passing_checks() {
    let server = a11y_contrast_server();
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend([
        "a11y".to_owned(),
        "contrast".to_owned(),
        "--fail-only".to_owned(),
    ]);

    let output = std::process::Command::new(ff_rdp_bin())
        .args(&args)
        .output()
        .expect("failed to spawn ff-rdp");

    handle.join().unwrap();

    assert!(
        output.status.success(),
        "expected success, stderr: {} stdout: {}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );

    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("stdout must be valid JSON");

    // The fixture has all checks passing (aa_normal: true), so --fail-only should return empty.
    let results = json["results"]
        .as_array()
        .expect("contrast results should be an array");
    assert!(
        results.is_empty(),
        "all fixture checks pass AA — fail-only should return empty array"
    );

    // iter-127: `total` counts what the command returns (the failures), so with
    // zero failing checks it must be 0 — NOT the sampled element count.
    assert_eq!(
        json["total"], 0,
        "total must count returned failures (0), not the sampled element count"
    );

    // The sampled element count moves to its own `sampled` field so the
    // "how many elements were checked" signal is preserved.
    assert_eq!(
        json["sampled"], 2,
        "sampled reports the number of examined elements (2 in this fixture)"
    );
}

// ---------------------------------------------------------------------------
// a11y contrast: total == sampled without --fail-only (iter-127 shape parity)
// ---------------------------------------------------------------------------

#[test]
fn a11y_contrast_without_fail_only_total_equals_sampled() {
    let server = a11y_contrast_server();
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend(["a11y".to_owned(), "contrast".to_owned()]);

    let output = std::process::Command::new(ff_rdp_bin())
        .args(&args)
        .output()
        .expect("failed to spawn ff-rdp");

    handle.join().unwrap();

    assert!(
        output.status.success(),
        "expected success, stderr: {} stdout: {}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );

    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("stdout must be valid JSON");

    // Without --fail-only every check is returned, so `total` (returned count)
    // equals `sampled` (examined count) — the fixture has 2 checks. Both keys
    // are always present so the envelope shape is stable across flag combos.
    assert_eq!(json["total"], 2, "total counts all 2 fixture checks");
    assert_eq!(
        json["sampled"], 2,
        "sampled reports all 2 examined elements"
    );
    assert_eq!(
        json["total"], json["sampled"],
        "total must equal sampled when not filtering"
    );
}

// ---------------------------------------------------------------------------
// a11y contrast: --jq filter
// ---------------------------------------------------------------------------

#[test]
fn a11y_contrast_with_jq_extracts_total() {
    let server = a11y_contrast_server();
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend([
        "--jq".to_owned(),
        ".meta.summary.total".to_owned(),
        "a11y".to_owned(),
        "contrast".to_owned(),
    ]);

    let output = std::process::Command::new(ff_rdp_bin())
        .args(&args)
        .output()
        .expect("failed to spawn ff-rdp");

    handle.join().unwrap();

    assert!(
        output.status.success(),
        "expected success, stderr: {} stdout: {}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(stdout.trim(), "2", "fixture has 2 contrast checks");
}

// ---------------------------------------------------------------------------
// a11y summary: JSON output
// ---------------------------------------------------------------------------

#[test]
fn a11y_summary_outputs_json_with_sections() {
    let server = a11y_summary_server();
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend(["a11y".to_owned(), "summary".to_owned()]);

    let output = std::process::Command::new(ff_rdp_bin())
        .args(&args)
        .output()
        .expect("failed to spawn ff-rdp");

    handle.join().unwrap();

    assert!(
        output.status.success(),
        "expected success, stderr: {} stdout: {}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );

    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("stdout must be valid JSON");

    // Results should have the three summary sections.
    assert!(
        json["results"]["landmarks"].is_array(),
        "results must have landmarks array"
    );
    assert!(
        json["results"]["headings"].is_array(),
        "results must have headings array"
    );
    assert!(
        json["results"]["interactive"].is_array(),
        "results must have interactive array"
    );

    // Fixture has 2 landmarks.
    assert_eq!(
        json["results"]["landmarks"].as_array().unwrap().len(),
        2,
        "fixture has 2 landmarks"
    );

    // Fixture has 2 headings.
    assert_eq!(
        json["results"]["headings"].as_array().unwrap().len(),
        2,
        "fixture has 2 headings"
    );

    // Fixture has 3 interactive elements.
    assert_eq!(
        json["results"]["interactive"].as_array().unwrap().len(),
        3,
        "fixture has 3 interactive elements"
    );
}

// ---------------------------------------------------------------------------
// a11y summary: --format text
// ---------------------------------------------------------------------------

#[test]
fn a11y_summary_text_format_renders_sections() {
    let server = a11y_summary_server();
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend([
        "--format".to_owned(),
        "text".to_owned(),
        "a11y".to_owned(),
        "summary".to_owned(),
    ]);

    let output = std::process::Command::new(ff_rdp_bin())
        .args(&args)
        .output()
        .expect("failed to spawn ff-rdp");

    handle.join().unwrap();

    assert!(
        output.status.success(),
        "expected success, stderr: {} stdout: {}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);

    // Text output should contain section headers and content.
    assert!(
        stdout.contains("LANDMARKS"),
        "should contain LANDMARKS header"
    );
    assert!(
        stdout.contains("HEADINGS"),
        "should contain HEADINGS header"
    );
    assert!(
        stdout.contains("INTERACTIVE"),
        "should contain INTERACTIVE header"
    );

    // Fixture headings: h1 "Example Domain", h2 "More information".
    assert!(
        stdout.contains("h1 Example Domain"),
        "should render h1 heading"
    );

    // Fixture has a link.
    assert!(
        stdout.contains("link") && stdout.contains("More information"),
        "should render the link element"
    );

    // Must not be wrapped in JSON quotes (the old bug).
    assert!(
        !stdout.trim().starts_with('"'),
        "text output must not be JSON-quoted"
    );
}

// ---------------------------------------------------------------------------
// a11y summary: --jq filter
// ---------------------------------------------------------------------------

#[test]
fn a11y_summary_with_jq_extracts_headings() {
    let server = a11y_summary_server();
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend([
        "--jq".to_owned(),
        ".results.headings | length".to_owned(),
        "a11y".to_owned(),
        "summary".to_owned(),
    ]);

    let output = std::process::Command::new(ff_rdp_bin())
        .args(&args)
        .output()
        .expect("failed to spawn ff-rdp");

    handle.join().unwrap();

    assert!(
        output.status.success(),
        "expected success, stderr: {} stdout: {}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(stdout.trim(), "2", "fixture has 2 headings");
}
