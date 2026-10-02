//! Live tests for iter-109 — network throttling and URL blocking
//! (network-parent actor), as `navigate --throttle` / `navigate --block`.
//!
//! Throttling and blocking live only for the RDP connection that set them, so
//! the set + observe pair must run inside ONE command: `navigate` applies them
//! on its own connection right before `navigateTo` and the load it waits for
//! (and, with `--with-network`, captures) is the one they govern.
//!
//! ACs:
//!   - live_navigate_block_url_pattern: a request matching `--block` never
//!     reports a 2xx status in the same command's `--with-network` capture,
//!     while an unblocked sub-resource does; the envelope echoes the list.
//!   - live_navigate_throttle_slow3g_slows_load: a `--throttle slow-3g` load
//!     takes measurably longer than baseline — additive, not a ratio (see
//!     iteration 177).
//!
//! # Running
//!
//!   FF_RDP_LIVE_TESTS=1 FF_RDP_LIVE_NETWORK_TESTS=1 \
//!       cargo test -p ff-rdp-cli --test live live_109 -- --include-ignored --nocapture

use std::collections::HashMap;
use std::process::{Command, Output};

use ff_rdp_core::ThrottleProfile;
use serde_json::Value;

use crate::common::{
    FixtureRoute, FixtureServer, LiveFirefox, ff_rdp_bin, live_network_tests_enabled,
    live_tests_enabled,
};

/// Samples taken on each side of the throttled/baseline comparison. Odd, so
/// the median is a real sample.
const LOAD_SAMPLES: usize = 3;

/// Fraction of the profile's declared round-trip latency the throttled load
/// must pay over baseline. A page load pays the latency at least once (the
/// document request), so half of it is a floor with headroom.
const MIN_LATENCY_FRACTION: f64 = 0.5;

fn run(port: u16, extra: &[&str]) -> Output {
    Command::new(ff_rdp_bin())
        .args([
            "--host",
            "127.0.0.1",
            "--port",
            &port.to_string(),
            "--timeout",
            "30000",
        ])
        .args(extra)
        .output()
        .expect("ff-rdp command")
}

/// Run `ff-rdp <args...>` and return the parsed JSON stdout, asserting success.
fn run_json(port: u16, extra: &[&str]) -> Value {
    let out = run(port, extra);
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

/// `live_navigate_block_url_pattern`: `--block` governs the load of the same
/// command, and is refused with `--no-wait` (it would end before the load).
#[test]
#[ignore = "requires Firefox — set FF_RDP_LIVE_TESTS=1"]
fn live_navigate_block_url_pattern() {
    if !live_tests_enabled() {
        eprintln!("live_navigate_block_url_pattern: set FF_RDP_LIVE_TESTS=1");
        return;
    }
    let mut routes = HashMap::new();
    routes.insert(
        "/".to_owned(),
        FixtureRoute::html(
            "<!doctype html><title>t109</title><body>\
             <img src=\"/blocked.png\"><script src=\"/ok.js\"></script></body>",
        ),
    );
    routes.insert(
        "/ok.js".to_owned(),
        FixtureRoute {
            content_type: "text/javascript",
            body: b"window.__t109 = 1;".to_vec(),
            extra_headers: Vec::new(),
            delay: std::time::Duration::ZERO,
        },
    );
    routes.insert(
        "/blocked.png".to_owned(),
        FixtureRoute {
            content_type: "image/png",
            body: Vec::new(),
            extra_headers: Vec::new(),
            delay: std::time::Duration::ZERO,
        },
    );
    let Some(server) = FixtureServer::start(routes) else {
        eprintln!("live_navigate_block_url_pattern: no fixture HTTP — skipping");
        return;
    };
    let ff = LiveFirefox::headless_on_random_port();
    let port = ff.port();
    let url = server.base_url();

    let nav = run_json(
        port,
        &[
            "navigate",
            &url,
            "--with-network",
            "--network-timeout",
            "3000",
            "--block",
            "blocked.png",
        ],
    );
    assert_eq!(
        nav["results"]["network_conditions"]["blocked_urls"],
        serde_json::json!(["blocked.png"]),
        "the envelope must echo the applied block-list: {nav}"
    );
    let entries = nav["results"]["network"]["entries"]
        .as_array()
        .unwrap_or_else(|| panic!("--with-network must report entries: {nav}"));
    let status_of = |needle: &str| {
        entries
            .iter()
            .filter(|e| e["url"].as_str().is_some_and(|u| u.contains(needle)))
            .map(|e| e["status"].as_u64())
            .collect::<Vec<_>>()
    };
    assert!(
        status_of("ok.js").contains(&Some(200)),
        "an unblocked sub-resource must still load with 200: {entries:?}"
    );
    assert!(
        !status_of("blocked.png")
            .iter()
            .any(|s| s.is_some_and(|code| (200..300).contains(&code))),
        "a request matching --block must never report a 2xx status: {entries:?}"
    );

    let no_wait = run(port, &["navigate", &url, "--no-wait", "--block", "x"]);
    assert!(
        !no_wait.status.success(),
        "--block with --no-wait must be refused: stdout={}",
        String::from_utf8_lossy(&no_wait.stdout)
    );
}

/// The median `elapsed_ms` of [`LOAD_SAMPLES`] navigations to `url`.
fn median_load_ms(port: u16, url: &str, extra: &[&str]) -> (Vec<f64>, f64) {
    let mut samples: Vec<f64> = (0..LOAD_SAMPLES)
        .map(|i| {
            let busted = format!("{url}?t109={i}_{}", extra.len());
            let mut args = vec!["navigate", busted.as_str()];
            args.extend_from_slice(extra);
            let nav = run_json(port, &args);
            nav["results"]["elapsed_ms"]
                .as_f64()
                .unwrap_or_else(|| panic!("navigate must report elapsed_ms: {nav}"))
        })
        .collect();
    samples.sort_by(f64::total_cmp);
    let median = samples[LOAD_SAMPLES / 2];
    (samples, median)
}

/// `live_navigate_throttle_slow3g_slows_load`: a `--throttle slow-3g` load pays
/// at least half of the profile's declared round-trip latency over baseline.
#[test]
#[ignore = "requires Firefox + network access — set FF_RDP_LIVE_TESTS=1 FF_RDP_LIVE_NETWORK_TESTS=1"]
fn live_navigate_throttle_slow3g_slows_load() {
    if !live_tests_enabled() || !live_network_tests_enabled() {
        eprintln!(
            "live_navigate_throttle_slow3g_slows_load: set FF_RDP_LIVE_TESTS=1 and FF_RDP_LIVE_NETWORK_TESTS=1"
        );
        return;
    }
    let ff = LiveFirefox::headless_on_random_port();
    let port = ff.port();
    let target = "https://example.com/";

    let echo = run_json(port, &["navigate", target, "--throttle", "slow-3g"]);
    assert_eq!(
        echo["results"]["network_conditions"]["throttle"], "slow-3g",
        "the envelope must echo the applied profile: {echo}"
    );

    let (base, baseline) = median_load_ms(port, target, &[]);
    let (thr, throttled) = median_load_ms(port, target, &["--throttle", "slow-3g"]);
    let declared_latency_ms = f64::from(
        u32::try_from(ThrottleProfile::Slow3g.latency_ms()).expect("declared latency fits in u32"),
    );
    let required_delta = declared_latency_ms * MIN_LATENCY_FRACTION;
    let delta = throttled - baseline;
    assert!(
        delta >= required_delta,
        "under slow-3g the load must take at least {required_delta:.0}ms longer than baseline \
         (half of the declared {declared_latency_ms:.0}ms latency), but took {delta:.0}ms: \
         baseline={base:?} throttled={thr:?}"
    );
}
