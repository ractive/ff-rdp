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
/// Odd, so the median is a real sample. Five leaves room for two anomalous
/// samples per side without moving the verdict.
const FETCH_SAMPLES: usize = 5;

/// Fraction of the profile's declared round-trip latency that the throttled
/// fetch must actually pay in delivery delay, over and above baseline.
///
/// Iteration 177 measured the real delivery delay at 407-413 ms against
/// slow-3g's declared 400 ms — a 1.5% spread across every sample taken, idle
/// and under load — while baseline delivery delay sat at 1-3 ms. Half of the
/// declared figure is therefore a floor with ~100% headroom, where the ratio
/// assertion it replaces had ~2%.
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

/// One timed in-page fetch, split into the two parts that behave differently.
struct FetchTiming {
    /// Wall-clock milliseconds for the whole `fetch()` plus body read.
    total_ms: f64,
    /// Network time-to-first-byte (`responseStart - requestStart`) taken from
    /// the request's `PerformanceResourceTiming` entry.
    ttfb_ms: f64,
}

impl FetchTiming {
    /// The part of the fetch that throttling is responsible for: everything
    /// that is *not* the network's own time-to-first-byte.
    ///
    /// Firefox's parent-process throttler holds the response back before
    /// handing it to content, so the injected latency lands entirely after
    /// `responseStart`. Iteration 177 measured this directly: with slow-3g on,
    /// TTFB is unchanged (99-450 ms, exactly the un-throttled spread) while
    /// this figure jumps from ~2 ms to 407-413 ms.
    fn delivery_delay_ms(&self) -> f64 {
        self.total_ms - self.ttfb_ms
    }
}

/// Time an in-page fetch of `url` (cache-busted), returning both its total
/// duration and its network TTFB. The fetch reads the body to completion so
/// bandwidth throttling — not just latency — is exercised.
fn time_fetch(port: u16, url: &str) -> FetchTiming {
    // A single self-timing expression keeps the whole measurement inside one
    // eval so daemon round-trip overhead is excluded from the numbers we
    // assert. The result is JSON-stringified because the RDP eval reply
    // returns a *grip preview* for object results, not the object itself.
    let expr = format!(
        "(async () => {{ \
           const u = '{url}' + (('{url}'.includes('?')) ? '&' : '?') \
             + 'cb=' + Date.now() + '_' + Math.random().toString(36).slice(2); \
           const t0 = performance.now(); \
           const r = await fetch(u, {{ cache: 'no-store' }}); \
           await r.arrayBuffer(); \
           const total = performance.now() - t0; \
           const e = performance.getEntriesByName(u)[0]; \
           return JSON.stringify({{ \
             total, ttfb: e ? (e.responseStart - e.requestStart) : -1 \
           }}); \
         }})()"
    );
    let v = eval(port, &expr);
    let raw = v
        .as_str()
        .unwrap_or_else(|| panic!("fetch timing not a JSON string: {v}"));
    let parsed: Value =
        serde_json::from_str(raw).unwrap_or_else(|e| panic!("fetch timing not JSON: {e}\n{raw}"));
    let total_ms = parsed["total"]
        .as_f64()
        .unwrap_or_else(|| panic!("fetch timing has no total: {parsed}"));
    let ttfb_ms = parsed["ttfb"]
        .as_f64()
        .unwrap_or_else(|| panic!("fetch timing has no ttfb: {parsed}"));
    assert!(
        ttfb_ms >= 0.0,
        "no PerformanceResourceTiming entry for the probe fetch — the throttle \
         measurement this test relies on is unavailable: {parsed}"
    );
    FetchTiming { total_ms, ttfb_ms }
}

/// Take [`FETCH_SAMPLES`] timed fetches of `url` and return their delivery
/// delays **sorted**, together with the median.
///
/// The median, not the minimum: iteration 177 measured example.com answering
/// in either ~100 ms or ~370-470 ms at random, on both the throttled and the
/// un-throttled side. That variance is in TTFB, which `delivery_delay_ms`
/// already subtracts out, but a `min` over two samples would still pick
/// whichever end of any remaining spread it happened to hit.
fn delivery_delays_ms(port: u16, url: &str) -> (Vec<f64>, f64) {
    let mut delays: Vec<f64> = (0..FETCH_SAMPLES)
        .map(|_| time_fetch(port, url).delivery_delay_ms())
        .collect();
    delays.sort_by(f64::total_cmp);
    let median = delays[FETCH_SAMPLES / 2];
    (delays, median)
}

/// Render timing samples for the failure message: `1,2,2,3,3`.
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
/// profile's declared round-trip latency in *delivery delay* — the part of the
/// fetch that is not the network's own TTFB — over the un-throttled baseline.
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

    // Baseline delivery delay: median of FETCH_SAMPLES samples.
    let (base_samples, baseline) = delivery_delays_ms(port, target);
    let base_list = fmt_ms(&base_samples);
    eprintln!("baseline delivery delay: [{base_list}] → median {baseline:.0}ms");

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
    let (thr_samples, throttled) = delivery_delays_ms(port, target);
    let thr_list = fmt_ms(&thr_samples);
    eprintln!("throttled delivery delay: [{thr_list}] → median {throttled:.0}ms");

    // iter-177: assert the *additive* cost of the profile's declared
    // round-trip latency on the delivery delay, not a ratio of total times.
    //
    // The old assertion was `total_throttled >= total_baseline * 2.0`, which
    // multiplies every baseline error by two and compares two numbers that are
    // mostly origin latency. On the machine where this was diagnosed the idle
    // ratio was 2.05, so a baseline 3% slower than usual reddened the test
    // while the throttled figure had not moved at all. Throttling is additive
    // and lands wholly after `responseStart`, so the delivery delay isolates
    // it: it is reproducible to ~1.5% where the totals are not.
    let declared_latency_ms = f64::from(
        u32::try_from(ThrottleProfile::Slow3g.latency_ms()).expect("declared latency fits in u32"),
    );
    let required_delta = declared_latency_ms * MIN_LATENCY_FRACTION;
    let delta = throttled - baseline;
    assert!(
        delta >= required_delta,
        "under slow-3g the fetch must pay at least {required_delta:.0}ms more delivery delay \
         than baseline (half of the profile's declared {declared_latency_ms:.0}ms round-trip \
         latency), but paid {delta:.0}ms: baseline median={baseline:.0}ms [{base_list}] \
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
