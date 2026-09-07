//! Live tests for iteration 252 — `console --follow` delivers `console-message`
//! resources on **both** routes.
//!
//! Iteration 252 audited every direct-route `getWatcher` call site that still
//! omitted `isServerTargetSwitchingEnabled` after iteration 174 fixed the two
//! navigation waits, and asked one question: does any of them subscribe to a
//! resource only the content process emits? Exactly one does —
//! `console.rs::run_follow_direct`, which watches `console-message` and
//! `error-message`. Both live in `FrameTargetResources`
//! (`devtools/server/actors/resources/index.js:62-125`); every other remaining
//! site watches `network-event` or `cookies`, which are `ParentProcessResources`
//! (same file, `:212-235`) and structurally cannot be starved this way.
//!
//! Measuring it turned up **two** defects stacked on top of each other, which
//! is why iteration 174's attempt at the same comparison saw empty stdout on
//! both routes and could conclude nothing:
//!
//! 1. **The direct route received nothing at all** — the iteration-174 shape in
//!    a second place. `watchResources` was acked and then the connection went
//!    silent for the whole window while the page logged once per second. Two
//!    server-side preconditions were unmet: without
//!    `isServerTargetSwitchingEnabled` the top-level browsing context is
//!    rejected by `shouldNotifyWindowGlobal`
//!    (`watcher/browsing-context-helpers.sys.mjs:174-182`), and without a
//!    preceding `watchTargets("frame")` the content-process half of
//!    `watchResources` has no target actors to fan the new resource types out
//!    to (`js-process-actor/DevToolsProcessChild.sys.mjs:409-414`).
//! 2. **Neither route printed anything even when the frames arrived** —
//!    `parse_single_console_resource` understood only the nested
//!    `{"message": {…}}` shape of the legacy `consoleAPICall` push. A
//!    `console-message` *resource* is flat: `resources/console-messages.js:55`
//!    passes `prepareConsoleMessageForRemote`'s result straight to
//!    `onAvailable`.
//!
//! Isolated on Firefox 155.0.1, over an 8 s window against a page logging at
//! 1 Hz:
//!
//! ```text
//! neither fix       daemon 0 lines   direct 0 lines
//! parser fix only   daemon 7 lines   direct 0 lines   ← defect 1, alone
//! both fixes        daemon 8 lines   direct 8 lines
//! ```
//!
//! daemon-parity: the assertion runs on both routes, because the audit's whole
//! question was whether they diverge.
//!
//! # Running
//!
//!   FF_RDP_LIVE_TESTS=1 cargo test-live -p ff-rdp-cli --test live live_252 -- --nocapture

use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::common::{FixtureRoute, FixtureServer, LiveFirefox, ff_rdp_bin, live_tests_enabled};

/// The token the fixture page logs. Distinctive enough that `--pattern` cannot
/// match anything Firefox itself emits.
const PROBE: &str = "iter252-console-follow-probe";

/// How long to wait for the first matching line before calling the route
/// starved. The page logs every 250 ms, so a healthy route answers in well
/// under a second; 20 s leaves an enormous margin for a loaded CI box while
/// still failing in bounded time on the defect (which never answers at all).
const OBSERVE_BUDGET: Duration = Duration::from_secs(20);

/// The fixture page. The ticker is **page-driven** on purpose: a probe emitted
/// by a second `ff-rdp eval` would need the daemon's single RPC-writer slot,
/// which the daemon-route `console --follow` under test is already holding.
/// A page that logs on its own removes the CLI from the emitting side entirely,
/// so both routes are measured against an identical, independent source.
fn fixture_routes() -> HashMap<String, FixtureRoute> {
    let mut routes = HashMap::new();
    routes.insert(
        "/tick".to_owned(),
        FixtureRoute::html(format!(
            "<!doctype html><title>iter-252 console ticker</title>\
             <body>iter-252</body>\
             <script>setInterval(function(){{console.log('{PROBE}');}}, 250);</script>"
        )),
    );
    routes
}

fn base_args(port: u16) -> Vec<String> {
    vec![
        "--host".to_owned(),
        "127.0.0.1".to_owned(),
        "--port".to_owned(),
        port.to_string(),
    ]
}

fn direct_args(port: u16) -> Vec<String> {
    let mut args = base_args(port);
    args.push("--no-daemon".to_owned());
    args
}

fn stop_daemon(port: u16) {
    let _ = Command::new(ff_rdp_bin())
        .args(base_args(port))
        .args(["daemon", "stop"])
        .output();
}

/// Push every stdout line of `child` into a shared buffer from a reader thread,
/// so the test can poll without blocking on a stream that may never produce a
/// byte — which is precisely the failure being tested for.
fn collect_stdout_lines(child: &mut Child) -> Arc<Mutex<Vec<String>>> {
    let lines: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let stdout = child.stdout.take().expect("follow child stdout piped");
    let sink = Arc::clone(&lines);
    std::thread::spawn(move || {
        let reader = BufReader::new(stdout);
        for line in reader.lines() {
            match line {
                Ok(l) => sink.lock().expect("stdout sink lock").push(l),
                Err(_) => break,
            }
        }
    });
    lines
}

/// Run `console --follow` over one route and return the matching NDJSON lines
/// seen within [`OBSERVE_BUDGET`].
fn follow_console(global: &[String], label: &str) -> Vec<String> {
    let mut child = Command::new(ff_rdp_bin())
        .args(global)
        .args(["console", "--follow", "--pattern", PROBE])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap_or_else(|e| panic!("{label}: spawn `console --follow`: {e}"));

    let lines = collect_stdout_lines(&mut child);

    let deadline = Instant::now() + OBSERVE_BUDGET;
    let mut matched: Vec<String> = Vec::new();
    while Instant::now() < deadline {
        // A follow that exits on its own is a failure worth reporting as
        // itself rather than as a timeout.
        if let Ok(Some(status)) = child.try_wait() {
            let guard = lines.lock().expect("stdout lines lock");
            panic!(
                "{label}: `console --follow` exited early with {status} — it must \
                 stream until killed. Lines seen: {:?}",
                *guard
            );
        }
        {
            let guard = lines.lock().expect("stdout lines lock");
            matched = guard
                .iter()
                .filter(|line| line.contains(PROBE))
                .cloned()
                .collect();
        }
        if !matched.is_empty() {
            break;
        }
        std::thread::sleep(Duration::from_millis(200));
    }

    let _ = child.kill();
    let _ = child.wait();
    matched
}

/// Assert one route delivered the page's console output, with the failure text
/// naming the two mechanisms that can produce silence here.
fn assert_route_sees_console(global: &[String], route: &str) {
    let label = format!("{route}: console --follow");
    let matched = follow_console(global, &label);

    assert!(
        !matched.is_empty(),
        "{route}: `console --follow` saw no `{PROBE}` line in {OBSERVE_BUDGET:?} while the page \
         logged one every 250 ms. Either the `console-message` resources never \
         arrived (the iteration-174 shape: a watcher obtained without \
         `isServerTargetSwitchingEnabled`, or `watchResources` issued with no \
         preceding `watchTargets(\"frame\")`, so no content-process frame target \
         exists to emit them), or they arrived and were dropped by \
         `parse_single_console_resource` (which must accept the flat resource \
         payload, not only the nested `consoleAPICall` `message` wrapper)."
    );

    // The line has to be a usable console record, not merely a substring match:
    // a resource that parsed into an empty `message` would still contain the
    // probe token via some other field.
    let parsed: serde_json::Value = serde_json::from_str(&matched[0])
        .unwrap_or_else(|e| panic!("{route}: follow line is not JSON: {e}\n{}", matched[0]));
    assert_eq!(
        parsed["message"], PROBE,
        "{route}: `message` must carry the logged text, got {parsed}"
    );
    assert_eq!(
        parsed["level"], "log",
        "{route}: `level` must survive parsing, got {parsed}"
    );
}

/// AC (iteration 252): every content-process subscriber is measured on both
/// routes. `console --follow` is the only one the audit found, and it must
/// deliver on the direct route as well as through the daemon.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_252_console_follow_sees_content_process_messages_both_routes() {
    if !live_tests_enabled() {
        eprintln!(
            "live_252_console_follow_sees_content_process_messages_both_routes: \
             set FF_RDP_LIVE_TESTS=1 to run"
        );
        return;
    }

    let Some(server) = FixtureServer::start(fixture_routes()) else {
        panic!("live_252: could not bind the local fixture server");
    };
    let url = format!("{}/tick", server.base_url());

    let ff = LiveFirefox::headless_on_random_port();
    let port = ff.port();

    // Drive the page directly, so the daemon's single RPC-writer slot stays
    // free for the daemon leg's own follow stream.
    let nav = Command::new(ff_rdp_bin())
        .args(direct_args(port))
        .args(["navigate", &url])
        .output()
        .unwrap_or_else(|e| panic!("live_252: spawn navigate {url}: {e}"));
    assert!(
        nav.status.success(),
        "live_252: navigate to the ticker page must exit 0, got: {}{}",
        String::from_utf8_lossy(&nav.stdout),
        String::from_utf8_lossy(&nav.stderr),
    );

    // --- direct route ----------------------------------------------------
    // The leg that reproduces the defect. Run first, before any daemon exists,
    // so nothing about the daemon's watcher can mask it.
    assert_route_sees_console(&direct_args(port), "direct");

    // --- daemon route ----------------------------------------------------
    // The control. If this leg fails the harness is not measuring anything and
    // the direct result above is not evidence either way — the exact trap
    // iteration 174 fell into.
    assert!(
        ff.with_daemon().is_some(),
        "live_252: the proxy daemon did not start for Firefox on port {port}"
    );
    assert_route_sees_console(&base_args(port), "daemon");

    stop_daemon(port);
}
