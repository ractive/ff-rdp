//! Live tests for iter-126 — canonical network JSON shape.
//!
//! `navigate --with-network` (and the standalone `network` detail view) used to
//! flip between a `{entries, …}` object on busy pages and a BARE ARRAY on quiet
//! ones, so `.results.network.entries` / `.results.network.total_requests` threw
//! `cannot index array` half the time. These tests assert the ONE canonical
//! object shape on every path: quiet page, busy page, `--all`, and the
//! standalone `network` detail envelope carrying summary fields.
//!
//! # Running
//!
//! Requires Firefox, network access (example.com + a busy page), and the
//! ff-rdp binary. Gates on `FF_RDP_LIVE_NETWORK_TESTS=1`.
//!
//!   FF_RDP_LIVE_NETWORK_TESTS=1 cargo test -p ff-rdp-cli --test live live_126 -- --nocapture

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

/// Assert the value at `.results.network` is the canonical object with all
/// entry-level and summary keys present. Returns the object for further checks.
fn assert_canonical_network(json: &serde_json::Value) {
    let network = &json["results"]["network"];
    assert!(
        network.is_object(),
        "results.network must be an object (never a bare array), got: {network}"
    );
    // Entry-level keys.
    assert!(
        network["entries"].is_array(),
        "results.network.entries must be an array — no `cannot index array`, got: {}",
        network["entries"]
    );
    assert!(network["shown"].is_u64(), "shown must be numeric");
    assert!(network["total"].is_u64(), "total must be numeric");
    assert!(network["truncated"].is_boolean(), "truncated must be bool");
    // Summary keys.
    assert!(
        network["total_requests"].is_u64(),
        "results.network.total_requests must be numeric, got: {}",
        network["total_requests"]
    );
    assert!(
        network["total_transfer_bytes"].is_number(),
        "total_transfer_bytes must be numeric"
    );
    assert!(network["by_cause_type"].is_object());
    assert!(network["slowest"].is_array());
}

/// The canonical key set on `.results.network`, used for key-by-key parity.
fn network_keys(json: &serde_json::Value) -> Vec<String> {
    let mut keys: Vec<String> = json["results"]["network"]
        .as_object()
        .expect("results.network is an object")
        .keys()
        .cloned()
        .collect();
    keys.sort();
    keys
}

/// `live_navigate_with_network_shape_quiet`: `navigate --with-network --jq` on a
/// quiet page (example.com, ≤20 requests) yields `.results.network.entries` of
/// type array and a numeric `.results.network.total_requests` — no bare array.
///
/// `live_navigate_with_network_shape_busy`: the same on a busy page (>20
/// requests) yields the identical key set, with `truncated == true`,
/// `shown == 20`, and `total_requests >= total`.
///
/// Both branches run inside one Firefox instance and the quiet/busy key sets
/// are asserted equal key-by-key.
#[test]
#[ignore = "requires Firefox, network access, and FF_RDP_LIVE_NETWORK_TESTS=1"]
fn live_navigate_with_network_shape_quiet_and_busy() {
    if std::env::var("FF_RDP_LIVE_NETWORK_TESTS").is_err() {
        eprintln!(
            "live_navigate_with_network_shape_quiet_and_busy: set FF_RDP_LIVE_NETWORK_TESTS=1 to run"
        );
        return;
    }

    let Some(ff) = LiveFirefox::headless_on_random_port() else {
        eprintln!(
            "live_navigate_with_network_shape_quiet_and_busy: Firefox not available — skipping"
        );
        return;
    };
    let port = ff.port();

    // --- Quiet page: example.com, ≤20 requests. ---
    let quiet = Command::new(ff_rdp_bin())
        .args(base_args(port))
        .args([
            "navigate",
            "https://example.com",
            "--with-network",
            "--jq",
            ".",
        ])
        .output()
        .expect("navigate quiet --with-network");
    if !quiet.status.success() {
        stop_daemon(port);
        eprintln!(
            "live_navigate_with_network_shape_quiet_and_busy: quiet navigate failed — {}",
            String::from_utf8_lossy(&quiet.stderr)
        );
        return;
    }
    let quiet_json = parse_json(&quiet);
    assert_canonical_network(&quiet_json);
    let quiet_net = &quiet_json["results"]["network"];
    assert!(
        quiet_net["total_requests"].as_u64().unwrap() <= 20,
        "example.com should be a quiet (≤20) page, got {}",
        quiet_net["total_requests"]
    );
    assert_eq!(
        quiet_net["truncated"], false,
        "quiet page must not truncate"
    );

    // --- Busy page: >20 requests. ---
    let busy = Command::new(ff_rdp_bin())
        .args(base_args(port))
        .args([
            "navigate",
            "https://en.wikipedia.org/wiki/Firefox",
            "--with-network",
            "--jq",
            ".",
        ])
        .output()
        .expect("navigate busy --with-network");
    stop_daemon(port);
    if !busy.status.success() {
        eprintln!(
            "live_navigate_with_network_shape_quiet_and_busy: busy navigate failed — {}",
            String::from_utf8_lossy(&busy.stderr)
        );
        return;
    }
    let busy_json = parse_json(&busy);
    assert_canonical_network(&busy_json);
    let busy_net = &busy_json["results"]["network"];
    let busy_total = busy_net["total"].as_u64().unwrap();
    let busy_total_requests = busy_net["total_requests"].as_u64().unwrap();
    assert!(
        busy_total_requests > 20,
        "wikipedia should be a busy (>20) page, got {busy_total_requests}"
    );
    assert_eq!(busy_net["truncated"], true, "busy page must truncate");
    assert_eq!(busy_net["shown"], 20, "busy page shows 20 by default");
    assert!(
        busy_total_requests >= busy_total,
        "total_requests ({busy_total_requests}) >= total ({busy_total})"
    );

    // Shape equality: the quiet and busy key sets must be identical.
    assert_eq!(
        network_keys(&quiet_json),
        network_keys(&busy_json),
        "quiet and busy network shapes must have the same key set"
    );

    eprintln!(
        "live_navigate_with_network_shape_quiet_and_busy: PASSED — quiet total_requests={}, busy total_requests={busy_total_requests}",
        quiet_net["total_requests"]
    );
}

/// `live_navigate_with_network_all_keeps_summary`: adding `--all` still returns
/// the object shape (full `entries`, summary fields intact) — never a bare array.
#[test]
#[ignore = "requires Firefox, network access, and FF_RDP_LIVE_NETWORK_TESTS=1"]
fn live_navigate_with_network_all_keeps_summary() {
    if std::env::var("FF_RDP_LIVE_NETWORK_TESTS").is_err() {
        eprintln!(
            "live_navigate_with_network_all_keeps_summary: set FF_RDP_LIVE_NETWORK_TESTS=1 to run"
        );
        return;
    }

    let Some(ff) = LiveFirefox::headless_on_random_port() else {
        eprintln!("live_navigate_with_network_all_keeps_summary: Firefox not available — skipping");
        return;
    };
    let port = ff.port();

    let out = Command::new(ff_rdp_bin())
        .args(base_args(port))
        .args([
            "navigate",
            "https://en.wikipedia.org/wiki/Firefox",
            "--with-network",
            "--all",
            "--jq",
            ".",
        ])
        .output()
        .expect("navigate --with-network --all");
    stop_daemon(port);
    if !out.status.success() {
        eprintln!(
            "live_navigate_with_network_all_keeps_summary: navigate failed — {}",
            String::from_utf8_lossy(&out.stderr)
        );
        return;
    }
    let json = parse_json(&out);
    assert_canonical_network(&json);
    let net = &json["results"]["network"];
    // --all expands entries to the full capture (shown == total), summary intact.
    assert_eq!(
        net["shown"].as_u64().unwrap(),
        net["total"].as_u64().unwrap(),
        "--all must show every entry (shown == total)"
    );
    assert_eq!(net["truncated"], false, "--all must not truncate");
    assert!(
        net["total_requests"].as_u64().unwrap() >= 1,
        "summary fields must remain present under --all"
    );
    eprintln!(
        "live_navigate_with_network_all_keeps_summary: PASSED — shown={} total_requests={}",
        net["shown"], net["total_requests"]
    );
}

/// `live_network_detail_carries_summary`: standalone `network --jq` returns the
/// summary fields (`total_requests`, `total_transfer_bytes`) alongside the entry
/// list on a page with captured traffic.
#[test]
#[ignore = "requires Firefox, network access, and FF_RDP_LIVE_NETWORK_TESTS=1"]
fn live_network_detail_carries_summary() {
    if std::env::var("FF_RDP_LIVE_NETWORK_TESTS").is_err() {
        eprintln!("live_network_detail_carries_summary: set FF_RDP_LIVE_NETWORK_TESTS=1 to run");
        return;
    }

    let Some(ff) = LiveFirefox::headless_on_random_port() else {
        eprintln!("live_network_detail_carries_summary: Firefox not available — skipping");
        return;
    };
    let port = ff.port();

    // Capture traffic first.
    let nav = Command::new(ff_rdp_bin())
        .args(base_args(port))
        .args(["navigate", "https://example.com", "--with-network"])
        .output()
        .expect("navigate --with-network");
    if !nav.status.success() {
        stop_daemon(port);
        eprintln!(
            "live_network_detail_carries_summary: navigate failed — {}",
            String::from_utf8_lossy(&nav.stderr)
        );
        return;
    }

    // Standalone `network --jq` forces detail mode; summary fields must ride along.
    let net = Command::new(ff_rdp_bin())
        .args(base_args(port))
        .args(["network", "--jq", "."])
        .output()
        .expect("network --jq");
    stop_daemon(port);
    if !net.status.success() {
        eprintln!(
            "live_network_detail_carries_summary: network failed — {}",
            String::from_utf8_lossy(&net.stderr)
        );
        return;
    }
    let json = parse_json(&net);
    // Detail mode: results is the entry array, summary fields at the envelope top.
    assert!(
        json["results"].is_array(),
        "network --jq detail results must be an array, got: {}",
        json["results"]
    );
    assert!(
        json["total_requests"].is_u64(),
        "detail envelope must carry total_requests, got: {json}"
    );
    assert!(
        json["total_transfer_bytes"].is_number(),
        "detail envelope must carry total_transfer_bytes"
    );
    assert!(json["by_cause_type"].is_object());
    assert!(json["slowest"].is_array());
    eprintln!(
        "live_network_detail_carries_summary: PASSED — total_requests={}",
        json["total_requests"]
    );
}
