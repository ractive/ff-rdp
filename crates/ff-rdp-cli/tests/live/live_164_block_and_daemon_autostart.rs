//! Live tests for iter-164 — the two defects iter-158's dual-gate `live-sweep`
//! uncovered.
//!
//! See `kb/iterations/iteration-164-two-failures-the-158-sweep-uncovered.md`.
//!
//! ## Defect 1 — `throttle --block <pattern>` did not block
//!
//! `throttle --block favicon` was accepted, echoed back in `results.blocked_urls`,
//! and then silently discarded by the very next `navigate`: navigate's
//! `ResourceCommand` teardown sends `unwatchResources(["document-event",
//! "network-event"])`, and through the daemon that landed on the **shared**
//! connection, destroying Firefox's `NetworkObserver` — which owns the URL
//! block-list. `live_164_block_url_pattern_rejects` reproduces the exact
//! block → navigate → probe order that failed.
//!
//! ## Defect 2 — daemon autostart gave up under load
//!
//! At load average 18.6 a freshly spawned daemon did not write its registry
//! entry inside the old hard-coded 5 s, so the caller silently fell back to a
//! direct connection. `live_164_daemon_autostart_survives_load` drives eight
//! concurrent launch + `eval` pairs and requires all eight to end up on the
//! daemon.
//!
//! # Running
//!
//!   FF_RDP_LIVE_TESTS=1 FF_RDP_LIVE_NETWORK_TESTS=1 \
//!       cargo test -p ff-rdp-cli --test live live_164 -- --include-ignored --nocapture

use std::process::Command;

use serde_json::Value;

use crate::common::{LiveFirefox, ff_rdp_bin, live_network_tests_enabled, live_tests_enabled};

/// Daemon-path args (no `--no-daemon`), so consecutive commands share the
/// persistent daemon connection.
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

fn stop_daemon(port: u16) {
    let _ = Command::new(ff_rdp_bin())
        .args(["--host", "127.0.0.1", "--port", &port.to_string()])
        .args(["daemon", "stop"])
        .output();
}

fn run_json(port: u16, extra: &[&str]) -> Value {
    let out = Command::new(ff_rdp_bin())
        .args(daemon_args(port))
        .args(extra)
        .output()
        .unwrap_or_else(|e| panic!("spawn ff-rdp {extra:?}: {e}"));
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

/// Probe from inside the page: does a `fetch` of `url` resolve or reject?
///
/// This is the strongest cross-version observation of blocking available — the
/// netmonitor's blocked-flag field name has varied across Firefox versions, so
/// reading the flag would test our decoding, not Firefox's enforcement.
fn fetch_probe(port: u16, url: &str) -> String {
    let expr = format!(
        "(async () => {{ try {{ \
           await fetch('{url}' + (('{url}'.includes('?')) ? '&' : '?') + 'x=' + Date.now(), \
                       {{ cache: 'no-store' }}); \
           return 'resolved'; \
         }} catch (e) {{ return 'rejected'; }} }})()"
    );
    run_json(port, &["eval", &expr])["results"]
        .as_str()
        .unwrap_or_else(|| panic!("fetch probe for {url} did not return a string"))
        .to_owned()
}

/// AC `live_164_block_url_pattern_rejects`.
///
/// After `throttle --block favicon` **and a subsequent `navigate`**, an in-page
/// `fetch` of a matching URL rejects while an un-blocked URL still resolves.
///
/// The `navigate` between the block and the probe is the whole point: without
/// it the block-list survives and the bug is invisible. That is precisely why
/// the defect was only ever reproducible through `live_109`'s ordering.
#[test]
#[ignore = "requires Firefox + network access — set FF_RDP_LIVE_TESTS=1 FF_RDP_LIVE_NETWORK_TESTS=1"]
fn live_164_block_url_pattern_rejects() {
    if !live_tests_enabled() || !live_network_tests_enabled() {
        eprintln!(
            "live_164_block_url_pattern_rejects: set FF_RDP_LIVE_TESTS=1 and \
             FF_RDP_LIVE_NETWORK_TESTS=1"
        );
        return;
    }
    let ff = LiveFirefox::headless_on_random_port();
    assert!(
        ff.with_daemon().is_some(),
        "live_164_block_url_pattern_rejects: the proxy daemon did not start for \
         Firefox on port {}",
        ff.port()
    );
    let port = ff.port();

    let applied = run_json(port, &["throttle", "--block", "favicon"]);
    assert_eq!(
        applied["results"]["blocked_urls"],
        serde_json::json!(["favicon"]),
        "intake: the envelope must echo the active block-list: {applied}"
    );

    // The step that used to destroy the block-list.
    run_json(port, &["navigate", "https://example.com"]);

    assert_eq!(
        fetch_probe(port, "https://example.com/favicon.ico"),
        "rejected",
        "enforcement: a blocked URL must still be blocked after a navigate — \
         this is iteration-164 defect 1"
    );
    assert_eq!(
        fetch_probe(port, "https://example.com/"),
        "resolved",
        "an un-blocked URL must still resolve (the block must be selective, \
         not a blanket network kill)"
    );

    // A second navigate must not erode it either.
    run_json(port, &["navigate", "https://example.com"]);
    assert_eq!(
        fetch_probe(port, "https://example.com/favicon.ico"),
        "rejected",
        "the block-list must survive repeated navigation, not just the first"
    );

    let unblocked = run_json(port, &["throttle", "--unblock"]);
    assert_eq!(
        unblocked["results"]["blocked_urls"],
        serde_json::json!([]),
        "throttle --unblock must echo an empty block-list: {unblocked}"
    );
    // A 404 still *resolves* the fetch promise; only a network-level block
    // rejects it.
    assert_eq!(
        fetch_probe(port, "https://example.com/favicon.ico"),
        "resolved",
        "after --unblock the previously-blocked URL must fetch without a network abort"
    );

    stop_daemon(port);
}

/// AC `live_164_daemon_autostart_survives_load`.
///
/// Eight concurrent `launch` + `eval` pairs, then `daemon status` on each: all
/// eight must report `running == true`. Pre-iter-164 the autostart handshake
/// gave up after a hard-coded 5 s wait for the registry entry and the caller
/// silently fell back to a direct connection.
///
/// Concurrency is the point — eight Firefoxes plus eight daemons starting at
/// once is what generates the contention. No network access is needed (the
/// `eval` is `1`), so this runs under `FF_RDP_LIVE_TESTS=1` alone.
#[test]
#[ignore = "requires Firefox — set FF_RDP_LIVE_TESTS=1"]
fn live_164_daemon_autostart_survives_load() {
    const INSTANCES: usize = 8;

    if !live_tests_enabled() {
        eprintln!("live_164_daemon_autostart_survives_load: set FF_RDP_LIVE_TESTS=1");
        return;
    }

    // Launch all eight in parallel, each on its own random port. `LiveFirefox`
    // panics on a launch failure, so a failure here is reported as a failure —
    // never as a skip.
    let handles: Vec<_> = (0..INSTANCES)
        .map(|i| {
            std::thread::Builder::new()
                .name(format!("live_164_daemon_autostart_survives_load/{i}"))
                .spawn(|| {
                    let ff = LiveFirefox::headless_on_random_port();
                    let port = ff.port();
                    // Triggers autostart through `resolve_connection_target`.
                    let started = ff.with_daemon();
                    // Independent confirmation via the product's own reporting
                    // surface, not the harness's return value.
                    let status = Command::new(ff_rdp_bin())
                        .args(["--host", "127.0.0.1", "--port", &port.to_string()])
                        .args(["daemon", "status"])
                        .output()
                        .expect("spawn ff-rdp daemon status");
                    let status_json: Value =
                        serde_json::from_slice(&status.stdout).unwrap_or(Value::Null);
                    let running = status_json["results"]["running"].as_bool();
                    // Keep Firefox alive until after the status read, then let
                    // `Drop` reap it.
                    drop(ff);
                    (port, started.is_some(), running)
                })
                .expect("spawn autostart worker thread")
        })
        .collect();

    let mut failures = Vec::new();
    let mut ports = Vec::new();
    for h in handles {
        let (port, harness_saw_daemon, running) = h.join().expect("autostart worker thread");
        ports.push(port);
        if running != Some(true) {
            failures.push(format!(
                "port {port}: daemon status running={running:?} \
                 (harness with_daemon saw daemon={harness_saw_daemon})"
            ));
        }
    }

    for port in ports {
        stop_daemon(port);
    }

    assert!(
        failures.is_empty(),
        "{} of {INSTANCES} concurrent autostarts did not end up on the daemon — \
         this is iteration-164 defect 2:\n  {}",
        failures.len(),
        failures.join("\n  ")
    );
}
