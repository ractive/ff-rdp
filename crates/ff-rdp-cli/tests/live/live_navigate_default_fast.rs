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

/// iter-122 Theme A — `live_navigate_default_fast`
///
/// A default `ff-rdp navigate` (implicit `--wait-strategy both`) to a simple
/// static page must return in wall-clock `< timeout/2`, proving the interleaved
/// readystate probe short-circuits instead of burning the full events budget
/// waiting for a `dom-complete` that may never fire on FF152.
///
/// Post-condition: wall-clock elapsed < timeout_ms / 2 (< 4000ms for an 8s
/// timeout), exit 0.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_navigate_default_fast() {
    const TIMEOUT_MS: u64 = 8000;

    if !live_tests_enabled() {
        eprintln!("live_navigate_default_fast: set FF_RDP_LIVE_TESTS=1 to run");
        return;
    }

    let Some(ff) = LiveFirefox::headless_on_random_port() else {
        eprintln!("live_navigate_default_fast: Firefox not available — skipping");
        return;
    };
    let Some(server) = spawn_html_server() else {
        eprintln!("live_navigate_default_fast: could not bind HTTP server — skipping");
        return;
    };
    let url = server.base_url();

    let start = Instant::now();
    let mut args = base_args(ff.port());
    args.push("--timeout".to_owned());
    args.push(TIMEOUT_MS.to_string());
    let out = Command::new(ff_rdp_bin())
        .args(args)
        .args(["navigate", &url])
        .output()
        .expect("ff-rdp navigate failed");
    let elapsed_ms = u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX);

    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "navigate failed: {stderr}");
    assert!(
        elapsed_ms < TIMEOUT_MS / 2,
        "default navigate must return in < timeout/2 ({}ms); took {elapsed_ms}ms — \
         the events-budget burn (iter-122) has regressed. stderr: {stderr}",
        TIMEOUT_MS / 2
    );
}

/// iter-122 Theme B — `live_navigate_elapsed_matches_wall`
///
/// `result.elapsed_ms` must reflect total wall-clock across both phases, not
/// just the readystate-poll duration (which was ~1ms). Assert it lands within
/// ±750ms of the externally-measured wall-clock for a default navigate.
///
/// Post-condition: `|results.elapsed_ms − measured_wall_ms| ≤ 750`.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_navigate_elapsed_matches_wall() {
    if !live_tests_enabled() {
        eprintln!("live_navigate_elapsed_matches_wall: set FF_RDP_LIVE_TESTS=1 to run");
        return;
    }

    let Some(ff) = LiveFirefox::headless_on_random_port() else {
        eprintln!("live_navigate_elapsed_matches_wall: Firefox not available — skipping");
        return;
    };
    let Some(server) = spawn_html_server() else {
        eprintln!("live_navigate_elapsed_matches_wall: could not bind HTTP server — skipping");
        return;
    };
    let url = server.base_url();

    let start = Instant::now();
    let mut args = base_args(ff.port());
    args.push("--timeout".to_owned());
    args.push("8000".to_owned());
    let out = Command::new(ff_rdp_bin())
        .args(args)
        .args(["navigate", &url])
        .output()
        .expect("ff-rdp navigate failed");
    let measured_wall_ms = i128::try_from(start.elapsed().as_millis()).unwrap_or(i128::MAX);

    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "navigate failed: {stderr}");

    let stdout = String::from_utf8_lossy(&out.stdout);
    let json: serde_json::Value = serde_json::from_str(stdout.trim())
        .unwrap_or_else(|e| panic!("navigate stdout is not JSON: {e}\nstdout: {stdout}"));
    let elapsed_ms = json["results"]["elapsed_ms"].as_i64().map_or_else(
        || panic!("results.elapsed_ms missing/non-int: {json}"),
        i128::from,
    );

    // The CLI's reported elapsed is measured from navigate dispatch, so it is
    // strictly ≤ the externally-measured wall time (which also includes connect
    // + teardown). Assert it is within ±750ms and never absurdly small.
    let delta = (measured_wall_ms - elapsed_ms).abs();
    assert!(
        delta <= 750,
        "elapsed_ms ({elapsed_ms}) must be within ±750ms of measured wall ({measured_wall_ms}); \
         delta {delta}ms — honest-timing fix (iter-122 Theme B) regressed"
    );
    assert!(
        elapsed_ms > 5,
        "elapsed_ms ({elapsed_ms}) is implausibly small — it is reporting only the \
         readystate-poll duration, the pre-iter-122 bug"
    );
}

/// iter-122 Theme B — `live_navigate_spa_committed_url`
///
/// Navigating to a page must yield a real `committed_url` equal to the landed
/// `location.href`, never `"about:blank"` (or an empty string). This guards the
/// SPA regression where a missing `dom-loading` URL left `committed_url` empty.
///
/// Post-condition: `results.committed_url` starts with the fixture base URL and
/// is neither empty nor `"about:blank"`.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_navigate_spa_committed_url() {
    if !live_tests_enabled() {
        eprintln!("live_navigate_spa_committed_url: set FF_RDP_LIVE_TESTS=1 to run");
        return;
    }

    let Some(ff) = LiveFirefox::headless_on_random_port() else {
        eprintln!("live_navigate_spa_committed_url: Firefox not available — skipping");
        return;
    };
    let Some(server) = spawn_html_server() else {
        eprintln!("live_navigate_spa_committed_url: could not bind HTTP server — skipping");
        return;
    };
    let url = server.base_url();

    let mut args = base_args(ff.port());
    args.push("--timeout".to_owned());
    args.push("8000".to_owned());
    let out = Command::new(ff_rdp_bin())
        .args(args)
        .args(["navigate", &url])
        .output()
        .expect("ff-rdp navigate failed");

    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "navigate failed: {stderr}");

    let stdout = String::from_utf8_lossy(&out.stdout);
    let json: serde_json::Value = serde_json::from_str(stdout.trim())
        .unwrap_or_else(|e| panic!("navigate stdout is not JSON: {e}\nstdout: {stdout}"));
    let committed_url = json["results"]["committed_url"]
        .as_str()
        .unwrap_or_else(|| panic!("results.committed_url missing/non-string: {json}"));

    assert_ne!(
        committed_url, "about:blank",
        "committed_url must be the real landed URL, not about:blank (iter-122 Theme B)"
    );
    assert!(
        !committed_url.is_empty(),
        "committed_url must not be empty (iter-122 Theme B): {json}"
    );
    assert!(
        committed_url.starts_with(&url),
        "committed_url ({committed_url}) must be the fixture URL ({url})"
    );
}
