//! iter-138 — navigation truthfulness II: HTTP status, SPA history, honest
//! timeouts.
//!
//! Theme A: `navigate` reports the main document's HTTP status.
//! Theme B (REGRESSION fix, iter-130): `back`/`forward` across a same-document
//! (SPA `pushState`) history entry complete promptly instead of hard-failing
//! with a Timeout.
//! Theme C: same-page fragment navigation (`#frag`) succeeds instead of
//! burning the full timeout.
//! Theme D: a genuine commit-wait timeout reports the real wall-clock elapsed
//! time, not an internal sub-budget.
//! Theme F: `back`/`forward`'s `committed_url` always matches the TOP-level
//! document, never a subframe's.
//! Theme G: `navigate --with-network` keeps `committed_url`/`ready_state`
//! (and now `status`) alongside the captured network data.
//!
//! daemon-parity: every test in this file drives the CLI over the **default**
//! (daemon) connection — no test here passes `--no-daemon`. Per iter-137 Run
//! guidance (this plan's own Notes section), a live test that only exercises
//! `--no-daemon` proves nothing about the path real invocations use.
//!
//! # Running
//!
//!   FF_RDP_LIVE_TESTS=1 cargo test-live -p ff-rdp-cli --test live live_138 -- --nocapture

use std::collections::HashMap;
use std::process::{Command, Output};
use std::time::Instant;

use crate::common::{FixtureRoute, FixtureServer, LiveFirefox, ff_rdp_bin};

/// Global args for the **default** connection mode: no `--no-daemon`, so the
/// CLI auto-starts and proxies through the daemon — mirrors
/// `live_137_daemon_mode_parity::daemon_args`.
fn daemon_args(port: u16) -> Vec<String> {
    vec![
        "--host".to_owned(),
        "127.0.0.1".to_owned(),
        "--port".to_owned(),
        port.to_string(),
        "--timeout".to_owned(),
        "15000".to_owned(),
    ]
}

fn parse_json(output: &Output) -> serde_json::Value {
    let s = String::from_utf8_lossy(&output.stdout);
    serde_json::from_str(s.trim()).unwrap_or_else(|e| {
        panic!(
            "stdout is not valid JSON: {e}\nstdout={s}\nstderr={}",
            String::from_utf8_lossy(&output.stderr)
        )
    })
}

fn results(output: &Output) -> serde_json::Value {
    parse_json(output)["results"].clone()
}

fn combined(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn stop_daemon(port: u16) {
    let _ = Command::new(ff_rdp_bin())
        .args(["--host", "127.0.0.1", "--port", &port.to_string()])
        .args(["daemon", "stop"])
        .output();
}

fn firefox_with_daemon(test: &str) -> Option<LiveFirefox> {
    let ff = LiveFirefox::headless_on_random_port()?;
    if ff.with_daemon().is_none() {
        eprintln!("{test}: daemon did not start — skipping");
        return None;
    }
    Some(ff)
}

/// AC: `live_138_navigate_reports_404` — `navigate` to a known 404 reports
/// status 404.
#[test]
#[ignore = "requires Firefox — FF_RDP_LIVE_TESTS=1"]
fn live_138_navigate_reports_404() {
    if std::env::var("FF_RDP_LIVE_TESTS").is_err() {
        eprintln!("live_138_navigate_reports_404: set FF_RDP_LIVE_TESTS=1 to run");
        return;
    }
    let Some(ff) = firefox_with_daemon("live_138_navigate_reports_404") else {
        eprintln!("live_138_navigate_reports_404: Firefox not available — skipping");
        return;
    };
    let port = ff.port();

    // No routes registered — FixtureServer answers every path with a real
    // `404 Not Found` (see `handle_connection`'s fallback in tests/common).
    let Some(server) = FixtureServer::start(HashMap::new()) else {
        eprintln!("live_138_navigate_reports_404: could not bind local HTTP — skipping");
        stop_daemon(port);
        return;
    };
    let url = format!("{}/this-page-does-not-exist-xyz", server.base_url());

    let nav = Command::new(ff_rdp_bin())
        .args(daemon_args(port))
        .args(["navigate", &url])
        .output()
        .expect("navigate 404 page");
    let out = combined(&nav);
    stop_daemon(port);

    assert!(
        nav.status.success(),
        "navigate to a 404 page must still SUCCEED (the page loaded, it just \
         returned 404) — {out}"
    );
    let r = results(&nav);
    assert_eq!(
        r["status"], 404,
        "navigate must report the main document's real HTTP status: {r}"
    );
}

/// AC: `live_138_navigate_reports_200` — status 200 on a normal page (no
/// false positives).
#[test]
#[ignore = "requires Firefox — FF_RDP_LIVE_TESTS=1"]
fn live_138_navigate_reports_200() {
    if std::env::var("FF_RDP_LIVE_TESTS").is_err() {
        eprintln!("live_138_navigate_reports_200: set FF_RDP_LIVE_TESTS=1 to run");
        return;
    }
    let Some(ff) = firefox_with_daemon("live_138_navigate_reports_200") else {
        eprintln!("live_138_navigate_reports_200: Firefox not available — skipping");
        return;
    };
    let port = ff.port();

    let mut routes = HashMap::new();
    routes.insert(
        "/page".to_owned(),
        FixtureRoute::html("<!doctype html><title>iter-138 200</title><h1>hi</h1>"),
    );
    let Some(server) = FixtureServer::start(routes) else {
        eprintln!("live_138_navigate_reports_200: could not bind local HTTP — skipping");
        stop_daemon(port);
        return;
    };
    let url = format!("{}/page", server.base_url());

    let nav = Command::new(ff_rdp_bin())
        .args(daemon_args(port))
        .args(["navigate", &url])
        .output()
        .expect("navigate 200 page");
    let out = combined(&nav);
    stop_daemon(port);

    assert!(nav.status.success(), "navigate must succeed: {out}");
    let r = results(&nav);
    assert_eq!(r["status"], 200, "navigate must report status 200: {r}");
}

/// AC: `live_138_pushstate_back_succeeds` — `back` across a `pushState` entry
/// returns the real URL promptly with exit 0. Asserts wall-clock well under
/// the timeout.
///
/// Pre-fix repro (this plan's Theme B, a regression introduced by iter-130):
/// `back` across a same-document `popstate` produced no fresh navigation and
/// no `readyState` transition, so the shared commit-wait could never be
/// satisfied — a correct traversal hard-failed with `Timeout` (exit 124)
/// after burning the whole `--timeout` budget.
#[test]
#[ignore = "requires Firefox — FF_RDP_LIVE_TESTS=1"]
fn live_138_pushstate_back_succeeds() {
    if std::env::var("FF_RDP_LIVE_TESTS").is_err() {
        eprintln!("live_138_pushstate_back_succeeds: set FF_RDP_LIVE_TESTS=1 to run");
        return;
    }
    let Some(ff) = firefox_with_daemon("live_138_pushstate_back_succeeds") else {
        eprintln!("live_138_pushstate_back_succeeds: Firefox not available — skipping");
        return;
    };
    let port = ff.port();

    let mut routes = HashMap::new();
    routes.insert(
        "/page".to_owned(),
        FixtureRoute::html("<!doctype html><title>iter-138 pushState</title><h1>hi</h1>"),
    );
    let Some(server) = FixtureServer::start(routes) else {
        eprintln!("live_138_pushstate_back_succeeds: could not bind local HTTP — skipping");
        stop_daemon(port);
        return;
    };
    let url = format!("{}/page", server.base_url());

    let nav = Command::new(ff_rdp_bin())
        .args(daemon_args(port))
        .args(["navigate", &url])
        .output()
        .expect("navigate");
    if !nav.status.success() {
        eprintln!(
            "live_138_pushstate_back_succeeds: navigate failed — {}",
            combined(&nav)
        );
        stop_daemon(port);
        return;
    }

    let push = Command::new(ff_rdp_bin())
        .args(daemon_args(port))
        .args(["eval", "history.pushState({}, '', '/route1')"])
        .output()
        .expect("pushState");
    assert!(
        push.status.success(),
        "pushState eval must succeed: {}",
        combined(&push)
    );

    let timeout_ms: u64 = 8000;
    let started = Instant::now();
    let back = Command::new(ff_rdp_bin())
        .args(daemon_args(port))
        .args(["back", "--timeout", &timeout_ms.to_string()])
        .output()
        .expect("back");
    let wall_ms = started.elapsed().as_millis();
    let out = combined(&back);
    stop_daemon(port);

    assert!(
        back.status.success(),
        "back across a pushState entry must SUCCEED — the traversal itself \
         works, only the wait used to hard-fail (iter-130 regression): {out}"
    );
    assert!(
        wall_ms < u128::from(timeout_ms) / 2,
        "back must resolve via the same-document commit check (well under \
         the {timeout_ms}ms --timeout), took {wall_ms}ms: {out}"
    );

    let r = results(&back);
    let committed = r["committed_url"].as_str().unwrap_or("");
    assert!(
        committed.ends_with("/page"),
        "back must land on the pre-pushState URL, got {committed:?}: {r}"
    );
}

/// AC: `live_138_fragment_navigate_succeeds` — `navigate` to `#frag` succeeds.
///
/// Pre-fix repro: a same-page fragment navigation is a same-document
/// navigation (same root cause family as Theme B) — the commit wait burned
/// the full `--timeout` and returned a Timeout error even though
/// `location.href` confirmed the fragment navigation had succeeded.
#[test]
#[ignore = "requires Firefox — FF_RDP_LIVE_TESTS=1"]
fn live_138_fragment_navigate_succeeds() {
    if std::env::var("FF_RDP_LIVE_TESTS").is_err() {
        eprintln!("live_138_fragment_navigate_succeeds: set FF_RDP_LIVE_TESTS=1 to run");
        return;
    }
    let Some(ff) = firefox_with_daemon("live_138_fragment_navigate_succeeds") else {
        eprintln!("live_138_fragment_navigate_succeeds: Firefox not available — skipping");
        return;
    };
    let port = ff.port();

    let mut routes = HashMap::new();
    routes.insert(
        "/page".to_owned(),
        FixtureRoute::html(
            "<!doctype html><title>iter-138 fragment</title><h1 id=\"frag\">hi</h1>",
        ),
    );
    let Some(server) = FixtureServer::start(routes) else {
        eprintln!("live_138_fragment_navigate_succeeds: could not bind local HTTP — skipping");
        stop_daemon(port);
        return;
    };
    let url = format!("{}/page", server.base_url());

    let nav = Command::new(ff_rdp_bin())
        .args(daemon_args(port))
        .args(["navigate", &url])
        .output()
        .expect("navigate");
    if !nav.status.success() {
        eprintln!(
            "live_138_fragment_navigate_succeeds: navigate failed — {}",
            combined(&nav)
        );
        stop_daemon(port);
        return;
    }

    let frag_url = format!("{url}#frag");
    let timeout_ms: u64 = 8000;
    let started = Instant::now();
    let frag_nav = Command::new(ff_rdp_bin())
        .args(daemon_args(port))
        .args(["navigate", &frag_url, "--timeout", &timeout_ms.to_string()])
        .output()
        .expect("fragment navigate");
    let wall_ms = started.elapsed().as_millis();
    let out = combined(&frag_nav);
    stop_daemon(port);

    assert!(
        frag_nav.status.success(),
        "fragment navigation must succeed, not report a Timeout: {out}"
    );
    assert!(
        wall_ms < u128::from(timeout_ms) / 2,
        "fragment navigate must resolve promptly, took {wall_ms}ms: {out}"
    );
    let r = results(&frag_nav);
    let committed = r["committed_url"].as_str().unwrap_or("");
    assert!(
        committed.ends_with("#frag"),
        "fragment navigate's committed_url should carry the fragment, got \
         {committed:?}: {r}"
    );
}

/// AC: `live_138_timeout_message_matches_wall_clock` — the reported budget is
/// within tolerance of the observed wall-clock elapsed time.
///
/// Uses a `back` with genuinely nothing to go back to (a fresh tab has no
/// prior history entry) as a deterministic, network-free way to force a real
/// commit-wait timeout: no document-event fires, `location.href` never
/// changes (so the iter-138 same-document check never resolves either), and
/// `document.readyState` never changes — every completion path this wait
/// tries is exhausted, so it fails honestly after the full `--timeout`.
#[test]
#[ignore = "requires Firefox — FF_RDP_LIVE_TESTS=1"]
fn live_138_timeout_message_matches_wall_clock() {
    if std::env::var("FF_RDP_LIVE_TESTS").is_err() {
        eprintln!("live_138_timeout_message_matches_wall_clock: set FF_RDP_LIVE_TESTS=1 to run");
        return;
    }
    let Some(ff) = firefox_with_daemon("live_138_timeout_message_matches_wall_clock") else {
        eprintln!("live_138_timeout_message_matches_wall_clock: Firefox not available — skipping");
        return;
    };
    let port = ff.port();

    let timeout_ms: u64 = 3000;
    let started = Instant::now();
    let back = Command::new(ff_rdp_bin())
        .args(daemon_args(port))
        .args(["back", "--timeout", &timeout_ms.to_string()])
        .output()
        .expect("back with no history");
    let wall_ms = started.elapsed().as_millis();
    let out = combined(&back);
    stop_daemon(port);

    if back.status.success() {
        // Some Firefox versions may report an actor-level error for a no-op
        // goBack rather than timing out our wait — that's a different (and
        // fine) failure mode this test isn't targeting; skip rather than
        // fail on an environment-dependent path.
        eprintln!(
            "live_138_timeout_message_matches_wall_clock: back unexpectedly \
             succeeded (no history to exercise the timeout path) — skipping: {out}"
        );
        return;
    }
    assert_eq!(
        back.status.code(),
        Some(124),
        "expected a Timeout exit (124): {out}"
    );

    // Extract the "within {N}ms" number the timeout message reports.
    let Some(idx) = out.find("within ") else {
        panic!("timeout message must report a budget with 'within Nms': {out}");
    };
    let digits: String = out[idx + "within ".len()..]
        .chars()
        .take_while(char::is_ascii_digit)
        .collect();
    let reported_ms: u128 = digits
        .parse()
        .unwrap_or_else(|_| panic!("could not parse reported ms from: {out}"));

    // Tolerance: within 50% of the measured wall-clock, or an absolute 1500ms
    // margin — generous enough to absorb process spawn / IPC jitter, but far
    // tighter than the ~3x under-report iter-138 fixed (a --timeout of 8000ms
    // used to report "within 2384ms").
    let lower = wall_ms.saturating_sub(1500).min(wall_ms / 2);
    let upper = wall_ms + 1500;
    assert!(
        reported_ms >= lower && reported_ms <= upper,
        "reported budget {reported_ms}ms must be close to the measured \
         wall-clock {wall_ms}ms (--timeout was {timeout_ms}ms): {out}"
    );
}

/// AC: `live_138_back_forward_committed_url_is_top_frame` — `committed_url`
/// matches `eval location.href` after traversal on a page with cross-origin
/// subframes.
///
/// Pre-fix repro: `back`/`forward` could report a *subframe's* URL as
/// `committed_url` (e.g. an ad/analytics iframe reloading) because
/// `watchTargets("frame")` makes Firefox deliver `document-event`s for
/// subframe targets too, and this wait loop trusted whichever one arrived
/// first. Two `FixtureServer`s on different ports are genuinely
/// cross-origin (scheme+host+port), reproducing the same target-mix
/// `watchTargets("frame")` triggers without needing real network access.
#[test]
#[ignore = "requires Firefox — FF_RDP_LIVE_TESTS=1"]
fn live_138_back_forward_committed_url_is_top_frame() {
    if std::env::var("FF_RDP_LIVE_TESTS").is_err() {
        eprintln!(
            "live_138_back_forward_committed_url_is_top_frame: set FF_RDP_LIVE_TESTS=1 to run"
        );
        return;
    }
    let Some(ff) = firefox_with_daemon("live_138_back_forward_committed_url_is_top_frame") else {
        eprintln!(
            "live_138_back_forward_committed_url_is_top_frame: Firefox not available — skipping"
        );
        return;
    };
    let port = ff.port();

    let mut iframe_routes = HashMap::new();
    iframe_routes.insert(
        "/iframe".to_owned(),
        FixtureRoute::html("<!doctype html><title>iter-138 subframe</title><p>subframe</p>"),
    );
    let Some(iframe_server) = FixtureServer::start(iframe_routes) else {
        eprintln!(
            "live_138_back_forward_committed_url_is_top_frame: could not bind subframe HTTP — skipping"
        );
        stop_daemon(port);
        return;
    };

    let mut routes = HashMap::new();
    routes.insert(
        "/a".to_owned(),
        FixtureRoute::html(format!(
            "<!doctype html><title>iter-138 A</title><h1>A</h1>\
             <iframe src=\"{}/iframe\"></iframe>",
            iframe_server.base_url()
        )),
    );
    routes.insert(
        "/b".to_owned(),
        FixtureRoute::html("<!doctype html><title>iter-138 B</title><h1>B</h1>"),
    );
    let Some(server) = FixtureServer::start(routes) else {
        eprintln!(
            "live_138_back_forward_committed_url_is_top_frame: could not bind top HTTP — skipping"
        );
        stop_daemon(port);
        return;
    };
    let url_a = format!("{}/a", server.base_url());
    let url_b = format!("{}/b", server.base_url());

    for url in [&url_a, &url_b] {
        let nav = Command::new(ff_rdp_bin())
            .args(daemon_args(port))
            .args(["navigate", url])
            .output()
            .expect("navigate");
        if !nav.status.success() {
            eprintln!(
                "live_138_back_forward_committed_url_is_top_frame: navigate {url} failed — {}",
                combined(&nav)
            );
            stop_daemon(port);
            return;
        }
    }

    let back = Command::new(ff_rdp_bin())
        .args(daemon_args(port))
        .args(["back"])
        .output()
        .expect("back");
    assert!(
        back.status.success(),
        "back must succeed: {}",
        combined(&back)
    );
    let back_results = results(&back);
    let back_committed = back_results["committed_url"]
        .as_str()
        .unwrap_or("")
        .to_owned();

    let eval_href = Command::new(ff_rdp_bin())
        .args(daemon_args(port))
        .args(["eval", "location.href"])
        .output()
        .expect("eval location.href");
    stop_daemon(port);
    assert!(
        eval_href.status.success(),
        "eval location.href must succeed: {}",
        combined(&eval_href)
    );
    let real_href = results(&eval_href).as_str().unwrap_or("").to_owned();

    assert!(
        back_committed.ends_with("/a"),
        "back's committed_url must be the TOP-level page (/a), not the \
         subframe's URL, got {back_committed:?}: {back_results}"
    );
    assert_eq!(
        back_committed, real_href,
        "back's committed_url must match the real top-level location.href \
         (never a subframe's URL)"
    );
}

/// AC: `live_138_with_network_keeps_envelope` — `navigate --with-network`
/// returns non-null `committed_url` and `ready_state` alongside network data.
#[test]
#[ignore = "requires Firefox — FF_RDP_LIVE_TESTS=1"]
fn live_138_with_network_keeps_envelope() {
    if std::env::var("FF_RDP_LIVE_TESTS").is_err() {
        eprintln!("live_138_with_network_keeps_envelope: set FF_RDP_LIVE_TESTS=1 to run");
        return;
    }
    let Some(ff) = firefox_with_daemon("live_138_with_network_keeps_envelope") else {
        eprintln!("live_138_with_network_keeps_envelope: Firefox not available — skipping");
        return;
    };
    let port = ff.port();

    let mut routes = HashMap::new();
    routes.insert(
        "/page".to_owned(),
        FixtureRoute::html("<!doctype html><title>iter-138 with-network</title><h1>hi</h1>"),
    );
    let Some(server) = FixtureServer::start(routes) else {
        eprintln!("live_138_with_network_keeps_envelope: could not bind local HTTP — skipping");
        stop_daemon(port);
        return;
    };
    let url = format!("{}/page", server.base_url());

    let nav = Command::new(ff_rdp_bin())
        .args(daemon_args(port))
        .args(["navigate", &url, "--with-network"])
        .output()
        .expect("navigate --with-network");
    let out = combined(&nav);
    stop_daemon(port);

    assert!(
        nav.status.success(),
        "navigate --with-network must succeed: {out}"
    );
    let r = results(&nav);
    assert!(
        r.get("network").is_some(),
        "network data must still be present: {r}"
    );
    let committed = r["committed_url"].as_str().unwrap_or("");
    assert!(
        committed.ends_with("/page"),
        "committed_url must not be dropped by --with-network, got {committed:?}: {r}"
    );
    assert!(
        r["ready_state"].is_string(),
        "ready_state must not be dropped by --with-network: {r}"
    );
    assert_eq!(
        r["status"], 200,
        "--with-network should surface the main document's status too: {r}"
    );
}
