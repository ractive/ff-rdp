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
//!     cargo test -p ff-rdp-cli --test live live_network_headers -- --nocapture

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

/// `live_network_headers`:
/// Run `ff-rdp network --detail --headers` while navigating to example.com
/// from a second process, and assert:
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

    let ff = LiveFirefox::headless_on_random_port();

    // A one-shot `network` only sees requests made while it is connected, so
    // start it first (it drains until the stream has been quiet for
    // `--timeout`) and navigate from a second process while it listens.
    let port = ff.port().to_string();
    let watcher = Command::new(ff_rdp_bin())
        .args(["--host", "127.0.0.1", "--port", &port, "--timeout", "8000"])
        .args(["network", "--detail", "--headers", "--format", "json"])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("spawn network --detail --headers");
    std::thread::sleep(std::time::Duration::from_millis(1500));

    let nav = Command::new(ff_rdp_bin())
        .args(["--host", "127.0.0.1", "--port", &port, "--timeout", "20000"])
        .args(["navigate", "https://example.com"])
        .output()
        .expect("navigate");
    let network = watcher
        .wait_with_output()
        .expect("network --detail --headers output");
    if !nav.status.success() {
        eprintln!(
            "live_network_headers: navigate failed — {}",
            String::from_utf8_lossy(&nav.stderr)
        );
        return;
    }

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

    // iter-110 Theme B(b): `headers.response` is an ARRAY of `{name, value}`
    // pairs (see commands::network — Firefox header order is preserved), not a
    // name→value object. The stale `.as_object()` read always returned None.
    let has_content_type_or_server = entries.iter().any(|entry| {
        let Some(response_headers) = entry["headers"]["response"].as_array() else {
            return false;
        };
        if response_headers.is_empty() {
            return false;
        }
        response_headers.iter().any(|h| {
            let lower = h["name"].as_str().unwrap_or_default().to_lowercase();
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
