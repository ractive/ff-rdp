//! Live tests for the dogfooding-session-64 navigation truthfulness fixes.
//!
//! - #1  `navigate` (and a navigating `click`) onto Firefox's certificate
//!   error page fails with `nav_cert_error` and Firefox's own error code,
//!   instead of reporting `ready_state: complete`.
//! - #2  the navigation wait ignores subframe `document-event`s: an iframe's
//!   `about:blank` reaching `dom-complete` is not the page completing.
//! - #12/#28 a `click` that starts a navigation waits for the destination to
//!   commit, so the next command — or the next `run` step — reads it.
//!
//! # Running
//!
//!   FF_RDP_LIVE_TESTS=1 cargo test -p ff-rdp-cli --test live live_df64 -- --include-ignored
//!
//! The certificate test reaches badssl.com and also needs
//! `FF_RDP_LIVE_NETWORK_TESTS=1 FF_RDP_LIVE_SITES_TESTS=1`: a local HTTPS
//! origin with a broken certificate is not something the std-only fixture
//! server can provide.

use std::collections::HashMap;
use std::process::{Command, Output};
use std::time::Duration;

use serde_json::Value;

use crate::common::{
    FixtureRoute, FixtureServer, LiveFirefox, ff_rdp_bin, live_sites_tests_enabled,
    live_tests_enabled,
};

fn run(port: u16, args: &[&str]) -> Output {
    Command::new(ff_rdp_bin())
        .args([
            "--host",
            "127.0.0.1",
            "--port",
            &port.to_string(),
            "--timeout",
            "30000",
        ])
        .args(args)
        .output()
        .expect("ff-rdp command")
}

fn stdout_json(out: &Output, what: &str) -> Value {
    let stdout = String::from_utf8_lossy(&out.stdout);
    serde_json::from_str(stdout.trim()).unwrap_or_else(|e| {
        panic!(
            "{what}: output not JSON ({e}): stdout={stdout} stderr={}",
            String::from_utf8_lossy(&out.stderr)
        )
    })
}

fn run_json(port: u16, args: &[&str]) -> Value {
    let out = run(port, args);
    assert!(
        out.status.success(),
        "{args:?} failed: stdout={} stderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    stdout_json(&out, &format!("{args:?}"))
}

fn eval_string(port: u16, js: &str) -> String {
    let v = run_json(port, &["eval", js]);
    v["results"].as_str().unwrap_or_default().to_owned()
}

/// #1: an expired certificate is a failed navigation, for `navigate` and for
/// a `click` that follows a link into it.
#[test]
#[ignore = "requires Firefox + badssl.com — set FF_RDP_LIVE_TESTS=1 FF_RDP_LIVE_NETWORK_TESTS=1 FF_RDP_LIVE_SITES_TESTS=1"]
fn live_df64_cert_error_is_a_failed_navigation() {
    if !live_sites_tests_enabled() {
        eprintln!(
            "live_df64_cert_error_is_a_failed_navigation: set FF_RDP_LIVE_TESTS=1 \
             FF_RDP_LIVE_NETWORK_TESTS=1 FF_RDP_LIVE_SITES_TESTS=1"
        );
        return;
    }
    let ff = LiveFirefox::headless_on_random_port();
    let port = ff.port();
    let bad = "https://expired.badssl.com/";

    let out = run(port, &["navigate", bad]);
    assert_eq!(
        out.status.code(),
        Some(8),
        "a cert error must exit 8: {}",
        String::from_utf8_lossy(&out.stdout)
    );
    let err = stdout_json(&out, "navigate");
    assert_eq!(err["error_type"], "nav_cert_error", "{err}");
    assert_eq!(err["url"], bad, "{err}");
    assert_eq!(
        err["firefox_error"], "SEC_ERROR_EXPIRED_CERTIFICATE",
        "{err}"
    );

    let mut routes = HashMap::new();
    routes.insert(
        "/".to_owned(),
        FixtureRoute::html(format!(
            "<!doctype html><title>df64</title><a id=bad href=\"{bad}\">bad</a>"
        )),
    );
    let server = FixtureServer::start(routes).expect("fixture server");
    run_json(port, &["navigate", &server.base_url()]);
    let out = run(port, &["click", "#bad"]);
    let err = stdout_json(&out, "click");
    assert_eq!(out.status.code(), Some(8), "{err}");
    assert_eq!(err["error_type"], "nav_cert_error", "{err}");
}

/// #2: a page whose iframe finishes long before the page does is reported
/// `complete` only once the page itself is — under `--throttle`, as in the
/// dogfooding repro, and the page is really complete when the next command
/// asks.
#[test]
#[ignore = "requires Firefox — set FF_RDP_LIVE_TESTS=1"]
fn live_df64_readiness_ignores_subframe_completion() {
    if !live_tests_enabled() {
        eprintln!("live_df64_readiness_ignores_subframe_completion: set FF_RDP_LIVE_TESTS=1");
        return;
    }
    let slow = Duration::from_millis(2500);
    let mut routes = HashMap::new();
    routes.insert(
        "/".to_owned(),
        FixtureRoute::html(
            "<!doctype html><title>df64-frames</title><body>\
             <script>for (let i = 0; i < 3; i++) \
               document.body.appendChild(document.createElement('iframe'));</script>\
             <img src=\"/slow.png\"></body>",
        ),
    );
    routes.insert(
        "/slow.png".to_owned(),
        FixtureRoute {
            content_type: "image/png",
            body: Vec::new(),
            extra_headers: Vec::new(),
            delay: slow,
        },
    );
    let server = FixtureServer::start(routes).expect("fixture server");
    let ff = LiveFirefox::headless_on_random_port();
    let port = ff.port();
    run_json(port, &["navigate", "about:blank"]);

    let nav = run_json(
        port,
        &[
            "navigate",
            &server.base_url(),
            "--throttle",
            "slow-3g",
            "--timeout",
            "60000",
        ],
    );
    assert_eq!(nav["results"]["ready_state"], "complete", "{nav}");
    let elapsed = nav["results"]["elapsed_ms"].as_u64().unwrap_or(0);
    let slow_ms = u64::try_from(slow.as_millis()).unwrap_or(u64::MAX);
    assert!(
        elapsed >= slow_ms,
        "`complete` before the delayed image could have loaded ({elapsed} ms < {slow_ms} ms) \
         — an iframe's dom-complete was taken for the page's: {nav}"
    );
    assert_eq!(eval_string(port, "document.readyState"), "complete");
}

/// #12/#28: a navigating click returns once the destination committed — the
/// next command reads it — and a `run` playbook's `wait`/`assert_url` after
/// such a click see the destination. Also: a `navigate` straight after a
/// click that returned at `interactive` reports the page it went to, not the
/// replayed lifecycle of the one it left.
#[test]
#[ignore = "requires Firefox — set FF_RDP_LIVE_TESTS=1"]
fn live_df64_click_waits_for_its_navigation() {
    if !live_tests_enabled() {
        eprintln!("live_df64_click_waits_for_its_navigation: set FF_RDP_LIVE_TESTS=1");
        return;
    }
    let mut routes = HashMap::new();
    routes.insert(
        "/".to_owned(),
        FixtureRoute::html(
            "<!doctype html><title>df64-a</title>\
             <a id=go href=\"/b\">go</a><button id=stay onclick=\"this.textContent='x'\">s</button>",
        ),
    );
    // Held back so the navigation is genuinely in flight when `click`'s eval
    // returns; the old behaviour returned before it committed.
    routes.insert(
        "/b".to_owned(),
        FixtureRoute::html(
            "<!doctype html><title>df64-b</title><div id=landed>b</div>\
             <img src=\"/b-slow.png\">",
        )
        .with_delay(Duration::from_millis(800)),
    );
    routes.insert(
        "/b-slow.png".to_owned(),
        FixtureRoute {
            content_type: "image/png",
            body: Vec::new(),
            extra_headers: Vec::new(),
            delay: Duration::from_millis(1500),
        },
    );
    let server = FixtureServer::start(routes).expect("fixture server");
    let base = server.base_url();
    let ff = LiveFirefox::headless_on_random_port();
    let port = ff.port();

    // A click that does not navigate reports `navigated: null`.
    run_json(port, &["navigate", &base]);
    let stay = run_json(port, &["click", "#stay"]);
    assert_eq!(stay["results"]["navigated"], Value::Null, "{stay}");

    let click = run_json(port, &["click", "#go"]);
    let navigated = &click["results"]["navigated"];
    assert_eq!(navigated["committed"], true, "{click}");
    assert_eq!(navigated["url"], format!("{base}/b"), "{click}");
    assert_eq!(navigated["status"], 200, "{click}");
    assert_eq!(eval_string(port, "location.pathname"), "/b");

    // `/b` is still loading its image: the next navigate must not report it.
    let back = run_json(port, &["navigate", &format!("{base}/?again=1")]);
    assert_eq!(
        back["results"]["committed_url"],
        format!("{base}/?again=1"),
        "{back}"
    );

    let script = serde_json::json!({
        "version": 1,
        "steps": [
            {"navigate": {"url": base}},
            {"click": {"selector": "#go"}},
            {"wait": {"selector": "#landed"}},
            {"assert_url": {"matches": "/b$"}},
        ]
    });
    let path = std::env::temp_dir().join(format!("ff-rdp-df64-play-{port}.json"));
    std::fs::write(&path, script.to_string()).expect("write playbook");
    let out = run(port, &["run", &path.to_string_lossy()]);
    let _ = std::fs::remove_file(&path);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success(),
        "playbook navigate→click→wait→assert_url must pass: {stdout} {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        stdout.contains("\"summary\":true,\"ok\":true"),
        "summary must be ok: {stdout}"
    );
}

/// Asserts `out` is the `nav_cert_error` envelope for `bad`, exit 8.
fn assert_cert_error(out: &Output, bad: &str, what: &str) {
    let err = stdout_json(out, what);
    assert_eq!(out.status.code(), Some(8), "{what} must exit 8: {err}");
    assert_eq!(err["error_type"], "nav_cert_error", "{what}: {err}");
    assert_eq!(err["url"], bad, "{what}: {err}");
    assert_eq!(
        err["firefox_error"], "SEC_ERROR_EXPIRED_CERTIFICATE",
        "{what}: {err}"
    );
}

/// N1/N2 of the session-64 verification: `click --with-page`, `back`,
/// `forward` and `reload` landing on the certificate error page fail the way
/// `navigate` does, instead of reporting the badssl URL as committed.
#[test]
#[ignore = "requires Firefox + badssl.com — set FF_RDP_LIVE_TESTS=1 FF_RDP_LIVE_NETWORK_TESTS=1 FF_RDP_LIVE_SITES_TESTS=1"]
fn live_df64_cert_error_fails_with_page_and_history() {
    if !live_sites_tests_enabled() {
        eprintln!(
            "live_df64_cert_error_fails_with_page_and_history: set FF_RDP_LIVE_TESTS=1 \
             FF_RDP_LIVE_NETWORK_TESTS=1 FF_RDP_LIVE_SITES_TESTS=1"
        );
        return;
    }
    let ff = LiveFirefox::headless_on_random_port();
    let port = ff.port();
    let bad = "https://expired.badssl.com/";

    // N1: the same link click as #1, with `--with-page`.
    run_json(
        port,
        &[
            "navigate",
            "--allow-unsafe-urls",
            &format!("data:text/html,<a id=bad href=\"{bad}\">x</a>"),
        ],
    );
    let out = run(port, &["click", "#bad", "--with-page"]);
    assert_cert_error(&out, bad, "click --with-page");

    // N2: history and reload across the error page.
    let mut routes = HashMap::new();
    routes.insert(
        "/".to_owned(),
        FixtureRoute::html("<!doctype html><title>df64-ok</title><p>ok</p>"),
    );
    let server = FixtureServer::start(routes).expect("fixture server");
    let good = server.base_url();
    run_json(port, &["navigate", &good]);
    assert_cert_error(&run(port, &["navigate", bad]), bad, "navigate");
    run_json(port, &["navigate", &format!("{good}/?again=1")]);

    assert_cert_error(&run(port, &["back"]), bad, "back onto the error page");
    assert_cert_error(&run(port, &["reload"]), bad, "reload on the error page");
    // A click on the error page that was already showing is not a failed
    // navigation. A short --timeout: the page view on an error page waits
    // for a readiness it never reports (backlog), and the click is what
    // this asserts.
    let out = Command::new(ff_rdp_bin())
        .args(["--host", "127.0.0.1", "--port", &port.to_string()])
        .args(["--timeout", "3000", "click", "body", "--with-page"])
        .output()
        .expect("ff-rdp command");
    assert!(
        out.status.success(),
        "click on the error page: {}",
        String::from_utf8_lossy(&out.stdout)
    );
    let on_error_page = stdout_json(&out, "click on the error page");
    assert_eq!(on_error_page["results"]["clicked"], true, "{on_error_page}");
    let ok = run_json(port, &["back"]);
    assert!(
        ok["results"]["committed_url"]
            .as_str()
            .is_some_and(|u| u.starts_with(&good)),
        "back off the error page must succeed: {ok}"
    );
    assert_cert_error(&run(port, &["forward"]), bad, "forward onto the error page");
}
