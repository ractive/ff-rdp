/// Live test for Theme C (iter-84): `navigate` with default `--wait both`
/// strategy completes without "no remaining budget" timeout errors.
///
/// Root cause: the events phase consumed the full timeout budget leaving 0ms
/// for the readystate fallback. Fixed by splitting the budget 70/30.
///
/// Self-launches headless Firefox on a random port and navigates to a local
/// HTTP fixture (rather than https://example.com) — a real HTTP round trip
/// still exercises the default `--wait both` budget-splitting code path
/// (unlike a `data:` URL, which resolves instantly and would not meaningfully
/// exercise the events/readystate budget split under test).
///
/// AC: live_navigate_default_fast — completes in ≤ timeout_ms with status:ok
use std::collections::HashMap;
use std::process::Command;
use std::time::Instant;

use crate::common::{
    FixtureRoute, FixtureServer, LiveFirefox, base_args, ff_rdp_bin, live_tests_enabled,
};

const FIXTURE_BODY: &str =
    "<!DOCTYPE html><html><head></head><body><p>navigate fast fixture</p></body></html>";

/// Start a fixture server serving `FIXTURE_BODY` at `/`.
fn spawn_html_server() -> Option<FixtureServer> {
    let mut routes = HashMap::new();
    routes.insert("/".to_owned(), FixtureRoute::html(FIXTURE_BODY));
    FixtureServer::start(routes)
}

/// Theme C: navigate with default `--wait both` strategy does not exhaust
/// its budget before the readystate fallback fires.
///
/// Self-launches headless Firefox on a random port.
/// Post-condition: exit 0 within 10 s; no "no remaining budget" in stderr.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_navigate_default_fast_no_budget_exhaustion() {
    if !live_tests_enabled() {
        eprintln!(
            "live_navigate_default_fast_no_budget_exhaustion: set FF_RDP_LIVE_TESTS=1 to run"
        );
        return;
    }

    let Some(ff) = LiveFirefox::headless_on_random_port() else {
        eprintln!(
            "live_navigate_default_fast_no_budget_exhaustion: Firefox not available — skipping"
        );
        return;
    };

    let Some(server) = spawn_html_server() else {
        eprintln!(
            "live_navigate_default_fast_no_budget_exhaustion: could not bind HTTP server — skipping"
        );
        return;
    };
    let url = server.base_url();

    let start = Instant::now();
    let mut args = base_args(ff.port());
    // Global --timeout must be placed before the subcommand.
    let out = Command::new(ff_rdp_bin())
        .args({
            args.push("--timeout".to_owned());
            args.push("8000".to_owned());
            args
        })
        .args(["navigate", &url])
        .output()
        .expect("ff-rdp navigate failed");

    let elapsed = start.elapsed().as_millis();
    let stderr = String::from_utf8_lossy(&out.stderr);

    assert!(out.status.success(), "navigate failed: {stderr}");
    assert!(
        !stderr.contains("no remaining budget"),
        "Theme C regression: 'no remaining budget' appeared in stderr: {stderr}"
    );
    assert!(
        elapsed < 10_000,
        "navigate took too long: {elapsed}ms (expected < 10000ms)"
    );
}

/// `--timeout` (global operation timeout, placed before the subcommand) is
/// honored by `navigate` and the command still completes successfully.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_navigate_global_timeout_flag_accepted() {
    if !live_tests_enabled() {
        eprintln!("live_navigate_global_timeout_flag_accepted: set FF_RDP_LIVE_TESTS=1 to run");
        return;
    }

    let Some(ff) = LiveFirefox::headless_on_random_port() else {
        eprintln!("live_navigate_global_timeout_flag_accepted: Firefox not available — skipping");
        return;
    };

    let Some(server) = spawn_html_server() else {
        eprintln!(
            "live_navigate_global_timeout_flag_accepted: could not bind HTTP server — skipping"
        );
        return;
    };
    let url = server.base_url();

    let mut args = base_args(ff.port());
    args.push("--timeout".to_owned());
    args.push("5000".to_owned());
    let out = Command::new(ff_rdp_bin())
        .args(args)
        .args(["navigate", &url])
        .output()
        .expect("ff-rdp navigate failed");

    assert!(
        out.status.success(),
        "navigate with --timeout failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}
