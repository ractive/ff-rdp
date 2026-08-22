//! Live tests for iteration 159 — the daemon's network watcher.
//!
//! Between iter-137 and iter-159 the daemon buffered **zero** network events.
//! Every daemon-mode `ff-rdp network` therefore answered from the Performance
//! API, with `method`, `status`, `content_type` and `transfer_size` null on
//! every row, and nothing in the default output said so. Two mechanisms hid it:
//! the `store-events` workaround fed the broken path from the working one, and
//! the one test that would have caught it navigated with `--with-network`
//! first, which is exactly what triggers that workaround.
//!
//! **Every assertion here starts from a daemon buffer proven empty and reaches
//! the page through the daemon only.** A test that permits a preceding direct
//! `--with-network` call is measuring the workaround, not the watcher — that is
//! the whole reason this regression survived two releases.
//!
//! daemon-parity: these tests ARE the daemon-mode tests. `--no-daemon` appears
//! only as the reference leg of a same-page comparison
//! (`live_159_daemon_direct_watcher_parity`,
//! `live_159_frame_targets_survive_the_fix`).
//!
//! # Running
//!
//!   FF_RDP_LIVE_NETWORK_TESTS=1 cargo test -p ff-rdp-cli --test live live_159 -- --nocapture

use std::process::{Command, Output};
use std::time::Instant;

use crate::common::{LiveFirefox, ff_rdp_bin};

/// A page whose subresource set is large enough that a partial capture is
/// obviously distinguishable from a complete one.
const BUSY_PAGE: &str = "https://en.wikipedia.org/wiki/Firefox";

/// A genuinely cross-origin iframe fixture — the same shape iter-129/137 used,
/// so the frame-count comparison is made on identical content.
const CROSS_ORIGIN_FIXTURE: &str =
    r#"data:text/html,<h1>top</h1><iframe src="https://example.com"></iframe>"#;

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

fn direct_args(port: u16) -> Vec<String> {
    let mut args = daemon_args(port);
    args.push("--no-daemon".to_owned());
    args
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

fn run(args: Vec<String>) -> Output {
    Command::new(ff_rdp_bin())
        .args(args)
        .output()
        .expect("spawn ff-rdp")
}

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

/// Number of `network-event` entries the daemon is currently holding.
///
/// `daemon status` reports `buffer_sizes` as a map that omits types with zero
/// entries, so an absent key means zero.
fn buffered_network_events(port: u16) -> u64 {
    let mut args = daemon_args(port);
    args.extend(["daemon".to_owned(), "status".to_owned()]);
    let out = run(args);
    let json = parse_json(&out);
    json["results"]["buffer_sizes"]["network-event"]
        .as_u64()
        .unwrap_or(0)
}

/// True when a failing live command hit the iter-164 proxy-startup flake rather
/// than anything this iteration touches.
///
/// Under a contended sweep the daemon proxy sometimes fails to come up at all;
/// that is [[iteration-164-two-failures-the-158-sweep-uncovered]], not a
/// watcher-routing regression. Misattributing it costs a debugging session on
/// code the failure never reached.
fn is_proxy_startup_flake(out: &Output) -> bool {
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    text.contains("the proxy daemon did not start")
}

fn network_tests_enabled(test: &str) -> bool {
    if std::env::var("FF_RDP_LIVE_NETWORK_TESTS").is_err() {
        eprintln!("{test}: set FF_RDP_LIVE_NETWORK_TESTS=1 to run");
        return false;
    }
    true
}

// ---------------------------------------------------------------------------
// Theme B/C — the watcher actually delivers, from a provably empty buffer
// ---------------------------------------------------------------------------

/// `live_159_daemon_watcher_captures_plain_navigate` +
/// `live_159_watcher_result_is_uncontaminated`.
///
/// On a daemon whose `buffer_sizes` reports 0 network events beforehand, a
/// **plain** `navigate` (no `--with-network` anywhere in this test body)
/// followed by `network --source watcher --detail` must return >= 1 entry, of
/// which at least one carries a non-null `method` and a non-null `status`.
///
/// Measured baseline before the fix: 0 entries, and `network` (no `--source`)
/// reporting `meta.source: "performance-api"`.
#[test]
#[ignore = "requires Firefox, network access, and FF_RDP_LIVE_NETWORK_TESTS=1"]
fn live_159_daemon_watcher_captures_plain_navigate() {
    if !network_tests_enabled("live_159_daemon_watcher_captures_plain_navigate") {
        return;
    }
    let ff = LiveFirefox::headless_on_random_port();
    let port = ff.port();

    // Proof of an empty start: nothing has fed this buffer, and nothing in this
    // test can feed it except the daemon's own watcher.
    let before = buffered_network_events(port);
    assert_eq!(
        before, 0,
        "the daemon buffer must be proven empty before the navigate; got {before} \
         network events — a non-empty start would make a pass unattributable"
    );

    let mut nav = daemon_args(port);
    nav.extend(["navigate".to_owned(), BUSY_PAGE.to_owned()]);
    let nav_out = run(nav);
    if !nav_out.status.success() {
        stop_daemon(port);
        if is_proxy_startup_flake(&nav_out) {
            eprintln!(
                "live_159_daemon_watcher_captures_plain_navigate: skipped — iter-164 \
                 proxy-startup flake, not a watcher failure"
            );
            return;
        }
        panic!(
            "plain daemon navigate must succeed: {}",
            crate::common::output_note(&nav_out)
        );
    }

    let after = buffered_network_events(port);
    assert!(
        after > 0,
        "the daemon's own watcher must have buffered network events after a plain \
         navigate; buffer went {before} -> {after}"
    );

    let mut net = daemon_args(port);
    net.extend([
        "network".to_owned(),
        "--source".to_owned(),
        "watcher".to_owned(),
        "--detail".to_owned(),
    ]);
    let net_out = run(net);
    stop_daemon(port);
    assert!(
        net_out.status.success(),
        "network --source watcher must succeed: {}",
        crate::common::output_note(&net_out)
    );

    let json = parse_json(&net_out);
    let entries = json["results"]
        .as_array()
        .expect("results must be an array");
    assert!(
        !entries.is_empty(),
        "expected >= 1 watcher entry after a plain navigate, got 0 (pre-fix baseline)"
    );
    assert_eq!(json["meta"]["source"], "watcher");

    let full = entries
        .iter()
        .filter(|e| !e["method"].is_null() && !e["status"].is_null())
        .count();
    assert!(
        full >= 1,
        "expected >= 1 entry with non-null method AND status; got {full} of {} entries: {}",
        entries.len(),
        serde_json::to_string(&entries[0]).unwrap_or_default()
    );

    eprintln!(
        "live_159_daemon_watcher_captures_plain_navigate: PASSED — buffer {before} -> {after}, \
         {} entries, {full} with method+status",
        entries.len()
    );
}

/// `live_159_watcher_result_is_uncontaminated`: the buffer-empty precondition,
/// asserted on its own so the property has a test of its own name.
///
/// `store-events` used to let a `--no-daemon --with-network` call push its
/// direct capture into the daemon buffer, so any daemon-mode assertion made
/// after one was measuring the workaround rather than the watcher. There is no
/// `--no-daemon` and no `--with-network` anywhere in this test body: the buffer
/// is asserted **== 0** before the navigate and **> 0** after, so the events can
/// only have come from the daemon's own watcher.
#[test]
#[ignore = "requires Firefox, network access, and FF_RDP_LIVE_NETWORK_TESTS=1"]
fn live_159_watcher_result_is_uncontaminated() {
    if !network_tests_enabled("live_159_watcher_result_is_uncontaminated") {
        return;
    }
    let ff = LiveFirefox::headless_on_random_port();
    let port = ff.port();

    let before = buffered_network_events(port);
    assert_eq!(
        before, 0,
        "nothing may have fed the buffer before the navigate; got {before}"
    );

    let mut nav = daemon_args(port);
    nav.extend(["navigate".to_owned(), BUSY_PAGE.to_owned()]);
    let nav_out = run(nav);
    if !nav_out.status.success() {
        stop_daemon(port);
        if is_proxy_startup_flake(&nav_out) {
            eprintln!("live_159_watcher_result_is_uncontaminated: skipped — iter-164 flake");
            return;
        }
        panic!(
            "plain daemon navigate must succeed: {}",
            crate::common::output_note(&nav_out)
        );
    }

    let after = buffered_network_events(port);
    stop_daemon(port);
    assert!(
        after > 0,
        "the daemon's own watcher must be the thing that filled the buffer; \
         went {before} -> {after}"
    );

    eprintln!(
        "live_159_watcher_result_is_uncontaminated: PASSED — buffer {before} -> {after}, \
         no direct call in this test body"
    );
}

/// `live_159_network_default_source_is_watcher`: with the auto-fallback
/// deleted, daemon-mode `network` with **no** `--source` reports
/// `meta.source == "watcher"` and returns entries with a non-null `method`,
/// while `--source performance-api` still returns Performance-API rows — the
/// explicit opt-out survives the deletion.
#[test]
#[ignore = "requires Firefox, network access, and FF_RDP_LIVE_NETWORK_TESTS=1"]
fn live_159_network_default_source_is_watcher() {
    if !network_tests_enabled("live_159_network_default_source_is_watcher") {
        return;
    }
    let ff = LiveFirefox::headless_on_random_port();
    let port = ff.port();

    assert_eq!(buffered_network_events(port), 0, "buffer must start empty");

    let mut nav = daemon_args(port);
    nav.extend(["navigate".to_owned(), BUSY_PAGE.to_owned()]);
    let nav_out = run(nav);
    if !nav_out.status.success() {
        stop_daemon(port);
        if is_proxy_startup_flake(&nav_out) {
            eprintln!("live_159_network_default_source_is_watcher: skipped — iter-164 flake");
            return;
        }
        panic!(
            "plain daemon navigate must succeed: {}",
            crate::common::output_note(&nav_out)
        );
    }

    let mut net = daemon_args(port);
    net.extend(["network".to_owned(), "--detail".to_owned()]);
    let net_out = run(net);
    assert!(net_out.status.success(), "network must succeed");
    let json = parse_json(&net_out);
    assert_eq!(
        json["meta"]["source"], "watcher",
        "the default source must be the watcher, never a silent substitute"
    );
    let entries = json["results"].as_array().expect("results array");
    assert!(
        entries.iter().any(|e| !e["method"].is_null()),
        "default-source rows must carry a method — the field the Performance API cannot supply"
    );

    // The explicit opt-out still works. It needs its own navigate because the
    // read above drained the buffer.
    let mut nav2 = daemon_args(port);
    nav2.extend(["navigate".to_owned(), BUSY_PAGE.to_owned()]);
    let _ = run(nav2);
    let mut perf = daemon_args(port);
    perf.extend([
        "network".to_owned(),
        "--detail".to_owned(),
        "--source".to_owned(),
        "performance-api".to_owned(),
    ]);
    let perf_out = run(perf);
    stop_daemon(port);
    assert!(
        perf_out.status.success(),
        "--source performance-api must remain supported"
    );
    let perf_json = parse_json(&perf_out);
    assert_eq!(perf_json["meta"]["source"], "performance-api");

    eprintln!(
        "live_159_network_default_source_is_watcher: PASSED — {} default rows, opt-out intact",
        entries.len()
    );
}

/// `live_159_daemon_direct_watcher_parity`: the same page on the same Firefox
/// instance must yield `meta.source == "watcher"` and a non-zero entry count in
/// **both** daemon mode and `--no-daemon` mode.
///
/// The daemon leg reads its own watcher buffer after a plain navigate; the
/// direct leg has no buffer, so its comparable capture is
/// `navigate --with-network --no-daemon`, which subscribes for the duration of
/// the navigation. Measured baseline before the fix: 0 daemon entries vs 10
/// direct entries.
#[test]
#[ignore = "requires Firefox, network access, and FF_RDP_LIVE_NETWORK_TESTS=1"]
fn live_159_daemon_direct_watcher_parity() {
    if !network_tests_enabled("live_159_daemon_direct_watcher_parity") {
        return;
    }
    let ff = LiveFirefox::headless_on_random_port();
    let port = ff.port();

    assert_eq!(buffered_network_events(port), 0, "buffer must start empty");

    // Daemon leg first, on a cold HTTP cache.
    let mut nav = daemon_args(port);
    nav.extend(["navigate".to_owned(), BUSY_PAGE.to_owned()]);
    let nav_out = run(nav);
    if !nav_out.status.success() {
        stop_daemon(port);
        if is_proxy_startup_flake(&nav_out) {
            eprintln!("live_159_daemon_direct_watcher_parity: skipped — iter-164 flake");
            return;
        }
        panic!(
            "plain daemon navigate must succeed: {}",
            crate::common::output_note(&nav_out)
        );
    }
    let mut net = daemon_args(port);
    net.extend([
        "network".to_owned(),
        "--source".to_owned(),
        "watcher".to_owned(),
        "--detail".to_owned(),
    ]);
    let daemon_out = run(net);
    assert!(daemon_out.status.success(), "daemon network must succeed");
    let daemon_json = parse_json(&daemon_out);
    assert_eq!(daemon_json["meta"]["source"], "watcher");
    let daemon_count = daemon_json["results"]
        .as_array()
        .expect("results array")
        .len();

    // Direct leg: same page, same Firefox instance.
    let mut dnav = direct_args(port);
    dnav.extend([
        "navigate".to_owned(),
        BUSY_PAGE.to_owned(),
        "--with-network".to_owned(),
    ]);
    let direct_out = run(dnav);
    stop_daemon(port);
    assert!(
        direct_out.status.success(),
        "direct navigate --with-network must succeed: {}",
        crate::common::output_note(&direct_out)
    );
    let direct_json = parse_json(&direct_out);
    let network = &direct_json["results"]["network"];
    let direct_count = network["entries"].as_array().map_or(0, std::vec::Vec::len);
    let all_watcher = network["entries"]
        .as_array()
        .is_some_and(|e| e.iter().all(|x| x["source"] == "watcher"));

    assert!(
        daemon_count > 0,
        "daemon-mode watcher count must be non-zero (pre-fix baseline: 0)"
    );
    assert!(
        direct_count > 0,
        "direct-mode watcher count must be non-zero"
    );
    assert!(
        all_watcher,
        "every direct-mode entry must report source=watcher"
    );

    eprintln!(
        "live_159_daemon_direct_watcher_parity: PASSED — daemon {daemon_count} entries \
         (source=watcher) vs direct {direct_count} entries (source=watcher)"
    );
}

// ---------------------------------------------------------------------------
// Theme B — iter-137's guarantee must survive
// ---------------------------------------------------------------------------

/// `live_159_frame_targets_survive_the_fix`: `click 'body' --frame <name>`
/// through the daemon on a multi-frame page reports the same non-zero frame
/// count as the same command with `--no-daemon`, and
/// `daemon status --jq '.results.live_target_count'` is > 0.
///
/// iter-137 set `isServerTargetSwitchingEnabled: true` on the daemon's core
/// watcher to make frame-target enumeration work through the proxy. iter-159's
/// wire evidence says that flag is not what broke network delivery, so it
/// stays — and this test is what makes "it stays" checkable rather than
/// asserted.
#[test]
#[ignore = "requires Firefox, network access, and FF_RDP_LIVE_NETWORK_TESTS=1"]
fn live_159_frame_targets_survive_the_fix() {
    if !network_tests_enabled("live_159_frame_targets_survive_the_fix") {
        return;
    }
    let ff = LiveFirefox::headless_on_random_port();
    let port = ff.port();

    let mut nav = daemon_args(port);
    nav.extend([
        "navigate".to_owned(),
        CROSS_ORIGIN_FIXTURE.to_owned(),
        "--allow-unsafe-urls".to_owned(),
    ]);
    let nav_out = run(nav);
    if !nav_out.status.success() {
        stop_daemon(port);
        if is_proxy_startup_flake(&nav_out) {
            eprintln!("live_159_frame_targets_survive_the_fix: skipped — iter-164 flake");
            return;
        }
        panic!(
            "daemon navigate to the frame fixture must succeed: {}",
            crate::common::output_note(&nav_out)
        );
    }

    // A deliberately non-existent frame name: the error envelope names how many
    // frames the enumeration actually found, which is the number under test.
    let frame_count = |args: Vec<String>| -> usize {
        let mut a = args;
        a.extend([
            "click".to_owned(),
            "body".to_owned(),
            "--frame".to_owned(),
            "no-such-frame-159".to_owned(),
        ]);
        let out = run(a);
        let text = format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        // "N frame(s) available" — the enumeration's own count.
        text.split_whitespace()
            .zip(text.split_whitespace().skip(1))
            .find(|(_, next)| next.starts_with("frame(s)"))
            .and_then(|(n, _)| n.trim_matches(|c: char| !c.is_ascii_digit()).parse().ok())
            .unwrap_or(0)
    };

    let via_daemon = frame_count(daemon_args(port));
    let direct = frame_count(direct_args(port));

    let mut status = daemon_args(port);
    status.extend(["daemon".to_owned(), "status".to_owned()]);
    let status_json = parse_json(&run(status));
    let live_targets = status_json["results"]["live_target_count"]
        .as_u64()
        .unwrap_or(0);
    stop_daemon(port);

    assert!(
        direct > 0,
        "the direct leg must find frames on the fixture; got {direct}"
    );
    assert_eq!(
        via_daemon, direct,
        "frame enumeration through the daemon must match --no-daemon — iter-137's \
         guarantee; got daemon {via_daemon} vs direct {direct}"
    );
    assert!(
        live_targets > 0,
        "daemon live_target_count must be > 0; got {live_targets}"
    );

    eprintln!(
        "live_159_frame_targets_survive_the_fix: PASSED — {via_daemon} frames both modes, \
         live_target_count={live_targets}"
    );
}

// ---------------------------------------------------------------------------
// Theme D — the two command-family fixes
// ---------------------------------------------------------------------------

/// `live_159_with_network_returns_on_idle`: `navigate <quiet-page>
/// --with-network --timeout 30000` returns in under 60% of the stated timeout
/// once the resource stream goes idle, and still returns >= 1 network entry.
///
/// `drain_network_events_timed` used to loop until `start.elapsed() >=
/// total_timeout` with no early exit, so this call took the full 30 s on a page
/// that finished in under a second.
const IDLE_TEST_TIMEOUT_MS: u128 = 30_000;

#[test]
#[ignore = "requires Firefox, network access, and FF_RDP_LIVE_NETWORK_TESTS=1"]
fn live_159_with_network_returns_on_idle() {
    if !network_tests_enabled("live_159_with_network_returns_on_idle") {
        return;
    }
    let ff = LiveFirefox::headless_on_random_port();
    let port = ff.port();

    let mut args = direct_args(port);
    args.extend([
        "navigate".to_owned(),
        "https://example.com".to_owned(),
        "--with-network".to_owned(),
        "--network-timeout".to_owned(),
        IDLE_TEST_TIMEOUT_MS.to_string(),
    ]);

    let start = Instant::now();
    let out = run(args);
    let elapsed = start.elapsed().as_millis();
    stop_daemon(port);

    assert!(
        out.status.success(),
        "navigate --with-network must succeed: {}",
        crate::common::output_note(&out)
    );
    let json = parse_json(&out);
    let entries = json["results"]["network"]["entries"]
        .as_array()
        .map_or(0, std::vec::Vec::len);
    assert!(entries >= 1, "expected >= 1 network entry, got {entries}");

    let budget = IDLE_TEST_TIMEOUT_MS * 60 / 100;
    assert!(
        elapsed < budget,
        "the drain must stop on idle, not on the wall clock: took {elapsed} ms of a \
         {IDLE_TEST_TIMEOUT_MS} ms timeout (budget {budget} ms)"
    );

    eprintln!(
        "live_159_with_network_returns_on_idle: PASSED — {elapsed} ms of {IDLE_TEST_TIMEOUT_MS} ms, \
         {entries} entries"
    );
}

/// `live_159_with_network_and_auto_consent_together`: `navigate
/// <consent-walled-url> --with-network --auto-consent` is accepted by the
/// argument parser (exit code is not clap's 2) and a single invocation returns
/// both a non-null `results.consent.cmp` and >= 1 network entry.
///
/// The two flags were `conflicts_with` at the clap level, so on a
/// consent-walled site — the only kind where both matter — you could dismiss
/// the banner or capture the network, never both in one call.
#[test]
#[ignore = "requires Firefox, network access, and FF_RDP_LIVE_NETWORK_TESTS=1"]
fn live_159_with_network_and_auto_consent_together() {
    if !network_tests_enabled("live_159_with_network_and_auto_consent_together") {
        return;
    }
    let ff = LiveFirefox::headless_on_random_port();
    let port = ff.port();

    let mut args = daemon_args(port);
    args.extend([
        "navigate".to_owned(),
        "https://www.theguardian.com".to_owned(),
        "--with-network".to_owned(),
        "--auto-consent".to_owned(),
    ]);
    let out = run(args);
    stop_daemon(port);

    assert_ne!(
        out.status.code(),
        Some(2),
        "clap must accept --with-network together with --auto-consent; {}",
        crate::common::output_note(&out)
    );
    assert!(
        out.status.success(),
        "the combined invocation must succeed: {}",
        crate::common::output_note(&out)
    );

    let json = parse_json(&out);
    let cmp = json["results"]["consent"]["cmp"].clone();
    let entries = json["results"]["network"]["entries"]
        .as_array()
        .map_or(0, std::vec::Vec::len);

    assert!(
        !cmp.is_null(),
        "expected a detected CMP on a consent-walled page; got results.consent = {}",
        json["results"]["consent"]
    );
    assert!(
        entries >= 1,
        "the same invocation must also return network entries; got {entries}"
    );

    eprintln!(
        "live_159_with_network_and_auto_consent_together: PASSED — cmp={cmp}, {entries} entries"
    );
}
