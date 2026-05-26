//! iter-82 AC: `live_cookies_surfaces_js_readable_cookie`.
//!
//! Navigates to a fixture page that sets `document.cookie = "probe=1"` from
//! JS, then runs `ff-rdp cookies --include-document-cookie` and asserts the
//! cookie name `"probe"` appears in the results.
//!
//! This validates Theme D: cookies set via JS without a `Domain=` attribute
//! (which StorageActor sometimes misses) are surfaced via the
//! `--include-document-cookie` fallback path.
//!
//! # Running
//!
//!   FF_RDP_LIVE_TESTS=1 cargo test-live -p ff-rdp-cli \
//!       --test live_cookies -- --nocapture

#[path = "common/mod.rs"]
mod common;

use std::process::Command;

use common::{LiveFirefox, base_args, ff_rdp_bin};

/// Fixture page: sets a JS-writable cookie without `Domain=` or `Secure`,
/// then exposes `window.__cookieSet = true` so the caller knows the script ran.
const FIXTURE_HTML: &str = "data:text/html;charset=utf-8,\
<!DOCTYPE html><html><head></head><body>\
<script>document.cookie='probe=1';window.__cookieSet=true;</script>\
</body></html>";

/// `live_cookies_surfaces_js_readable_cookie`:
/// Navigate to a fixture page that sets `document.cookie = "probe=1"` from
/// JS, then assert that `ff-rdp cookies --include-document-cookie` surfaces
/// a cookie named `"probe"` in its results.
///
/// Gated on `FF_RDP_LIVE_TESTS=1`.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_cookies_surfaces_js_readable_cookie() {
    if std::env::var("FF_RDP_LIVE_TESTS").is_err() {
        eprintln!("live_cookies_surfaces_js_readable_cookie: set FF_RDP_LIVE_TESTS=1 to run");
        return;
    }

    let Some(ff) = LiveFirefox::headless_on_random_port() else {
        eprintln!("live_cookies_surfaces_js_readable_cookie: Firefox not available — skipping");
        return;
    };

    // Navigate to fixture so the JS cookie is set.
    let nav = Command::new(ff_rdp_bin())
        .args(base_args(ff.port()))
        .args(["navigate", FIXTURE_HTML])
        .output()
        .expect("ff-rdp navigate");
    assert!(
        nav.status.success(),
        "live_cookies_surfaces_js_readable_cookie: navigate failed — {}",
        String::from_utf8_lossy(&nav.stderr)
    );

    // Run cookies with the document-cookie fallback enabled.
    let out = Command::new(ff_rdp_bin())
        .args(base_args(ff.port()))
        .args(["cookies", "--include-document-cookie"])
        .output()
        .expect("ff-rdp cookies --include-document-cookie");
    assert!(
        out.status.success(),
        "live_cookies_surfaces_js_readable_cookie: cookies failed — stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );

    let stdout = String::from_utf8_lossy(&out.stdout);
    let json: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap_or_else(|e| {
        panic!(
            "live_cookies_surfaces_js_readable_cookie: output is not valid JSON: {e}\n\
                 stdout={stdout}\nstderr={}",
            String::from_utf8_lossy(&out.stderr)
        )
    });

    let results = json["results"]
        .as_array()
        .expect("results must be an array");

    let has_probe = results
        .iter()
        .any(|c| c["name"].as_str().unwrap_or("") == "probe");

    assert!(
        has_probe,
        "live_cookies_surfaces_js_readable_cookie: cookie 'probe' not found in results; \
         results={results:?}"
    );

    eprintln!(
        "live_cookies_surfaces_js_readable_cookie: PASS — found 'probe' among {} cookies",
        results.len()
    );
}
