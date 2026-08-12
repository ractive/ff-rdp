//! Live tests for iteration 145 — error envelope completeness.
//!
//! [[iteration-145-error-envelope-completeness]] Theme A: `click`'s two
//! remaining bare-stderr-then-`AppError::Exit(1)` paths (a genuine JS
//! exception thrown by the click JS, at the top-level attempt and inside the
//! frame-scan retry) now route through the standard JSON error envelope
//! (`error_type: "User"`), matching `eval.rs`'s iter-141 Theme E handling of
//! a thrown script exception.
//!
//! daemon-parity: every test here uses [`daemon_args`] (no `--no-daemon`) —
//! the default connection mode is exactly what a real invocation uses,
//! following the pattern iteration 137 established
//! (`live_137_daemon_mode_parity.rs`) and iteration 141 continued
//! (`live_141_output_hygiene.rs`).
//!
//! # Running
//!
//!   FF_RDP_LIVE_TESTS=1 cargo test-live -p ff-rdp-cli \
//!       --test live live_145_error_envelope_completeness -- --nocapture

use std::collections::HashMap;
use std::process::{Command, Output};

use serde_json::Value;

use crate::common::{FixtureRoute, FixtureServer, LiveFirefox, ff_rdp_bin, live_tests_enabled};

/// Args for the **default** connection mode: no `--no-daemon`, so the CLI
/// auto-starts and proxies through the daemon — see the module-level
/// `daemon-parity` note.
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

fn stop_daemon(port: u16) {
    let _ = Command::new(ff_rdp_bin())
        .args(["--host", "127.0.0.1", "--port", &port.to_string()])
        .args(["daemon", "stop"])
        .output();
}

/// Bring up Firefox with a running daemon, or `None` with a printed reason.
fn firefox_with_daemon(test: &str) -> Option<LiveFirefox> {
    let ff = LiveFirefox::headless_on_random_port()?;
    if ff.with_daemon().is_none() {
        eprintln!("{test}: daemon did not start — skipping");
        return None;
    }
    Some(ff)
}

fn navigate(port: u16, url: &str) -> Output {
    Command::new(ff_rdp_bin())
        .args(daemon_args(port))
        .args(["navigate", url])
        .output()
        .expect("ff-rdp navigate")
}

/// Run `ff-rdp <args>` over the daemon connection and return the raw output
/// (caller decides success/failure — these tests are specifically about
/// *failure* shapes).
fn run(port: u16, args: &[&str]) -> Output {
    Command::new(ff_rdp_bin())
        .args(daemon_args(port))
        .args(args)
        .output()
        .unwrap_or_else(|e| panic!("spawn ff-rdp {args:?}: {e}"))
}

fn combined(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

/// Parse stdout as JSON, panicking with full context if it isn't. Used on
/// the *failure* paths this suite exercises — the whole point of iter-145
/// Theme A is that stdout must still be valid JSON when the command fails.
fn parse_json(output: &Output) -> Value {
    let s = String::from_utf8_lossy(&output.stdout);
    serde_json::from_str(s.trim()).unwrap_or_else(|e| {
        panic!(
            "stdout is not valid JSON on a failing command — this is exactly the iter-145 \
             regression (bare text on stderr, nothing parseable on stdout): {e}\n\
             stdout={s}\nstderr={}",
            String::from_utf8_lossy(&output.stderr)
        )
    })
}

/// Poll `daemon status` until it reports at least one **live** target — the
/// same wait iteration 137's daemon-parity suite uses before a `--frame`
/// probe, so the frame-scan test below doesn't race the daemon's
/// `watchTargets("frame")` subscription.
fn wait_for_live_targets(port: u16) -> bool {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    while std::time::Instant::now() < deadline {
        let out = Command::new(ff_rdp_bin())
            .args(["--host", "127.0.0.1", "--port", &port.to_string()])
            .args(["daemon", "status"])
            .output()
            .expect("daemon status");
        let text = String::from_utf8_lossy(&out.stdout);
        if let Ok(json) = serde_json::from_str::<Value>(&text)
            && json["results"]["live_target_count"].as_u64().unwrap_or(0) >= 1
        {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(250));
    }
    false
}

/// A self-hosted top page embedding a same-origin `<iframe>` — gives the
/// frame-scan test a non-top frame target without depending on outbound
/// network reachability (unlike the `CROSS_ORIGIN_FIXTURE` data: URL
/// iteration 129/137 used). The frame-target mechanism is per browsing
/// context, not per origin, so a same-origin frame exercises the same
/// `click_in_scanned_frame` code path.
fn start_iframe_fixture() -> Option<FixtureServer> {
    let mut routes = HashMap::new();
    routes.insert(
        "/".to_owned(),
        FixtureRoute::html(
            "<!doctype html><title>t145-top</title><body><iframe src=\"/frame\"></iframe></body>",
        ),
    );
    routes.insert(
        "/frame".to_owned(),
        FixtureRoute::html("<!doctype html><title>t145-frame</title><body>frame</body>"),
    );
    FixtureServer::start(routes)
}

/// An invalid CSS selector — `document.querySelector` throws a `SyntaxError`
/// for it, which is a genuine JS failure, not the `ELEMENT_NOT_FOUND_MARKER`
/// click.rs's own click JS throws for a selector that is merely absent. This
/// is the reproduction the plan's Notes section asked for: "reproduce an
/// actual click-time JS throw against real Firefox" without needing a
/// fixture page whose click handler throws (which `el.click()` /
/// `dispatchEvent` do not propagate synchronously — the browser reports
/// listener exceptions to the console instead).
const INVALID_SELECTOR: &str = ":::not-a-valid-selector";

/// AC: `live_145_click_js_exception_envelope` — a `click` whose injected JS
/// throws (top-level attempt, `click.rs`'s former line 399) returns a JSON
/// envelope on stdout with `error_type` set to `User`, exits non-zero, and
/// writes nothing to stderr.
#[test]
#[ignore = "requires headless Firefox; set FF_RDP_LIVE_TESTS=1"]
fn live_145_click_js_exception_envelope() {
    if !live_tests_enabled() {
        eprintln!("live_145_click_js_exception_envelope: set FF_RDP_LIVE_TESTS=1 to run");
        return;
    }
    let Some(ff) = firefox_with_daemon("live_145_click_js_exception_envelope") else {
        return;
    };
    let port = ff.port();

    let Some(server) = start_iframe_fixture() else {
        eprintln!("live_145_click_js_exception_envelope: could not bind fixture HTTP — skipping");
        stop_daemon(port);
        return;
    };

    let nav = navigate(port, &server.base_url());
    if !nav.status.success() {
        eprintln!(
            "live_145_click_js_exception_envelope: navigate failed — {}",
            combined(&nav)
        );
        stop_daemon(port);
        return;
    }

    // --no-wait: exercise `do_click`'s top-level attempt directly, not the
    // auto-wait pre-check (a separate, unaudited code path — out of scope
    // for this iteration; see the plan's Theme A).
    let click = run(port, &["click", INVALID_SELECTOR, "--no-wait"]);
    stop_daemon(port);

    assert!(
        !click.status.success(),
        "click with an invalid selector must exit non-zero: {}",
        combined(&click)
    );
    assert!(
        click.stderr.is_empty(),
        "JSON-only output: nothing may go to stderr on this failure path; got: {}",
        String::from_utf8_lossy(&click.stderr)
    );
    let json = parse_json(&click);
    assert_eq!(
        json["error_type"], "User",
        "a thrown click-JS exception is a user error, not an internal one; got: {json}"
    );
    let message = json["error"].as_str().unwrap_or_default();
    assert!(
        !message.is_empty(),
        "envelope error must carry the thrown exception's message; got: {json}"
    );

    eprintln!("live_145_click_js_exception_envelope: PASSED — {json}");
}

/// AC: `live_145_click_frame_scan_js_exception_envelope` — the same holds for
/// a throw raised inside the frame-scan path (`click.rs`'s former line 508),
/// not just the top-level attempt. `--frame` routes directly into
/// `click_in_scanned_frame` without touching the top-level attempt at all
/// (see `do_click`'s doc comment), so this exercises exactly that call site.
#[test]
#[ignore = "requires headless Firefox; set FF_RDP_LIVE_TESTS=1"]
fn live_145_click_frame_scan_js_exception_envelope() {
    if !live_tests_enabled() {
        eprintln!(
            "live_145_click_frame_scan_js_exception_envelope: set FF_RDP_LIVE_TESTS=1 to run"
        );
        return;
    }
    let Some(ff) = firefox_with_daemon("live_145_click_frame_scan_js_exception_envelope") else {
        return;
    };
    let port = ff.port();

    let Some(server) = start_iframe_fixture() else {
        eprintln!(
            "live_145_click_frame_scan_js_exception_envelope: could not bind fixture HTTP — skipping"
        );
        stop_daemon(port);
        return;
    };

    let nav = navigate(port, &server.base_url());
    if !nav.status.success() {
        eprintln!(
            "live_145_click_frame_scan_js_exception_envelope: navigate failed — {}",
            combined(&nav)
        );
        stop_daemon(port);
        return;
    }

    assert!(
        wait_for_live_targets(port),
        "daemon never reported live frame targets"
    );

    let click = run(port, &["click", INVALID_SELECTOR, "--frame", "/frame"]);
    stop_daemon(port);

    assert!(
        !click.status.success(),
        "click with an invalid selector inside the scanned frame must exit non-zero: {}",
        combined(&click)
    );
    assert!(
        click.stderr.is_empty(),
        "JSON-only output: nothing may go to stderr on this failure path; got: {}",
        String::from_utf8_lossy(&click.stderr)
    );
    let json = parse_json(&click);
    assert_eq!(
        json["error_type"], "User",
        "a thrown click-JS exception inside the scanned frame is a user error, \
         not an internal one; got: {json}"
    );
    let message = json["error"].as_str().unwrap_or_default();
    assert!(
        !message.is_empty(),
        "envelope error must carry the thrown exception's message; got: {json}"
    );

    eprintln!("live_145_click_frame_scan_js_exception_envelope: PASSED — {json}");
}

/// AC: `live_145_click_element_not_found_unchanged` — the existing
/// informative frame-aware not-found diagnostic (iter-129/iter-140) is
/// unperturbed by this iteration: same shape, same content, still fails fast
/// rather than paying the full auto-wait timeout. This iteration only
/// touched the *genuine exception* branch (`classify_click_exception`
/// returning `Some`); the not-found branch (`None`, "keep scanning") and its
/// final diagnostic are untouched code, and this test pins that.
#[test]
#[ignore = "requires headless Firefox; set FF_RDP_LIVE_TESTS=1"]
fn live_145_click_element_not_found_unchanged() {
    if !live_tests_enabled() {
        eprintln!("live_145_click_element_not_found_unchanged: set FF_RDP_LIVE_TESTS=1 to run");
        return;
    }
    let Some(ff) = firefox_with_daemon("live_145_click_element_not_found_unchanged") else {
        return;
    };
    let port = ff.port();

    let Some(server) = start_iframe_fixture() else {
        eprintln!(
            "live_145_click_element_not_found_unchanged: could not bind fixture HTTP — skipping"
        );
        stop_daemon(port);
        return;
    };

    let nav = navigate(port, &server.base_url());
    if !nav.status.success() {
        eprintln!(
            "live_145_click_element_not_found_unchanged: navigate failed — {}",
            combined(&nav)
        );
        stop_daemon(port);
        return;
    }

    assert!(
        wait_for_live_targets(port),
        "daemon never reported live frame targets"
    );

    let started = std::time::Instant::now();
    let click = run(port, &["click", ".nonexistent-selector-xyz", "--no-wait"]);
    let elapsed = started.elapsed();
    stop_daemon(port);

    assert!(
        !click.status.success(),
        "click on a nonexistent (but syntactically valid) selector must fail: {}",
        combined(&click)
    );
    assert!(
        elapsed < std::time::Duration::from_secs(8),
        "must fail fast, not pay the auto-wait timeout: took {elapsed:?}"
    );
    let text = combined(&click);
    assert!(
        text.contains("matched in 0 of") && text.contains("frame(s) tried"),
        "error must name how many frames were tried, exactly as before iter-145: {text}"
    );
    assert!(
        text.contains("/frame"),
        "error must list the tried frame URLs, exactly as before iter-145: {text}"
    );

    eprintln!("live_145_click_element_not_found_unchanged: PASSED in {elapsed:?} — {text}");
}
