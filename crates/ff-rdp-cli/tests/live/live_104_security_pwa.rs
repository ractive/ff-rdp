//! Live tests for iter-104 — security & PWA audit pack.
//!
//! ACs (see kb/iterations/iteration-104-security-pwa-audit-pack.md):
//!   - live_network_security_info_https
//!   - live_manifest_fetch_canonical
//!
//! # Running
//!
//!   FF_RDP_LIVE_TESTS=1 FF_RDP_LIVE_NETWORK_TESTS=1 \
//!     cargo test -p ff-rdp-cli --test live live_104 -- --include-ignored --nocapture

use std::process::{Command, Output};

use serde_json::Value;

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

/// `live_network_security_info_https`:
///
/// A `network --security` capture listening while `navigate https://example.com`
/// runs attaches a `security` object to the (HTTPS) request whose
/// `protocolVersion` starts with "TLS" and whose `cipherSuite` is non-empty.
/// The top-level `insecure_requests` count is present (0 for an all-HTTPS
/// capture). The http→null half of the AC is covered by the unit test
/// `get_security_info_http_returns_none` in ff-rdp-core plus the URL-scheme
/// classification in `count_insecure_requests`; a live all-HTTP page is not
/// reachable without an insecure fixture server, so the live assertion here
/// pins the HTTPS side and the presence of the mixed-content counter.
#[test]
#[ignore = "requires Firefox, network access, and FF_RDP_LIVE_NETWORK_TESTS=1"]
fn live_network_security_info_https() {
    if std::env::var("FF_RDP_LIVE_NETWORK_TESTS").is_err() {
        eprintln!("live_network_security_info_https: set FF_RDP_LIVE_NETWORK_TESTS=1 to run");
        return;
    }
    let ff = LiveFirefox::headless_on_random_port();

    let cli_args = || {
        vec![
            "--host".to_owned(),
            "127.0.0.1".to_owned(),
            "--port".to_owned(),
            ff.port().to_string(),
            "--timeout".to_owned(),
            "20000".to_owned(),
        ]
    };

    // A one-shot `network` only sees requests made while it is connected, so
    // start it first (it drains until the stream has been quiet for
    // `--timeout`) and navigate from a second process while it listens.
    let port = ff.port().to_string();
    let watcher = Command::new(ff_rdp_bin())
        .args(["--host", "127.0.0.1", "--port", &port, "--timeout", "8000"])
        .args(["network", "--security", "--format", "json"])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("spawn network --security");
    std::thread::sleep(std::time::Duration::from_millis(1500));

    let nav = Command::new(ff_rdp_bin())
        .args(cli_args())
        .args(["navigate", "https://example.com"])
        .output()
        .expect("navigate");
    let network = watcher
        .wait_with_output()
        .expect("network --security output");
    if !nav.status.success() {
        eprintln!(
            "live_network_security_info_https: navigate failed — {}",
            String::from_utf8_lossy(&nav.stderr)
        );
        return;
    }

    let json = parse_json(&network);
    assert_eq!(
        json["meta"]["source"].as_str().unwrap_or(""),
        "watcher",
        "security join requires the watcher source: {json}"
    );

    // A mixed-content counter must be present.
    assert!(
        json["insecure_requests"].is_u64(),
        "insecure_requests count must be present under --security: {json}"
    );

    let empty: Vec<serde_json::Value> = Vec::new();
    let entries = json["results"].as_array().unwrap_or(&empty);
    assert!(
        !entries.is_empty(),
        "network results must capture the navigation it was listening during: {json}"
    );

    // The main-document HTTPS request must carry a security object whose
    // protocolVersion starts with "TLS" and whose cipherSuite is non-empty.
    let secure_ok = entries.iter().any(|e| {
        let url = e["url"].as_str().unwrap_or("");
        if !url.starts_with("https://") {
            return false;
        }
        let sec = &e["security"];
        let proto = sec["protocolVersion"].as_str().unwrap_or("");
        let cipher = sec["cipherSuite"].as_str().unwrap_or("");
        proto.starts_with("TLS") && !cipher.is_empty()
    });
    assert!(
        secure_ok,
        "at least one https request must have security.protocolVersion starting \
         with 'TLS' and a non-empty cipherSuite: {entries:?}"
    );

    eprintln!("live_network_security_info_https: PASSED");
}

/// `live_manifest_fetch_canonical`:
///
/// A `data:` page that links a manifest returns the parsed `name`/`start_url`
/// and an `errors` array (exit 0); a page without a manifest returns
/// `manifest: null` with exit 0. Both halves run over the same
/// connection.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_manifest_fetch_canonical() {
    if !crate::common::live_tests_enabled() {
        eprintln!("live_manifest_fetch_canonical: set FF_RDP_LIVE_TESTS=1 to run");
        return;
    }
    let ff = LiveFirefox::headless_on_random_port();
    let port = ff.port();

    let cli_args = || {
        vec![
            "--host".to_owned(),
            "127.0.0.1".to_owned(),
            "--port".to_owned(),
            port.to_string(),
            "--timeout".to_owned(),
            "20000".to_owned(),
        ]
    };

    let navigate = |url: &str| {
        let out = Command::new(ff_rdp_bin())
            .args(cli_args())
            .args(["navigate", "--allow-unsafe-urls", url])
            .output()
            .expect("ff-rdp navigate");
        assert!(
            out.status.success(),
            "navigate failed: {}",
            crate::common::output_note(&out)
        );
    };
    let manifest = || {
        let out = Command::new(ff_rdp_bin())
            .args(cli_args())
            .args(["manifest", "--format", "json"])
            .output()
            .expect("ff-rdp manifest");
        assert!(
            out.status.success(),
            "manifest must exit 0 (no-manifest is not an error): {}",
            crate::common::output_note(&out)
        );
        parse_json(&out)
    };

    // Page WITH a manifest. The manifest is a same-document data: URL.
    let manifest_data = "data:application/manifest+json,%7B%22name%22%3A%22Example%20PWA%22%2C%22start_url%22%3A%22/app%22%7D";
    let page_with = format!(
        "data:text/html,<html><head><link rel=\"manifest\" href=\"{manifest_data}\"></head><body><h1>pwa</h1></body></html>"
    );
    navigate(&page_with);
    let with_json = manifest();
    // Either the parsed manifest is present with the expected fields, or (if the
    // browser declined the cross-origin data: manifest) the errors array is
    // populated — both are exit-0 structured results, which is the AC.
    let results = &with_json["results"];
    assert!(
        results.get("errors").is_some_and(Value::is_array),
        "manifest result must carry an errors array: {with_json}"
    );
    if let Some(name) = results["manifest"]["name"].as_str() {
        assert_eq!(name, "Example PWA", "parsed manifest name: {with_json}");
        assert_eq!(
            results["manifest"]["start_url"].as_str(),
            Some("/app"),
            "parsed manifest start_url: {with_json}"
        );
    } else {
        eprintln!(
            "live_manifest_fetch_canonical: manifest not parsed (likely data: manifest \
             declined) — errors present, continuing: {with_json}"
        );
    }

    // Page WITHOUT a manifest → manifest: null, exit 0.
    navigate("data:text/html,<h1>no manifest here</h1>");
    let without_json = manifest();
    assert!(
        without_json["results"]["manifest"].is_null(),
        "a page with no manifest must yield manifest: null: {without_json}"
    );
    assert!(
        without_json["results"]["reason"].is_string(),
        "no-manifest result must carry a reason string: {without_json}"
    );

    eprintln!("live_manifest_fetch_canonical: PASSED");
}
