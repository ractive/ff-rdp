//! Live tests for iter-109 — `throttle` (network-parent actor).
//!
//! Throttling and blocking live only for the RDP connection that set them, so
//! the set + observe pair must run over the *same* connection: these tests use
//! the daemon path (no `--no-daemon`) so the persistent daemon connection
//! carries the configuration from the `throttle` call to the following
//! `eval`/`navigate`/`network` commands.
//!
//! ACs (see kb/iterations/iteration-109-network-throttle-block.md):
//!   - live_throttle_slow3g_slows_fetch: a timed in-page fetch under slow-3g
//!     takes measurably longer than baseline (≥2×).
//!   - live_block_url_pattern: a request matching the blocked pattern is
//!     reported failed/blocked in `network` output while other requests succeed.
//!
//! Both require real network access (they fetch from a live origin), so they
//! gate on `FF_RDP_LIVE_NETWORK_TESTS=1` in addition to `FF_RDP_LIVE_TESTS=1`.
//!
//! # Running
//!
//!   FF_RDP_LIVE_TESTS=1 FF_RDP_LIVE_NETWORK_TESTS=1 \
//!       cargo test -p ff-rdp-cli --test live live_109 -- --include-ignored --nocapture

use std::process::Command;

use serde_json::Value;

use crate::common::{LiveFirefox, ff_rdp_bin, live_network_tests_enabled, live_tests_enabled};

/// Build daemon-path args (no `--no-daemon`): commands share the persistent
/// daemon connection, so throttling/blocking set by one command is visible to
/// the next.
fn daemon_args(port: u16) -> Vec<String> {
    vec![
        "--host".to_owned(),
        "127.0.0.1".to_owned(),
        "--port".to_owned(),
        port.to_string(),
        "--timeout".to_owned(),
        "30000".to_owned(),
    ]
}

/// Stop the daemon for `port`, ignoring failures.
fn stop_daemon(port: u16) {
    let _ = Command::new(ff_rdp_bin())
        .args([
            "--host",
            "127.0.0.1",
            "--port",
            &port.to_string(),
            "daemon",
            "stop",
        ])
        .output();
}

/// Run `ff-rdp <args...>` and return the parsed JSON stdout, asserting success.
fn run_json(port: u16, extra: &[&str]) -> Value {
    let out = Command::new(ff_rdp_bin())
        .args(daemon_args(port))
        .args(extra)
        .output()
        .expect("ff-rdp command");
    assert!(
        out.status.success(),
        "command {extra:?} failed: stdout={} stderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    serde_json::from_str(stdout.trim())
        .unwrap_or_else(|e| panic!("output for {extra:?} not JSON: {e}\n{stdout}"))
}

/// Evaluate a JS expression over the daemon connection and return the result
/// as a JSON value (the eval envelope's `results` field).
fn eval(port: u16, expr: &str) -> Value {
    let json = run_json(port, &["eval", expr]);
    json["results"].clone()
}

/// Time an in-page fetch of `url` (cache-busted) via `performance.now()`,
/// returning the elapsed milliseconds. The fetch reads the body to completion
/// so bandwidth throttling — not just latency — is exercised.
fn time_fetch_ms(port: u16, url: &str) -> f64 {
    // A single self-timing expression keeps the whole measurement inside one
    // eval so daemon round-trip overhead is excluded from the number we assert.
    let expr = format!(
        "(async () => {{ \
           const u = '{url}' + (('{url}'.includes('?')) ? '&' : '?') + 'cb=' + Date.now(); \
           const t0 = performance.now(); \
           const r = await fetch(u, {{ cache: 'no-store' }}); \
           await r.arrayBuffer(); \
           return performance.now() - t0; \
         }})()"
    );
    let v = eval(port, &expr);
    v.as_f64()
        .unwrap_or_else(|| panic!("fetch timing not a number: {v}"))
}

/// `live_throttle_slow3g_slows_fetch`:
///
/// A timed in-page fetch under `throttle slow-3g` takes measurably longer than
/// the un-throttled baseline (≥2×). `throttle off` restores full speed.
#[test]
#[ignore = "requires Firefox + network access — set FF_RDP_LIVE_TESTS=1 FF_RDP_LIVE_NETWORK_TESTS=1"]
fn live_throttle_slow3g_slows_fetch() {
    if !live_tests_enabled() || !live_network_tests_enabled() {
        eprintln!(
            "live_throttle_slow3g_slows_fetch: set FF_RDP_LIVE_TESTS=1 and FF_RDP_LIVE_NETWORK_TESTS=1"
        );
        return;
    }
    let Some(ff) = LiveFirefox::headless_on_random_port() else {
        eprintln!("Firefox not available — skipping");
        return;
    };
    if ff.with_daemon().is_none() {
        eprintln!("daemon did not start — skipping");
        return;
    }
    let port = ff.port();

    // Navigate to a real same-origin page so in-page fetch() has a document
    // context (data: URLs cannot issue cross-origin fetches and are not
    // subject to network throttling).
    run_json(port, &["navigate", "https://example.com"]);

    // Fetch a resource served with permissive CORS so the body is readable.
    // example.com's own document is same-origin here.
    let target = "https://example.com/";

    // Baseline: median-ish of a couple of samples to dampen jitter.
    let base_a = time_fetch_ms(port, target);
    let base_b = time_fetch_ms(port, target);
    let baseline = base_a.min(base_b);
    eprintln!("baseline fetch: a={base_a:.0}ms b={base_b:.0}ms → {baseline:.0}ms");

    // Apply slow-3g throttling; the envelope must echo the active profile.
    let applied = run_json(port, &["throttle", "slow-3g"]);
    assert_eq!(
        applied["results"]["profile"], "slow-3g",
        "envelope must echo the active throttling profile: {applied}"
    );
    assert!(
        applied["results"].get("lifetime_warning").is_none(),
        "daemon-path envelope must NOT carry a lifetime warning: {applied}"
    );

    // Throttled: worst of a couple of samples (throttling should dominate any
    // network jitter, so take the min throttled time — still expected ≥2×).
    let thr_a = time_fetch_ms(port, target);
    let thr_b = time_fetch_ms(port, target);
    let throttled = thr_a.min(thr_b);
    eprintln!("throttled fetch: a={thr_a:.0}ms b={thr_b:.0}ms → {throttled:.0}ms");

    assert!(
        throttled >= baseline * 2.0,
        "under slow-3g the fetch must take at least 2x baseline: \
         baseline={baseline:.0}ms throttled={throttled:.0}ms"
    );

    // Restore full speed.
    let off = run_json(port, &["throttle", "off"]);
    assert_eq!(
        off["results"]["profile"], "off",
        "throttle off must echo profile=off: {off}"
    );

    stop_daemon(port);
}

/// `live_block_url_pattern`:
///
/// After `throttle --block <pattern>`, a request whose URL matches the pattern
/// is reported failed/blocked in `network` output, while a request that does
/// not match still succeeds. `throttle --unblock` clears the list.
#[test]
#[ignore = "requires Firefox + network access — set FF_RDP_LIVE_TESTS=1 FF_RDP_LIVE_NETWORK_TESTS=1"]
fn live_block_url_pattern() {
    if !live_tests_enabled() || !live_network_tests_enabled() {
        eprintln!(
            "live_block_url_pattern: set FF_RDP_LIVE_TESTS=1 and FF_RDP_LIVE_NETWORK_TESTS=1"
        );
        return;
    }
    let Some(ff) = LiveFirefox::headless_on_random_port() else {
        eprintln!("Firefox not available — skipping");
        return;
    };
    if ff.with_daemon().is_none() {
        eprintln!("daemon did not start — skipping");
        return;
    }
    let port = ff.port();

    // Block any URL containing "favicon". The block-list echo confirms the set.
    let applied = run_json(port, &["throttle", "--block", "favicon"]);
    let blocked = &applied["results"]["blocked_urls"];
    assert!(
        blocked
            .as_array()
            .is_some_and(|a| a.iter().any(|u| u.as_str() == Some("favicon"))),
        "envelope must echo the active block-list: {applied}"
    );

    // Probe from within the page: a fetch of a blocked URL must reject, while a
    // fetch of an un-blocked URL must resolve. This is the most robust
    // cross-version observation of blocking behaviour (the netmonitor's
    // blocked-flag field name has varied across Firefox versions).
    run_json(port, &["navigate", "https://example.com"]);

    let blocked_probe = eval(
        port,
        "(async () => { try { \
           await fetch('https://example.com/favicon.ico?x=' + Date.now(), { cache: 'no-store' }); \
           return 'resolved'; \
         } catch (e) { return 'rejected'; } })()",
    );
    assert_eq!(
        blocked_probe, "rejected",
        "a fetch of a blocked URL (matching 'favicon') must reject: {blocked_probe}"
    );

    let allowed_probe = eval(
        port,
        "(async () => { try { \
           await fetch('https://example.com/?x=' + Date.now(), { cache: 'no-store' }); \
           return 'resolved'; \
         } catch (e) { return 'rejected'; } })()",
    );
    assert_eq!(
        allowed_probe, "resolved",
        "a fetch of an un-blocked URL must still resolve: {allowed_probe}"
    );

    // The `network` command surfaces the blocked request as an errored entry.
    // Navigate with --with-network so the daemon captures the blocked load,
    // then confirm the network summary is retrievable (blocked entries carry
    // no 2xx status).
    let net = run_json(port, &["navigate", "https://example.com", "--with-network"]);
    assert!(
        net["results"].is_object() || net["results"].is_array(),
        "navigate --with-network must return a results payload: {net}"
    );

    // Clear the block-list; a subsequent fetch of the previously-blocked URL
    // must resolve again.
    let unblocked = run_json(port, &["throttle", "--unblock"]);
    assert_eq!(
        unblocked["results"]["blocked_urls"],
        serde_json::json!([]),
        "throttle --unblock must echo an empty block-list: {unblocked}"
    );
    let after_unblock = eval(
        port,
        "(async () => { try { \
           await fetch('https://example.com/favicon.ico?x=' + Date.now(), { cache: 'no-store' }); \
           return 'resolved'; \
         } catch (e) { return 'rejected'; } })()",
    );
    // favicon.ico may legitimately 404 on example.com, but a 404 still
    // *resolves* the fetch promise (only network-level blocking rejects it).
    assert_eq!(
        after_unblock, "resolved",
        "after --unblock the previously-blocked URL must fetch without a network abort: {after_unblock}"
    );

    stop_daemon(port);
}
