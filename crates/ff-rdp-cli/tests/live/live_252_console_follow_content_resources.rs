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
             <script>let tick=0;setInterval(function(){{++tick;\
             console.log('{PROBE}:tick:'+tick);\
             console.log('{PROBE}:literal:'+tick+':%s');\
             console.log('%s','{PROBE}:substituted:'+tick+':%s');\
             console.log('{PROBE}:long:'+tick+':'+ 'x'.repeat(10000));\
             console.log(Symbol('{PROBE}:symbol:'+tick));\
             console.log({{nested:Symbol('{PROBE}:nested:'+tick)}});\
             }}, 250);</script>"
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
        .stderr(Stdio::inherit())
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
        if matched.len() >= 36 {
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
fn assert_route_sees_console(global: &[String], route: &str) -> Vec<String> {
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
    assert!(
        parsed["message"].as_str().unwrap().contains(PROBE),
        "{route}: `message` must carry the logged text, got {parsed}"
    );
    assert_eq!(
        parsed["level"], "log",
        "{route}: `level` must survive parsing, got {parsed}"
    );
    let mut counts = std::collections::BTreeMap::new();
    let mut kinds = std::collections::BTreeSet::new();
    let mut violations = Vec::new();
    for line in &matched {
        let entry: serde_json::Value = serde_json::from_str(line).unwrap();
        let message = entry["message"].as_str().unwrap();
        let text = if message.starts_with('{') {
            let grip: serde_json::Value = serde_json::from_str(message).unwrap();
            assert!(
                grip["actor"].as_str().is_some(),
                "output must retain grip actor"
            );
            let initial = match grip["type"].as_str().unwrap() {
                "longString" => {
                    assert!(grip["length"].as_u64().unwrap() > 10000);
                    grip["initial"].as_str().unwrap()
                }
                "symbol" => grip["name"].as_str().unwrap(),
                "object" => {
                    let nested = &grip["preview"]["ownProperties"]["nested"]["value"];
                    assert_eq!(nested["type"], "symbol");
                    assert!(nested["actor"].as_str().is_some());
                    nested["name"].as_str().unwrap()
                }
                _ => panic!("{route}: unexpected grip: {grip}"),
            };
            let prefix = initial.split(':').take(3).collect::<Vec<_>>().join(":");
            eprintln!(
                "iter252 grip route={route} prefix={prefix} actor={} length={}",
                grip["actor"], grip["length"]
            );
            prefix
        } else {
            message.to_owned()
        };
        assert!(
            text.starts_with(PROBE),
            "{route}: logged text must survive: {text}"
        );
        let kind = text.split(':').nth(1).unwrap();
        kinds.insert(kind.to_owned());
        if matches!(kind, "literal" | "substituted") && !text.ends_with(":%s") {
            violations.push(format!("{route}: corrupted percent token: {text}"));
        }
        *counts.entry(text).or_insert(0) += 1;
    }
    eprintln!(
        "iter252 measurement route={route} lines={} unique={} counts={counts:?}",
        matched.len(),
        counts.len()
    );
    assert_eq!(
        kinds.len(),
        6,
        "{route}: every probe kind must arrive: {kinds:?}"
    );
    if !counts.values().all(|count| *count == 1) {
        violations.push(format!("{route}: duplicate timer logs: {counts:?}"));
    }
    violations
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
    let mut violations = assert_route_sees_console(&direct_args(port), "direct");

    // --- daemon route ----------------------------------------------------
    // The control. If this leg fails the harness is not measuring anything and
    // the direct result above is not evidence either way — the exact trap
    // iteration 174 fell into.
    assert!(
        ff.with_daemon().is_some(),
        "live_252: the proxy daemon did not start for Firefox on port {port}"
    );
    violations.extend(assert_route_sees_console(&base_args(port), "daemon"));

    // Plain console arms startListeners on the daemon's shared connection.
    // A later follow must not emit the legacy push and resource copies twice.
    let prime = Command::new(ff_rdp_bin())
        .args(base_args(port))
        .arg("console")
        .output()
        .expect("prime daemon console listeners");
    assert!(
        prime.status.success(),
        "prime failed: {}{}",
        String::from_utf8_lossy(&prime.stdout),
        String::from_utf8_lossy(&prime.stderr)
    );
    violations.extend(assert_route_sees_console(&base_args(port), "daemon-primed"));

    stop_daemon(port);
    assert!(violations.is_empty(), "{}", violations.join("\n"));
}

/// Observe the actual CLI connection, then probe each observed actor after the
/// ticker has stopped. unrecognizedPacketType means follow left the actor alive;
/// noSuchActor proves cleanup while the connection is still open. Release plus a
/// second probe verifies destruction even for symbols, which send no release ACK.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_252_direct_follow_releases_filtered_and_nested_grips() {
    use ff_rdp_core::{ProtocolError, RdpTransport};
    use serde_json::{Value, json};
    use std::collections::{BTreeMap, BTreeSet};
    use std::io::Write;
    use std::net::TcpListener;

    fn grips(value: &Value, actors: &mut BTreeMap<String, String>) {
        match value {
            Value::Object(fields) => {
                if let (Some(kind @ ("object" | "longString" | "symbol")), Some(actor)) =
                    (value["type"].as_str(), value["actor"].as_str())
                {
                    actors.insert(actor.to_owned(), kind.to_owned());
                }
                for field in fields.values() {
                    grips(field, actors);
                }
            }
            Value::Array(items) => {
                for item in items {
                    grips(item, actors);
                }
            }
            _ => {}
        }
    }

    assert!(live_tests_enabled());
    let mut routes = HashMap::new();
    routes.insert(
        "/grips".to_owned(),
        FixtureRoute::html(
            "<script>setTimeout(()=>{let n=0;const timer=setInterval(()=>{\
         for(const [method,label] of [['log','level'],['warn','pattern'],['warn','keep']]){\
         console[method]('iter252-lifetime:'+label+':'+n,\
         {nested:Symbol('name:'+n+label+'x'.repeat(10000)),\
         text:n+label+'y'.repeat(10000),child:{n}});}\
         if(++n===60)clearInterval(timer);},50)},2000)</script>"
                .to_owned(),
        ),
    );
    let server = FixtureServer::start(routes).expect("fixture server");
    let ff = LiveFirefox::headless_on_random_port();
    let nav = Command::new(ff_rdp_bin())
        .args(direct_args(ff.port()))
        .args(["navigate", &format!("{}/grips", server.base_url())])
        .output()
        .unwrap();
    assert!(nav.status.success(), "{nav:?}");

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let proxy_port = listener.local_addr().unwrap().port();
    let firefox_port = ff.port();
    let proxy = std::thread::spawn(move || {
        let (mut client, _) = listener.accept().unwrap();
        let transport =
            RdpTransport::connect_raw("127.0.0.1", firefox_port, Duration::from_secs(10)).unwrap();
        let (mut reader, writer) = transport.split();
        reader
            .set_read_timeout(Some(Duration::from_millis(100)))
            .unwrap();
        let writer = Arc::new(Mutex::new(writer));
        let requests = Arc::new(Mutex::new(BTreeSet::<String>::new()));
        let client_reader = client.try_clone().unwrap();
        let request_writer = Arc::clone(&writer);
        let request_log = Arc::clone(&requests);
        let requests_thread = std::thread::spawn(move || {
            let mut reader = BufReader::new(client_reader);
            while let Ok(packet) = ff_rdp_core::transport::recv_from(&mut reader) {
                if packet["type"] == "release" {
                    request_log
                        .lock()
                        .unwrap()
                        .insert(packet["to"].as_str().unwrap().to_owned());
                }
                if request_writer.lock().unwrap().send(&packet).is_err() {
                    break;
                }
            }
        });
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut actors = BTreeMap::new();
        let mut acknowledged = BTreeSet::new();
        let mut peak = 0;
        let mut peak_unsent = 0;
        while Instant::now() < deadline {
            match reader.recv() {
                Ok(packet) => {
                    if packet["type"] == "resources-available-array" {
                        grips(&packet, &mut actors);
                    }
                    if packet.get("type").is_none()
                        && packet.get("error").is_none()
                        && let Some(actor) = packet["from"].as_str()
                        && requests.lock().unwrap().contains(actor)
                    {
                        acknowledged.insert(actor.to_owned());
                    }
                    peak = peak.max(actors.len().saturating_sub(acknowledged.len()));
                    peak_unsent = peak_unsent
                        .max(actors.len().saturating_sub(requests.lock().unwrap().len()));
                    let frame = ff_rdp_core::transport::encode_frame(&packet.to_string());
                    client.write_all(frame.as_bytes()).unwrap();
                }
                Err(ProtocolError::Timeout) => {}
                Err(error) => panic!("proxy read: {error}"),
            }
        }
        let mut retained = BTreeMap::<String, usize>::new();
        let mut absent = 0;
        let mut release_acks = BTreeMap::<String, usize>::new();
        for (actor, kind) in &actors {
            for second in [false, true] {
                if second {
                    writer
                        .lock()
                        .unwrap()
                        .send(&json!({"to":actor,"type":"release"}))
                        .unwrap();
                }
                // An unsupported request distinguishes a live actor from an
                // absent one without allocating more grips. It also provides
                // a barrier after release: Firefox 155 symbols send no ACK.
                writer
                    .lock()
                    .unwrap()
                    .send(&json!({"to":actor,"type":"iter252LifetimeProbe"}))
                    .unwrap();
                let end = Instant::now() + Duration::from_secs(5);
                loop {
                    assert!(Instant::now() < end, "release probe timed out for {actor}");
                    match reader.recv() {
                        Ok(reply) if reply["from"] == *actor && reply.get("type").is_none() => {
                            if reply.get("error").is_none() {
                                *release_acks.entry(kind.clone()).or_default() += 1;
                                continue;
                            } else if reply["error"] == "unrecognizedPacketType" {
                                assert!(!second, "actor survived release: {reply}");
                                *retained.entry(kind.clone()).or_default() += 1;
                            } else {
                                assert_eq!(reply["error"], "noSuchActor", "{reply}");
                                if !second {
                                    absent += 1;
                                }
                            }
                            break;
                        }
                        Ok(packet) => {
                            let frame = ff_rdp_core::transport::encode_frame(&packet.to_string());
                            client.write_all(frame.as_bytes()).unwrap();
                        }
                        Err(ProtocolError::Timeout) => {}
                        Err(error) => panic!("release probe: {error}"),
                    }
                }
            }
        }
        eprintln!(
            "iter252 lifetime actors={} requested={} acknowledged={} peak_unacknowledged={peak} peak_unsent={peak_unsent} retained={retained:?} absent={absent} probe_release_acks={release_acks:?}",
            actors.len(),
            requests.lock().unwrap().len(),
            acknowledged.len()
        );
        (actors.len(), retained, absent, requests_thread)
    });
    let mut child = Command::new(ff_rdp_bin())
        .args(direct_args(proxy_port))
        .args([
            "console",
            "--follow",
            "--level",
            "warn",
            "--pattern",
            "iter252-lifetime:keep:",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();
    let lines = collect_stdout_lines(&mut child);
    let result = proxy.join();
    let _ = child.kill();
    child.wait().unwrap();
    let (observed, retained, absent, requests_thread) = result.unwrap();
    requests_thread.join().unwrap();
    assert!(
        observed >= 500,
        "sustained logging must allocate all grip families: {observed}"
    );
    assert_eq!(
        lines.lock().unwrap().len(),
        60,
        "only matching warning messages are emitted"
    );
    assert!(
        retained.is_empty(),
        "Firefox retained actors after follow processed them: {retained:?}"
    );
    assert_eq!(absent, observed);
}
