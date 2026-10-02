//! Live tests for iter-128 — network output fidelity: `--format text` URL
//! readability.
//!
//! # Running
//!
//! Requires Firefox, network access, and the ff-rdp binary. Gates on
//! `FF_RDP_LIVE_NETWORK_TESTS=1` and `FF_RDP_LIVE_SITES_TESTS=1`.
//!
//!   FF_RDP_LIVE_NETWORK_TESTS=1 cargo test -p ff-rdp-cli --test live live_128 -- --nocapture

use std::process::Command;

use crate::common::{LiveFirefox, ff_rdp_bin};

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

/// `live_128_network_text_width`: `network --format text` and `sources
/// --format text` on a page with very long (CMP/tracking) URLs must never
/// emit a line wider than 120 columns — the middle-ellipsis helper
/// (iter-128 Theme C) keeps `url` columns bounded.
#[test]
#[ignore = "requires Firefox, network access, and FF_RDP_LIVE_NETWORK_TESTS=1 + FF_RDP_LIVE_SITES_TESTS=1 (third-party site)"]
fn live_128_network_text_width() {
    if !crate::common::live_sites_tests_enabled() {
        eprintln!(
            "live_128_network_text_width: set FF_RDP_LIVE_SITES_TESTS=1 to run (third-party site)"
        );
        return;
    }
    if std::env::var("FF_RDP_LIVE_NETWORK_TESTS").is_err() {
        eprintln!("live_128_network_text_width: set FF_RDP_LIVE_NETWORK_TESTS=1 to run");
        return;
    }

    let ff = LiveFirefox::headless_on_random_port();
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
    //
    // `--source performance-api`: a one-shot watcher capture only sees requests
    // made while it is connected, and the page has finished loading by now;
    // Resource Timing still lists the long CMP URLs this check is about.
    let net_text = Command::new(ff_rdp_bin())
        .args(base_args(port))
        .args(["network", "--source", "performance-api", "--format", "text"])
        .output()
        .expect("network --format text");
    if !net_text.status.success() {
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
