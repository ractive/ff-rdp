//! Live test for iter-61o — previously-deferred iter-61l N1 AC.
//!
//! Verifies that `ff-rdp network --detail --headers` returns entries with
//! `meta.source == "watcher"` and at least one entry has a non-empty
//! `headers.response` map containing `Content-Type` or `Server`.
//!
//! # Running
//!
//! Requires Firefox, network access (example.com), and the ff-rdp binary.
//! Gates on `FF_RDP_LIVE_NETWORK_TESTS=1` (because it makes a real network request).
//!
//!   FF_RDP_LIVE_TESTS=1 FF_RDP_LIVE_NETWORK_TESTS=1 \
//!     cargo test -p ff-rdp-cli --test live_network_headers -- --nocapture

#[path = "common/mod.rs"]
mod common;

use std::process::{Command, Output};

use common::{LiveFirefox, ff_rdp_bin};

fn parse_json(output: &Output) -> serde_json::Value {
    let s = String::from_utf8_lossy(&output.stdout);
    serde_json::from_str(s.trim()).unwrap_or_else(|e| {
        panic!(
            "stdout is not valid JSON: {e}\nstdout={s}\nstderr={}",
            String::from_utf8_lossy(&output.stderr)
        )
    })
}

/// `live_network_headers`:
/// Navigate to example.com with `--with-network`, then call
/// `ff-rdp network --detail --headers` and assert:
/// - `meta.source == "watcher"`
/// - At least one entry has a non-empty `headers.response` map
///   containing `Content-Type` or `Server`.
#[test]
#[ignore = "requires Firefox, network access, and FF_RDP_LIVE_NETWORK_TESTS=1"]
fn live_network_headers() {
    if std::env::var("FF_RDP_LIVE_NETWORK_TESTS").is_err() {
        eprintln!("live_network_headers: set FF_RDP_LIVE_NETWORK_TESTS=1 to run");
        return;
    }

    let Some(ff) = LiveFirefox::headless_on_random_port() else {
        eprintln!("live_network_headers: Firefox not available — skipping");
        return;
    };

    let daemon_args = || {
        vec![
            "--host".to_owned(),
            "127.0.0.1".to_owned(),
            "--port".to_owned(),
            ff.port().to_string(),
            "--timeout".to_owned(),
            "20000".to_owned(),
        ]
    };

    // Navigate to example.com with --with-network to capture watcher events.
    let nav = Command::new(ff_rdp_bin())
        .args(daemon_args())
        .args(["navigate", "https://example.com", "--with-network"])
        .output()
        .expect("navigate --with-network");

    if !nav.status.success() {
        eprintln!(
            "live_network_headers: navigate failed — {}",
            String::from_utf8_lossy(&nav.stderr)
        );
        let _ = Command::new(ff_rdp_bin())
            .args([
                "--host",
                "127.0.0.1",
                "--port",
                &ff.port().to_string(),
                "daemon",
                "stop",
            ])
            .output();
        return;
    }

    // Call `ff-rdp network --detail --headers` — should return enriched entries.
    let network = Command::new(ff_rdp_bin())
        .args(daemon_args())
        .args(["network", "--detail", "--headers", "--format", "json"])
        .output()
        .expect("network --detail --headers");

    // Clean up daemon before asserting.
    let _ = Command::new(ff_rdp_bin())
        .args([
            "--host",
            "127.0.0.1",
            "--port",
            &ff.port().to_string(),
            "daemon",
            "stop",
        ])
        .output();

    let net_json = parse_json(&network);

    // Assert meta.source == "watcher".
    let source = net_json["meta"]["source"].as_str().unwrap_or("");
    assert_eq!(
        source, "watcher",
        "meta.source must be 'watcher' when --detail --headers is used after navigate --with-network.\n\
         Got source={source:?}\nFull response: {net_json}"
    );

    // Find at least one entry with a non-empty response headers map containing
    // Content-Type or Server.
    let empty_vec: Vec<serde_json::Value> = Vec::new();
    let entries = net_json["results"].as_array().unwrap_or(&empty_vec);

    assert!(
        !entries.is_empty(),
        "network results must be non-empty after navigate --with-network.\n\
         Full response: {net_json}"
    );

    let has_content_type_or_server = entries.iter().any(|entry| {
        let Some(response_headers) = entry["headers"]["response"].as_object() else {
            return false;
        };
        if response_headers.is_empty() {
            return false;
        }
        response_headers.keys().any(|k| {
            let lower = k.to_lowercase();
            lower == "content-type" || lower == "server"
        })
    });

    assert!(
        has_content_type_or_server,
        "at least one entry must have a non-empty headers.response map \
         containing 'Content-Type' or 'Server'.\n\
         entries: {entries:?}"
    );

    let count = entries.len();
    eprintln!(
        "live_network_headers: PASSED — source={source}, {count} entries, \
         Content-Type/Server header found"
    );
}
