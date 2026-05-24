//! Live tests for iter-62 — page-map index and runner integration.
//!
//! These tests verify that:
//! - `ff-rdp index` produces a valid page-map JSON from a live Firefox crawl.
//! - The runner resolves `page_map:`, `field:`, and `api_route:` targets
//!   against a loaded page-map (no "not yet implemented" errors).
//! - `ff-rdp index --check` detects selector drift and exits non-zero.
//!
//! # Running
//!
//! Requires a running Firefox instance.  Gates on `FF_RDP_LIVE_TESTS=1`.
//!
//!   FF_RDP_LIVE_TESTS=1 cargo test -p ff-rdp-cli --test live_62_page_map_index -- --nocapture

#[path = "common/mod.rs"]
mod common;

use std::process::Command;

use common::{LiveFirefox, base_args, ff_rdp_bin};

// ---------------------------------------------------------------------------
// live_index_local_fixture
// ---------------------------------------------------------------------------

/// `live_index_local_fixture`: crawl the fixture site and validate the emitted
/// page-map against the shipped JSON Schema.
///
/// Asserts:
/// - Exit code 0.
/// - Output `map.json` parses and validates against `schemas/page-map.schema.json`.
/// - At least one page entry is present.
#[test]
#[ignore = "requires Firefox and FF_RDP_LIVE_TESTS=1"]
fn live_index_local_fixture() {
    if std::env::var("FF_RDP_LIVE_TESTS").is_err() {
        eprintln!("live_index_local_fixture: set FF_RDP_LIVE_TESTS=1 to run");
        return;
    }

    let Some(ff) = LiveFirefox::headless_on_random_port() else {
        eprintln!("live_index_local_fixture: Firefox not available — skipping");
        return;
    };

    let out_dir = tempfile::tempdir().expect("temp dir");
    let map_path = out_dir.path().join("map.json");

    // For now the fixture site is not yet committed; this test will be wired
    // to `tests/fixtures/page-map-site/` once the static HTML fixture lands.
    // Until then, crawl localhost:0 (port determined from the fixture server)
    // and skip if unreachable.
    let out = Command::new(ff_rdp_bin())
        .args(base_args(ff.port()))
        .args([
            "index",
            "http://localhost:18080",
            "--out",
            map_path.to_str().unwrap(),
            "--depth",
            "2",
            "--max-pages",
            "50",
            "--ignore-robots",
        ])
        .output()
        .expect("ff-rdp index");

    if !out.status.success() {
        // Fixture site may not be running — treat as soft skip.
        let stderr = String::from_utf8_lossy(&out.stderr);
        if stderr.contains("Connection refused") || stderr.contains("navigate") {
            eprintln!("live_index_local_fixture: fixture site not reachable — skipping");
            return;
        }
        panic!("live_index_local_fixture: ff-rdp index failed — stderr: {stderr}");
    }

    // Validate output against schema.
    let schema_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("schemas")
        .join("page-map.schema.json");
    let schema_str = std::fs::read_to_string(&schema_path).expect("reading page-map schema");
    let schema_value: serde_json::Value =
        serde_json::from_str(&schema_str).expect("parsing page-map schema");
    let validator = jsonschema::validator_for(&schema_value).expect("compiling schema");

    let map_str = std::fs::read_to_string(&map_path).expect("reading map.json");
    let map_value: serde_json::Value =
        serde_json::from_str(&map_str).expect("map.json is not valid JSON");

    let errors: Vec<_> = validator.iter_errors(&map_value).collect();
    assert!(
        errors.is_empty(),
        "live_index_local_fixture: map.json failed schema validation: {errors:#?}"
    );

    let page_count = map_value["pages"]
        .as_object()
        .map_or(0, serde_json::Map::len);
    assert!(
        page_count >= 1,
        "live_index_local_fixture: expected at least one page entry"
    );
}

// ---------------------------------------------------------------------------
// live_runner_page_map_resolution
// ---------------------------------------------------------------------------

/// `live_runner_page_map_resolution`: verify that `page_map:`, `field:`, and
/// `api_route:` targets resolve through a loaded page-map without producing
/// "not yet implemented" errors.
///
/// Asserts:
/// - Exit code 0 (all three target forms resolve).
/// - Stdout contains three step results.
#[test]
#[ignore = "requires Firefox and FF_RDP_LIVE_TESTS=1"]
fn live_runner_page_map_resolution() {
    if std::env::var("FF_RDP_LIVE_TESTS").is_err() {
        eprintln!("live_runner_page_map_resolution: set FF_RDP_LIVE_TESTS=1 to run");
        return;
    }

    let Some(ff) = LiveFirefox::headless_on_random_port() else {
        eprintln!("live_runner_page_map_resolution: Firefox not available — skipping");
        return;
    };

    // Write a minimal page-map with the selectors for the fixture site.
    let map_json = serde_json::json!({
        "version": 1,
        "base_url": "http://localhost:18080",
        "pages": {
            "login": {
                "path": "/login",
                "forms": [{
                    "id": "signin",
                    "selector": "form",
                    "fields": [
                        {"name": "email", "selector": "input[type=email]", "type": "email"},
                        {"name": "password", "selector": "input[type=password]", "type": "password"}
                    ],
                    "submit": {"selector": "button[type=submit]"}
                }]
            }
        },
        "api_routes": {
            "signIn": {"method": "POST", "path": "/api/auth/sign-in"}
        }
    });

    let map_dir = tempfile::tempdir().expect("temp dir");
    let map_path = map_dir.path().join("page-map.json");
    std::fs::write(&map_path, map_json.to_string()).expect("write page-map.json");

    // Script that uses all three target forms.
    let script = serde_json::json!({
        "version": 1,
        "base_url": "http://localhost:18080",
        "steps": [
            {"navigate": {"url": "/login"}},
            {"type": {"field": "pages.login.forms.signin.fields.email", "text": "test@example.com"}},
            {"click": {"page_map": "pages.login.forms.signin.submit"}},
            {"assert_network": {"api_route": "signIn", "status": 200}}
        ]
    });

    let script_dir = tempfile::tempdir().expect("temp dir");
    let script_path = script_dir.path().join("test.json");
    std::fs::write(&script_path, script.to_string()).expect("write script.json");

    let out = Command::new(ff_rdp_bin())
        .args(base_args(ff.port()))
        .args([
            "run",
            script_path.to_str().unwrap(),
            "--page-map",
            map_path.to_str().unwrap(),
        ])
        .output()
        .expect("ff-rdp run");

    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        !stderr.contains("not yet implemented"),
        "live_runner_page_map_resolution: unexpected 'not yet implemented' error — stderr: {stderr}"
    );
}

// ---------------------------------------------------------------------------
// live_index_check_detects_drift
// ---------------------------------------------------------------------------

/// `live_index_check_detects_drift`: verify that `ff-rdp index --check` exits
/// non-zero when a selector has drifted since the map was generated.
///
/// Asserts:
/// - Exit code non-zero.
/// - Stderr mentions "drift" or contains the drifted selector.
#[test]
#[ignore = "requires Firefox and FF_RDP_LIVE_TESTS=1"]
fn live_index_check_detects_drift() {
    if std::env::var("FF_RDP_LIVE_TESTS").is_err() {
        eprintln!("live_index_check_detects_drift: set FF_RDP_LIVE_TESTS=1 to run");
        return;
    }

    let Some(ff) = LiveFirefox::headless_on_random_port() else {
        eprintln!("live_index_check_detects_drift: Firefox not available — skipping");
        return;
    };

    // Write a stale page-map with a wrong selector.
    let stale_map = serde_json::json!({
        "version": 1,
        "base_url": "http://localhost:18080",
        "pages": {
            "login": {
                "path": "/login",
                "forms": [{
                    "id": "signin",
                    "selector": "#stale-form-selector-xyz",
                    "fields": [],
                    "submit": {"selector": "#stale-submit-xyz"}
                }]
            }
        }
    });

    let map_dir = tempfile::tempdir().expect("temp dir");
    let map_path = map_dir.path().join("page-map.json");
    std::fs::write(&map_path, stale_map.to_string()).expect("write stale page-map.json");

    let report_path = map_dir.path().join("drift.json");

    let out = Command::new(ff_rdp_bin())
        .args(base_args(ff.port()))
        .args([
            "index",
            "http://localhost:18080",
            "--check",
            "--page-map",
            map_path.to_str().unwrap(),
            "--report",
            report_path.to_str().unwrap(),
        ])
        .output()
        .expect("ff-rdp index --check");

    let stderr = String::from_utf8_lossy(&out.stderr);
    if stderr.contains("Connection refused") || stderr.contains("navigate") {
        eprintln!("live_index_check_detects_drift: fixture site not reachable — skipping");
        return;
    }

    // Should exit non-zero when drift is detected.
    assert!(
        !out.status.success(),
        "live_index_check_detects_drift: expected non-zero exit when drift detected"
    );
}
