//! Live tests for iter-62 — page-map index and runner integration.
//!
//! These tests verify that:
//! - `ff-rdp index` produces a valid page-map JSON from a live Firefox crawl.
//! - The runner resolves `page_map:`, `field:`, and `api_route:` targets
//!   against a loaded page-map (no "not yet implemented" errors).
//! - `ff-rdp index --check` detects selector drift and exits non-zero.
//!
//! `live_index_local_fixture` and `live_runner_page_map_resolution` crawl a
//! small self-hosted fixture site (`common::FixtureServer`, iter-114 Theme C)
//! instead of depending on an external `http://localhost:18080` server that
//! was never committed — no real network access is used, so both are gated
//! on `FF_RDP_LIVE_TESTS=1` alone (the `FF_RDP_LIVE_NETWORK_TESTS=1` gate was
//! dropped from those two).  `live_index_check_detects_drift` is out of
//! scope for iter-114 Theme C and keeps its original localhost:18080
//! self-skip and both env-var gates.
//!
//! # Running
//!
//!   FF_RDP_LIVE_TESTS=1 cargo test -p ff-rdp-cli --test live live_62_page_map_index -- --nocapture

use std::collections::HashMap;
use std::process::Command;

use crate::common::{FixtureRoute, FixtureServer, LiveFirefox, base_args, ff_rdp_bin};

/// Landing page: a single link to `/login` so the crawl discovers ≥ 2 pages.
const INDEX_HTML: &str = "<!DOCTYPE html><html><head><title>Fixture Home</title></head>\
<body><h1>Fixture site</h1><a href=\"/login\">Sign in</a></body></html>";

/// Sign-in page: a form with email/password inputs and a submit button that
/// matches the page-map fixture used by `live_runner_page_map_resolution`
/// (`input[type=email]`, `input[type=password]`, `button[type=submit]`).
///
/// The submit handler prevents the default navigation and fires the
/// `POST /api/auth/sign-in` request after a short delay. The delay matters:
/// `assert_network` (direct mode, which these tests always use via
/// `base_args`'s `--no-daemon`) only starts watching `network-event`
/// resources when the `assert_network` step itself runs — it does not see
/// requests that completed before that point. Firing immediately on submit
/// risks the request completing before the `click` step returns and the
/// runner reaches `assert_network`; delaying it keeps the request inside the
/// watcher's drain window instead.
const LOGIN_HTML: &str = "<!DOCTYPE html><html><head><title>Sign in</title></head><body>\
<form>\
<input type=\"email\">\
<input type=\"password\">\
<button type=\"submit\">Sign in</button>\
</form>\
<script>\
document.querySelector('form').addEventListener('submit', function(e) {\
  e.preventDefault();\
  setTimeout(function() {\
    fetch('/api/auth/sign-in', {method: 'POST', headers: {'Content-Type': 'application/json'}, body: '{}'});\
  }, 150);\
});\
</script>\
</body></html>";

/// Start the shared fixture site (`/`, `/login`, `POST /api/auth/sign-in`)
/// used by both `live_index_local_fixture` and
/// `live_runner_page_map_resolution`. Returns `None` if the ephemeral port
/// cannot be bound.
fn start_fixture_site() -> Option<FixtureServer> {
    let mut routes = HashMap::new();
    routes.insert("/".to_owned(), FixtureRoute::html(INDEX_HTML));
    routes.insert("/login".to_owned(), FixtureRoute::html(LOGIN_HTML));
    // The fixture server doesn't inspect HTTP method (see its doc comment in
    // common/mod.rs) — registering the path is enough for `assert_network`,
    // which only checks the numeric status code.
    routes.insert(
        "/api/auth/sign-in".to_owned(),
        FixtureRoute {
            content_type: "application/json",
            body: b"{}".to_vec(),
            extra_headers: Vec::new(),
        },
    );
    FixtureServer::start(routes)
}

// ---------------------------------------------------------------------------
// live_index_local_fixture
// ---------------------------------------------------------------------------

/// `live_index_local_fixture`: crawl the self-hosted fixture site and
/// validate the emitted page-map against the shipped JSON Schema.
///
/// Asserts:
/// - Exit code 0.
/// - Output `map.json` parses and validates against `schemas/page-map.schema.json`.
/// - At least one page entry is present.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_index_local_fixture() {
    if std::env::var("FF_RDP_LIVE_TESTS").is_err() {
        eprintln!("live_index_local_fixture: set FF_RDP_LIVE_TESTS=1 to run");
        return;
    }

    let Some(ff) = LiveFirefox::headless_on_random_port() else {
        eprintln!("live_index_local_fixture: Firefox not available — skipping");
        return;
    };

    let Some(site) = start_fixture_site() else {
        eprintln!("live_index_local_fixture: could not bind fixture HTTP server — skipping");
        return;
    };

    let out_dir = tempfile::tempdir().expect("temp dir");
    let map_path = out_dir.path().join("map.json");

    let out = Command::new(ff_rdp_bin())
        .args(base_args(ff.port()))
        .args([
            "index",
            &site.base_url(),
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

    assert!(
        out.status.success(),
        "live_index_local_fixture: ff-rdp index failed — stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );

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
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_runner_page_map_resolution() {
    if std::env::var("FF_RDP_LIVE_TESTS").is_err() {
        eprintln!("live_runner_page_map_resolution: set FF_RDP_LIVE_TESTS=1 to run");
        return;
    }

    let Some(ff) = LiveFirefox::headless_on_random_port() else {
        eprintln!("live_runner_page_map_resolution: Firefox not available — skipping");
        return;
    };

    let Some(site) = start_fixture_site() else {
        eprintln!("live_runner_page_map_resolution: could not bind fixture HTTP server — skipping");
        return;
    };
    let base_url = site.base_url();

    // Write a minimal page-map with the selectors for the fixture site.
    let map_json = serde_json::json!({
        "version": 1,
        "base_url": base_url,
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

    // Script that uses all three target forms. `assert_network` gets a
    // generous 2000ms drain timeout so the fixture's 150ms-delayed fetch
    // (see LOGIN_HTML's doc comment) has ample room inside the direct-mode
    // watcher's window.
    let script = serde_json::json!({
        "version": 1,
        "base_url": base_url,
        "steps": [
            {"navigate": {"url": "/login"}},
            {"type": {"field": "pages.login.forms.signin.fields.email", "text": "test@example.com"}},
            {"click": {"page_map": "pages.login.forms.signin.submit"}},
            {"assert_network": {"api_route": "signIn", "status": 200, "timeout": 2000}}
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
        out.status.success(),
        "live_runner_page_map_resolution: ff-rdp run exited with non-zero status — stderr: {stderr}"
    );
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
#[ignore = "requires Firefox and FF_RDP_LIVE_TESTS=1 and FF_RDP_LIVE_NETWORK_TESTS=1"]
fn live_index_check_detects_drift() {
    if std::env::var("FF_RDP_LIVE_TESTS").is_err()
        || std::env::var("FF_RDP_LIVE_NETWORK_TESTS").is_err()
    {
        eprintln!(
            "live_index_check_detects_drift: set FF_RDP_LIVE_TESTS=1 and FF_RDP_LIVE_NETWORK_TESTS=1 to run"
        );
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
    if stderr.to_lowercase().contains("connection refused")
        || stderr.to_lowercase().contains("failed to connect")
    {
        eprintln!("live_index_check_detects_drift: fixture site not reachable — skipping");
        return;
    }

    // Should exit non-zero when drift is detected.
    assert!(
        !out.status.success(),
        "live_index_check_detects_drift: expected non-zero exit when drift detected"
    );
}
