//! Live tests for iter-128 — network output fidelity: watcher parity on the
//! `--detail`/`--jq` path, `--format text` URL readability, and `meta.route`
//! self-identification.
//!
//! # Running
//!
//! Requires Firefox, network access, and the ff-rdp binary. Gates on
//! `FF_RDP_LIVE_NETWORK_TESTS=1` (network tests) / `FF_RDP_LIVE_TESTS=1`
//! (meta.route, which needs no external network access).
//!
//!   FF_RDP_LIVE_NETWORK_TESTS=1 cargo test -p ff-rdp-cli --test live live_128 -- --nocapture

use std::process::{Command, Output};

use crate::common::{LiveFirefox, ff_rdp_bin};

fn parse_json(output: &Output) -> serde_json::Value {
    let s = String::from_utf8_lossy(&output.stdout);
    serde_json::from_str(s.trim()).unwrap_or_else(|e| {
        panic!(
            "stdout is not valid JSON: {e}\nstdout={s}\nstderr={}",
            String::from_utf8_lossy(&output.stderr)
        )
    })
}

fn base_args(port: u16) -> Vec<String> {
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

/// `live_128_network_detail_uses_watcher`: after a real `navigate
/// --with-network` populates the daemon's watcher buffer, a single
/// subsequent `network --detail --jq` call must read from that SAME
/// buffer (`source: "watcher"` on every entry) rather than silently
/// falling back to the lower-fidelity Performance API — and at least one
/// entry must carry a non-null `method` and `content_type` (iter-128
/// Theme B).
#[test]
#[ignore = "requires Firefox, network access, and FF_RDP_LIVE_NETWORK_TESTS=1"]
fn live_128_network_detail_uses_watcher() {
    if std::env::var("FF_RDP_LIVE_NETWORK_TESTS").is_err() {
        eprintln!("live_128_network_detail_uses_watcher: set FF_RDP_LIVE_NETWORK_TESTS=1 to run");
        return;
    }

    let Some(ff) = LiveFirefox::headless_on_random_port() else {
        eprintln!("live_128_network_detail_uses_watcher: Firefox not available — skipping");
        return;
    };
    let port = ff.port();

    let nav = Command::new(ff_rdp_bin())
        .args(base_args(port))
        .args([
            "navigate",
            "https://en.wikipedia.org/wiki/Firefox",
            "--with-network",
        ])
        .output()
        .expect("navigate --with-network");
    if !nav.status.success() {
        stop_daemon(port);
        eprintln!(
            "live_128_network_detail_uses_watcher: navigate failed — {}",
            String::from_utf8_lossy(&nav.stderr)
        );
        return;
    }

    let detail = Command::new(ff_rdp_bin())
        .args(base_args(port))
        .args(["network", "--detail", "--jq", "."])
        .output()
        .expect("network --detail --jq");
    stop_daemon(port);
    if !detail.status.success() {
        eprintln!(
            "live_128_network_detail_uses_watcher: network --detail failed — {}",
            String::from_utf8_lossy(&detail.stderr)
        );
        return;
    }
    let json = parse_json(&detail);
    let entries = json["results"]
        .as_array()
        .expect("results.entries must be an array");
    assert!(
        !entries.is_empty(),
        "expected buffered watcher events after navigate --with-network, got 0 entries"
    );

    let sources: Vec<&str> = entries
        .iter()
        .map(|e| e["source"].as_str().unwrap_or("?"))
        .collect();
    assert!(
        sources.iter().all(|&s| s == "watcher"),
        "every entry must report source=\"watcher\" (not a performance-api fallback) \
         after navigate --with-network already populated the daemon buffer; got sources: \
         {sources:?}"
    );

    let has_method = entries.iter().any(|e| !e["method"].is_null());
    assert!(has_method, "expected >=1 entry with non-null method");
    let has_content_type = entries.iter().any(|e| !e["content_type"].is_null());
    assert!(
        has_content_type,
        "expected >=1 entry with non-null content_type (iter-128 Theme B backfill)"
    );

    eprintln!(
        "live_128_network_detail_uses_watcher: PASSED — {} entries, all source=watcher",
        entries.len()
    );
}

/// `live_128_network_text_width`: `network --format text` and `sources
/// --format text` on a page with very long (CMP/tracking) URLs must never
/// emit a line wider than 120 columns — the middle-ellipsis helper
/// (iter-128 Theme C) keeps `url` columns bounded.
#[test]
#[ignore = "requires Firefox, network access, and FF_RDP_LIVE_NETWORK_TESTS=1"]
fn live_128_network_text_width() {
    if std::env::var("FF_RDP_LIVE_NETWORK_TESTS").is_err() {
        eprintln!("live_128_network_text_width: set FF_RDP_LIVE_NETWORK_TESTS=1 to run");
        return;
    }

    let Some(ff) = LiveFirefox::headless_on_random_port() else {
        eprintln!("live_128_network_text_width: Firefox not available — skipping");
        return;
    };
    let port = ff.port();

    // theguardian.com's Sourcepoint CMP fires ~900-char tracking URLs
    // (dogfooding-session-62 #2) — a reliable real-world repro for
    // "url column explodes the table width".
    let nav = Command::new(ff_rdp_bin())
        .args(base_args(port))
        .args(["navigate", "https://www.theguardian.com", "--with-network"])
        .output()
        .expect("navigate --with-network");
    if !nav.status.success() {
        stop_daemon(port);
        eprintln!(
            "live_128_network_text_width: navigate failed — {}",
            String::from_utf8_lossy(&nav.stderr)
        );
        return;
    }

    // NOT --detail: the plan's dogfood_path exercises the default summary
    // renderer (`render_network_summary_text_to`'s "Slowest Requests" list),
    // which is narrow enough (url + 3 numbers) to fit a 120-col budget once
    // the url is ellipsized. `--detail`'s full ~10-column table is a
    // different renderer (`render_table`, shared with `sources`) that also
    // ellipsizes the url column but — with that many columns — is not
    // expected to fit 120 columns even so; it is not part of this AC.
    let net_text = Command::new(ff_rdp_bin())
        .args(base_args(port))
        .args(["network", "--format", "text"])
        .output()
        .expect("network --format text");
    if !net_text.status.success() {
        stop_daemon(port);
        eprintln!(
            "live_128_network_text_width: network --format text failed — {}",
            String::from_utf8_lossy(&net_text.stderr)
        );
        return;
    }
    let net_stdout = String::from_utf8_lossy(&net_text.stdout);
    let has_long_source_url = net_stdout
        .lines()
        .any(|l| l.chars().count() > 200 || l.contains("sourcepoint") || l.contains("sp_"));
    for line in net_stdout.lines() {
        assert!(
            line.chars().count() <= 120,
            "network --format text line exceeds 120 columns ({} chars): {line:?}",
            line.chars().count()
        );
    }

    let sources_text = Command::new(ff_rdp_bin())
        .args(base_args(port))
        .args(["sources", "--format", "text"])
        .output()
        .expect("sources --format text");
    stop_daemon(port);
    if sources_text.status.success() {
        let sources_stdout = String::from_utf8_lossy(&sources_text.stdout);
        for line in sources_stdout.lines() {
            assert!(
                line.chars().count() <= 120,
                "sources --format text line exceeds 120 columns ({} chars): {line:?}",
                line.chars().count()
            );
        }
    } else {
        eprintln!(
            "live_128_network_text_width: sources --format text failed (non-fatal) — {}",
            String::from_utf8_lossy(&sources_text.stderr)
        );
    }

    eprintln!(
        "live_128_network_text_width: PASSED — all lines <=120 cols (saw a long source url: {has_long_source_url})"
    );
}

/// `live_128_meta_route`: a daemon-routed command reports `meta.route ==
/// "daemon"`; the SAME command with `--no-daemon` reports `"direct"`
/// (iter-128 Theme D). Uses only example.com (no CMP/consent dependency),
/// so this is gated on `FF_RDP_LIVE_TESTS=1` rather than the network tier.
#[test]
#[ignore = "requires Firefox and FF_RDP_LIVE_TESTS=1"]
fn live_128_meta_route() {
    if std::env::var("FF_RDP_LIVE_TESTS").is_err() {
        eprintln!("live_128_meta_route: set FF_RDP_LIVE_TESTS=1 to run");
        return;
    }

    let Some(ff) = LiveFirefox::headless_on_random_port() else {
        eprintln!("live_128_meta_route: Firefox not available — skipping");
        return;
    };
    let port = ff.port();

    // Direct FIRST: --no-daemon bypasses the daemon proxy entirely, and
    // doing this before any daemon exists sidesteps `daemon stop`'s
    // process-group reap (iter-95 Theme A) — which can take the directly
    // launched Firefox process down with it, so "stop the daemon, then
    // reconnect direct to the SAME Firefox" is not a reliable pattern (see
    // live_daemon_stop_mdn.rs for the same hazard documented head-on).
    let direct = Command::new(ff_rdp_bin())
        .args(base_args(port))
        .args(["--no-daemon", "--verbose", "network", "--jq", "."])
        .output()
        .expect("network --no-daemon (direct)");
    if !direct.status.success() {
        eprintln!(
            "live_128_meta_route: direct network failed — stdout={} stderr={}",
            String::from_utf8_lossy(&direct.stdout),
            String::from_utf8_lossy(&direct.stderr)
        );
        return;
    }
    let direct_json = parse_json(&direct);
    assert_eq!(
        direct_json["meta"]["route"], "direct",
        "--no-daemon command must report meta.route == \"direct\", got: {direct_json}"
    );

    // Daemon-routed SECOND: no --no-daemon, so connect_and_get_target
    // resolves (and auto-starts) the daemon proxy against the same Firefox.
    let daemon_routed = Command::new(ff_rdp_bin())
        .args(base_args(port))
        .args(["--verbose", "network", "--jq", "."])
        .output()
        .expect("network (daemon-routed)");
    stop_daemon(port);
    if !daemon_routed.status.success() {
        eprintln!(
            "live_128_meta_route: daemon-routed network failed — stdout={} stderr={}",
            String::from_utf8_lossy(&daemon_routed.stdout),
            String::from_utf8_lossy(&daemon_routed.stderr)
        );
        return;
    }
    let daemon_json = parse_json(&daemon_routed);
    assert_eq!(
        daemon_json["meta"]["route"], "daemon",
        "daemon-routed command must report meta.route == \"daemon\", got: {daemon_json}"
    );

    eprintln!("live_128_meta_route: PASSED — --no-daemon=\"direct\", daemon=\"daemon\"");
}
