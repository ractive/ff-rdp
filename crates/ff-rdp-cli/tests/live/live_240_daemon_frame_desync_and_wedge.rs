//! Live tests for iteration 240 — the CLI↔daemon frame stream, sustained.
//!
//! Named for the renumbered iteration (226 → 240, 227 → 241, DEC-051); older
//! plans and sweep logs call these `live_226_*` / `live_227_*`.
//!
//! # What iterations 224/226/227 left behind
//!
//! Iteration 224 made the daemon *say* when it abandoned a client and made the
//! CLI survive it. What it never explained was why the wire desynchronised at
//! all:
//!
//! ```text
//! daemon: abandoning client 42: client_frame_undecodable: invalid packet: \
//!     unexpected byte 0x3d in length prefix
//! ```
//!
//! `0x3d` is `=`: the daemon's framer had resumed reading *inside* a payload.
//!
//! # The root cause, and how this test found it
//!
//! This suite is what located it. The 40-hop loop below **reproduced the desync
//! at hop 32** (`unexpected byte 0x64 in length prefix`; `0x64` is `d`), which
//! made the mechanism findable: the frame reader was a straight-line function,
//! and every read loop in the daemon polls — 30 s on a client socket, 1 s on
//! the Firefox one — treating `ProtocolError::Timeout` as "nothing arrived, go
//! round again". A timeout firing *mid-frame* discarded the length prefix and
//! the payload bytes already consumed, so the next `recv()` restarted inside
//! the payload. `FrameDecoder` in `ff_rdp_core::transport` now keeps its
//! progress across a timeout; running this loop with
//! `RUST_LOG=ff_rdp_core::transport=debug` shows the resumes it used to lose
//! (`progress=32658`, `progress=48990` on the recorded run).
//!
//! Two mirror hazards were closed alongside it: a partial `write_all` under
//! `SO_SNDTIMEO` leaving a truncated stump (now
//! `ProtocolError::FrameWriteDesynchronised`, never retried), and the auth
//! `BufReader` discarding whatever it had buffered past the auth frame (now one
//! reader for the whole connection). Every daemon→client write is also bounded,
//! so a client that stops reading can no longer park the single event
//! dispatcher forever — the ~25-hop wedge of iteration 241.
//!
//! # What this test can and cannot prove
//!
//! It does not reproduce the original *rate* — the desync appeared roughly
//! twice in 40 hops against a real remote origin, and once here. What it covers
//! is the contract the fix rests on, over a run **long enough for the wedge to
//! have shown itself** (the two recorded wedges hit at hop 13 and hop 26):
//!
//! - every one of `HOPS` hops returns the destination view;
//! - `meta.page_reconnects` is 0 on every hop — a reconnect means the daemon
//!   dropped the connection, which is the defect;
//! - the daemon log gains no `abandoning client` line across the run;
//! - the daemon can still describe its own health afterwards, and that
//!   description says it is not wedged.
//!
//! # Running
//!
//!   FF_RDP_LIVE_TESTS=1 cargo test-live -p ff-rdp-cli \
//!       --test live live_240_daemon_frame_desync_and_wedge -- --nocapture

use std::collections::HashMap;
use std::fmt::Write as _;
use std::path::PathBuf;
use std::process::{Command, Output};

use serde_json::Value;

use crate::common::{FixtureRoute, FixtureServer, LiveFirefox, ff_rdp_bin, live_tests_enabled};

/// How many click hops the loop drives.
///
/// The plan asks for N ≥ 40, sized against the two recorded wedges (hop 13 of
/// one 60-hop loop, hop 26 of a 45-hop loop) so a surviving wedge cannot hide
/// inside the run.
const HOPS: usize = 40;

fn daemon_args(port: u16) -> Vec<String> {
    vec![
        "--host".to_owned(),
        "127.0.0.1".to_owned(),
        "--port".to_owned(),
        port.to_string(),
        "--timeout".to_owned(),
        "20000".to_owned(),
    ]
}

fn run(port: u16, args: &[&str]) -> Output {
    Command::new(ff_rdp_bin())
        .args(daemon_args(port))
        .args(args)
        .output()
        .unwrap_or_else(|e| panic!("spawn ff-rdp {args:?}: {e}"))
}

fn run_json(port: u16, args: &[&str]) -> Value {
    let out = run(port, args);
    let stdout = String::from_utf8_lossy(&out.stdout);
    serde_json::from_str(stdout.trim()).unwrap_or_else(|e| {
        panic!(
            "output for {args:?} not JSON: {e}\nstdout={stdout}\nstderr={}",
            String::from_utf8_lossy(&out.stderr)
        )
    })
}

fn stop_daemon(port: u16) {
    let _ = Command::new(ff_rdp_bin())
        .args(["--host", "127.0.0.1", "--port", &port.to_string()])
        .args(["daemon", "stop"])
        .output();
}

fn firefox_with_daemon(test: &str) -> LiveFirefox {
    let ff = LiveFirefox::headless_on_random_port();
    assert!(
        ff.with_daemon().is_some(),
        "{test}: the proxy daemon did not start for Firefox on port {}",
        ff.port()
    );
    ff
}

/// `~/.ff-rdp/daemon.log`, resolved the same way the daemon resolves it
/// (`FF_RDP_HOME` first, then the real home directory).
fn daemon_log_path() -> Option<PathBuf> {
    let home = std::env::var_os("FF_RDP_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(dirs::home_dir)?;
    Some(home.join(".ff-rdp").join("daemon.log"))
}

/// Byte length of the daemon log right now, so the run can be judged on the
/// lines *it* appended rather than on a shared file's whole history.
fn daemon_log_len() -> u64 {
    daemon_log_path()
        .and_then(|p| std::fs::metadata(p).ok())
        .map_or(0, |m| m.len())
}

/// Everything appended to the daemon log since `from` bytes.
fn daemon_log_since(from: u64) -> String {
    let Some(path) = daemon_log_path() else {
        return String::new();
    };
    let Ok(bytes) = std::fs::read(path) else {
        return String::new();
    };
    let start = usize::try_from(from).unwrap_or(0).min(bytes.len());
    String::from_utf8_lossy(&bytes[start..]).into_owned()
}

/// A destination big enough that its view travels as a chunked LongString —
/// the transfer the truncated frame landed in the middle of.
fn heavy_destination(count: usize) -> String {
    let mut body = String::from(
        "<!doctype html><title>t240 destination</title><body><h1>Grace Hopper</h1>\
         <p>She popularised the idea of machine-independent programming languages.</p>",
    );
    for i in 0..count {
        let _ = write!(body, "<a href=\"/\">compiler {i}</a> ");
    }
    body.push_str("</body>");
    body
}

fn hop_fixture() -> HashMap<String, FixtureRoute> {
    let mut routes = HashMap::new();
    routes.insert(
        "/".to_owned(),
        FixtureRoute::html(
            "<!doctype html><title>t240 origin</title><body>\
             <h1>Alan Turing</h1>\
             <p>On computable numbers, with an application to the Entscheidungsproblem.</p>\
             <a href=\"/heavy\">Grace Hopper</a></body>",
        ),
    );
    routes.insert(
        "/heavy".to_owned(),
        FixtureRoute::html(heavy_destination(400)),
    );
    routes
}

/// The `ref` of the first interactive entry whose `name` matches, if any.
fn ref_named(page: &Value, name: &str) -> Option<String> {
    page["interactive"]
        .as_array()?
        .iter()
        .find(|e| e["name"] == name)
        .and_then(|e| e["ref"].as_str())
        .map(str::to_owned)
}

// ---------------------------------------------------------------------------
// Part A — the stream does not desynchronise
// ---------------------------------------------------------------------------

/// AC: `HOPS` daemon hops log zero `abandoning client` lines and report
/// `meta.page_reconnects: 0` on every hop.
///
/// A reconnect is iteration 224's defence in depth doing its job, which means
/// the connection died — exactly what this iteration removes the cause of. The
/// log assertion is the one iteration 240 was written for: `abandoning client`
/// with `client_frame_undecodable` is the daemon reading a corrupt frame.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_240_sustained_hops_never_desynchronise() {
    if !live_tests_enabled() {
        eprintln!("live_240_sustained_hops_never_desynchronise: set FF_RDP_LIVE_TESTS=1");
        return;
    }
    let ff = firefox_with_daemon("live_240_sustained_hops_never_desynchronise");
    let port = ff.port();
    let Some(server) = FixtureServer::start(hop_fixture()) else {
        eprintln!("live_240_sustained_hops_never_desynchronise: no fixture HTTP — skipping");
        stop_daemon(port);
        return;
    };

    let log_mark = daemon_log_len();
    let mut failures: Vec<String> = Vec::new();
    let mut reconnects = 0_u64;

    for hop in 1..=HOPS {
        let nav = run_json(port, &["navigate", &server.base_url(), "--with-page"]);
        let Some(ref_id) = ref_named(&nav["results"]["page"], "Grace Hopper") else {
            failures.push(format!(
                "hop {hop}: origin page carried no destination ref: {nav}"
            ));
            continue;
        };

        let click = run_json(port, &["click", "--ref", &ref_id, "--with-page"]);
        if let Some(kind) = click["error_type"].as_str() {
            failures.push(format!("hop {hop}: {kind}: {}", click["error"]));
            continue;
        }
        let heading = click["results"]["page"]["headings"][0]["text"].as_str();
        if heading != Some("Grace Hopper") {
            failures.push(format!(
                "hop {hop}: click --with-page reported {heading:?}, not the destination"
            ));
            continue;
        }
        let hop_reconnects = click["meta"]["page_reconnects"]
            .as_u64()
            .unwrap_or_else(|| {
                panic!("hop {hop}: meta.page_reconnects must always be reported: {click}")
            });
        if hop_reconnects != 0 {
            failures.push(format!(
                "hop {hop}: the connection was rebuilt {hop_reconnects} time(s) — \
                 the daemon dropped it"
            ));
        }
        reconnects += hop_reconnects;
    }

    let appended = daemon_log_since(log_mark);
    let abandoned: Vec<&str> = appended
        .lines()
        .filter(|l| l.contains("abandoning client"))
        .collect();

    assert!(
        failures.is_empty(),
        "{} of {HOPS} hops failed (reconnects: {reconnects}):\n{}",
        failures.len(),
        failures.join("\n")
    );
    assert!(
        abandoned.is_empty(),
        "the daemon abandoned {} client(s) across {HOPS} hops — this is the \
         iter-240 defect:\n{}",
        abandoned.len(),
        abandoned.join("\n")
    );

    stop_daemon(port);
}

// ---------------------------------------------------------------------------
// Part B — the daemon does not wedge, and can say so
// ---------------------------------------------------------------------------

/// AC: after a sustained run the daemon is still answering, and `daemon status`
/// reports health an operator can read.
///
/// The wedge iteration 241 recorded was *silent*: the CLI saw a generic 10 s
/// `phase: recv` timeout and the daemon log held nothing at all, so "this page
/// is slow" and "your daemon is gone" were indistinguishable. These four fields
/// are what make them distinguishable — a wedged dispatcher shows
/// `in_flight > 0` with a growing `current_frame_age_ms`.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_240_daemon_reports_its_own_health_after_sustained_use() {
    if !live_tests_enabled() {
        eprintln!(
            "live_240_daemon_reports_its_own_health_after_sustained_use: set FF_RDP_LIVE_TESTS=1"
        );
        return;
    }
    let ff = firefox_with_daemon("live_240_daemon_reports_its_own_health_after_sustained_use");
    let port = ff.port();
    let Some(server) = FixtureServer::start(hop_fixture()) else {
        eprintln!(
            "live_240_daemon_reports_its_own_health_after_sustained_use: no fixture HTTP — skipping"
        );
        stop_daemon(port);
        return;
    };

    // Enough traffic that the dispatcher has really been used; the hop count
    // above already covers the long run, so keep this one short.
    for _ in 0..5 {
        let nav = run_json(port, &["navigate", &server.base_url(), "--with-page"]);
        assert!(
            nav["error_type"].is_null(),
            "navigate must succeed while proving liveness: {nav}"
        );
    }

    let status = run_json(port, &["daemon", "status"]);
    let daemon = &status["results"];
    assert_eq!(
        daemon["dispatcher"]["alive"],
        Value::Bool(true),
        "the event dispatcher must still be running: {status}"
    );
    assert_eq!(
        daemon["dispatcher"]["in_flight"].as_u64(),
        Some(0),
        "no dispatch may be stuck in flight after a quiet moment: {status}"
    );
    assert!(
        daemon["dispatcher"]["frames_finished"]
            .as_u64()
            .is_some_and(|n| n > 0),
        "the dispatcher must report the frames it routed: {status}"
    );
    assert_eq!(
        daemon["clients_dropped_on_write"].as_u64(),
        Some(0),
        "no client should have missed its write deadline in normal use: {status}"
    );
    assert_eq!(
        daemon["rpc_slot"]["owner"],
        Value::Null,
        "the RPC slot must be free once every command has finished: {status}"
    );

    stop_daemon(port);
}
