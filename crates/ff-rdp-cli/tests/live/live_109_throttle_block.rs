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
//!     takes measurably longer than baseline. The assertion is *additive*, not
//!     a ratio — see iteration 177 and the comment on the test itself.
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

use ff_rdp_core::ThrottleProfile;
use serde_json::Value;

use crate::common::{LiveFirefox, ff_rdp_bin, live_network_tests_enabled, live_tests_enabled};

/// Samples taken on each side of the throttled/baseline comparison.
///
/// Odd, so the median is a real sample. Five is enough for the median to
/// survive the connection-reuse regime: roughly one fetch in five completes
/// ~260 ms faster than its neighbours because it rides an already-open
/// connection, and that regime shift is present *both* throttled and
/// un-throttled (iteration 177 measured it on both sides).
const FETCH_SAMPLES: usize = 5;

/// Fraction of the profile's declared round-trip latency that the throttled
/// fetch must actually pay, over and above baseline.
///
/// Iteration 177 measured the real delta at 400–420 ms against slow-3g's
/// declared 400 ms, idle and under load, so half of the declared figure is a
/// floor with ~100% headroom rather than the ~2% the old ratio assertion had.
const MIN_LATENCY_FRACTION: f64 = 0.5;

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

/// Take [`FETCH_SAMPLES`] timed fetches of `url` and return them **sorted**
/// together with their median.
///
/// The median — not the minimum — is the estimator: iteration 177 measured a
/// bimodal distribution (fresh connection vs. reused connection, ~260 ms
/// apart) on both the throttled and the un-throttled side, and a `min` over
/// two samples picks whichever mode it happens to hit. Comparing a
/// fresh-connection baseline against a reused-connection throttled sample is
/// what actually produced the reds this test was seeing.
fn fetch_ms_samples(port: u16, url: &str) -> (Vec<f64>, f64) {
    let mut samples: Vec<f64> = (0..FETCH_SAMPLES)
        .map(|_| time_fetch_ms(port, url))
        .collect();
    samples.sort_by(f64::total_cmp);
    let median = samples[FETCH_SAMPLES / 2];
    (samples, median)
}

/// Render timing samples for the failure message: `368,371,374,459,801`.
fn fmt_ms(samples: &[f64]) -> String {
    samples
        .iter()
        .map(|ms| format!("{ms:.0}"))
        .collect::<Vec<_>>()
        .join(",")
}

/// `live_throttle_slow3g_slows_fetch`:
///
/// A timed in-page fetch under `throttle slow-3g` pays at least half of the
/// profile's declared round-trip latency on top of the un-throttled baseline.
/// `throttle off` restores full speed.
#[test]
#[ignore = "requires Firefox + network access — set FF_RDP_LIVE_TESTS=1 FF_RDP_LIVE_NETWORK_TESTS=1"]
fn live_throttle_slow3g_slows_fetch() {
    if !live_tests_enabled() || !live_network_tests_enabled() {
        eprintln!(
            "live_throttle_slow3g_slows_fetch: set FF_RDP_LIVE_TESTS=1 and FF_RDP_LIVE_NETWORK_TESTS=1"
        );
        return;
    }
    let ff = LiveFirefox::headless_on_random_port();
    // iter-158 Theme D discipline (applied here by iter-164): a `return` is
    // reported `ok` by libtest, which is how the enforcement gap this test
    // guards stayed invisible for so long.
    assert!(
        ff.with_daemon().is_some(),
        "live_throttle_slow3g_slows_fetch: the proxy daemon did not start for Firefox on port {}",
        ff.port()
    );
    let port = ff.port();

    // Navigate to a real same-origin page so in-page fetch() has a document
    // context (data: URLs cannot issue cross-origin fetches and are not
    // subject to network throttling).
    run_json(port, &["navigate", "https://example.com"]);

    // Fetch a resource served with permissive CORS so the body is readable.
    // example.com's own document is same-origin here.
    let target = "https://example.com/";

    // Baseline: median of FETCH_SAMPLES samples (see `fetch_ms_samples`).
    let (base_samples, baseline) = fetch_ms_samples(port, target);
    let base_list = fmt_ms(&base_samples);
    eprintln!("baseline fetch: [{base_list}] → median {baseline:.0}ms");

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

    // Throttled: same estimator on the same target.
    let (thr_samples, throttled) = fetch_ms_samples(port, target);
    let thr_list = fmt_ms(&thr_samples);
    eprintln!("throttled fetch: [{thr_list}] → median {throttled:.0}ms");

    // iter-177: assert the *additive* cost of the profile's declared
    // round-trip latency, not a ratio.
    //
    // The old assertion was `throttled >= baseline * 2.0`, which multiplies
    // every baseline error by two: on the machine where this was diagnosed the
    // idle ratio was 2.05, so a baseline 3% slower than usual reddened the test
    // while the throttled figure had not moved at all. Throttling is additive —
    // slow-3g injects a fixed round-trip latency — so the delta is what is
    // actually reproducible, and a baseline error costs it 1:1 instead of 2:1.
    let declared_latency_ms = ThrottleProfile::Slow3g.latency_ms() as f64;
    let required_delta = declared_latency_ms * MIN_LATENCY_FRACTION;
    let delta = throttled - baseline;
    assert!(
        delta >= required_delta,
        "under slow-3g the fetch must pay at least {required_delta:.0}ms more than baseline \
         (half of the profile's declared {declared_latency_ms:.0}ms round-trip latency), \
         but paid {delta:.0}ms: baseline median={baseline:.0}ms [{base_list}] \
         throttled median={throttled:.0}ms [{thr_list}]"
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
    let ff = LiveFirefox::headless_on_random_port();
    // iter-158 Theme D discipline (applied here by iter-164): a `return` is
    // reported `ok` by libtest, which is how the enforcement gap this test
    // guards stayed invisible for so long.
    assert!(
        ff.with_daemon().is_some(),
        "live_block_url_pattern: the proxy daemon did not start for Firefox on port {}",
        ff.port()
    );
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
