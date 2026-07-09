//! iter-82 AC: `live_cookies_surfaces_js_readable_cookie`.
//! iter-83 AC: `live_cookies_default_surfaces_js_readable_cookie`.
//!
//! Serves a minimal HTML page on a localhost HTTP port, navigates Firefox to
//! it, then runs `ff-rdp cookies --include-document-cookie` and asserts the
//! cookie name `"probe"` appears in the results.
//!
//! A `data:` URL is used as a fallback fixture in many tests, but cookies set
//! via `document.cookie` on `data:` URLs do not persist — browsers treat them
//! as cookie-averse origins.  A real `http://127.0.0.1` origin is required.
//!
//! This validates Theme D: cookies set via JS without a `Domain=` attribute
//! (which StorageActor sometimes misses) are surfaced via the
//! `--include-document-cookie` fallback path.
//!
//! # Running
//!
//!   FF_RDP_LIVE_TESTS=1 cargo test-live -p ff-rdp-cli \
//!       --test live live_cookies -- --nocapture

use std::io::{Read, Write};
use std::net::TcpListener;
use std::process::Command;
use std::thread;
use std::time::Duration;

use crate::common::{LiveFirefox, base_args, ff_rdp_bin};

/// HTML body served by the local fixture server.
///
/// Sets `document.cookie = "probe=1"` so the cookie is JS-readable on
/// the `http://127.0.0.1` origin.  The `source: "document.cookie"` path
/// in ff-rdp cookies must surface this entry when
/// `--include-document-cookie` is passed.
const FIXTURE_BODY: &[u8] = b"<!DOCTYPE html><html><head></head><body>\
<script>document.cookie='probe=1';window.__cookieSet=true;</script>\
</body></html>";

/// Spawn a minimal single-shot HTTP server on a random port.
///
/// The server accepts connections in a background thread (bounded to 10
/// accepts so the thread exits after the test), serving `FIXTURE_BODY`
/// with `Content-Type: text/html` on any `GET` request.  Returns
/// `(port, join-handle)`.
fn spawn_fixture_server() -> Option<(u16, thread::JoinHandle<()>)> {
    let listener = TcpListener::bind("127.0.0.1:0").ok()?;
    let port = listener.local_addr().ok()?.port();

    let handle = thread::spawn(move || {
        listener.set_nonblocking(false).ok();
        for stream in listener.incoming().take(10) {
            let Ok(mut stream) = stream else { continue };
            let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
            // Drain the HTTP request so the browser doesn't see a reset.
            let mut buf = [0u8; 2048];
            let _ = stream.read(&mut buf);

            let resp = format!(
                "HTTP/1.1 200 OK\r\n\
                 Content-Type: text/html; charset=utf-8\r\n\
                 Content-Length: {}\r\n\
                 Cache-Control: no-store\r\n\
                 Connection: close\r\n\r\n",
                FIXTURE_BODY.len()
            );
            let _ = stream.write_all(resp.as_bytes());
            let _ = stream.write_all(FIXTURE_BODY);
        }
    });

    Some((port, handle))
}

/// `live_cookies_surfaces_js_readable_cookie`:
/// Serve a fixture page that sets `document.cookie = "probe=1"` from JS
/// on a real `http://127.0.0.1` origin, navigate Firefox to it, then
/// assert that `ff-rdp cookies --include-document-cookie` surfaces a
/// cookie named `"probe"` in its results.
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

    let Some((http_port, _server)) = spawn_fixture_server() else {
        eprintln!(
            "live_cookies_surfaces_js_readable_cookie: could not bind HTTP server — skipping"
        );
        return;
    };

    let fixture_url = format!("http://127.0.0.1:{http_port}/");

    // Navigate to fixture so the JS cookie is set.
    let nav = Command::new(ff_rdp_bin())
        .args(base_args(ff.port()))
        .args(["navigate", &fixture_url])
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

    // Also assert the entry was surfaced via the document.cookie path.
    let probe = results
        .iter()
        .find(|c| c["name"].as_str().unwrap_or("") == "probe")
        .expect("probe cookie must exist");
    assert_eq!(
        probe["source"].as_str().unwrap_or(""),
        "document.cookie",
        "probe cookie must have source 'document.cookie'; got {:?}",
        probe["source"]
    );

    eprintln!(
        "live_cookies_surfaces_js_readable_cookie: PASS — found 'probe' among {} cookies",
        results.len()
    );
}

/// `live_cookies_default_surfaces_js_readable_cookie` (iter-83 AC):
///
/// Same as `live_cookies_surfaces_js_readable_cookie` but calls `ff-rdp cookies`
/// WITHOUT any `--include-document-cookie` flag.  Verifies that the default
/// behavior (as of iter-83 Theme D) includes document.cookie evaluation, so the
/// `probe` cookie shows up without an explicit flag.
///
/// Gated on `FF_RDP_LIVE_TESTS=1`.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_cookies_default_surfaces_js_readable_cookie() {
    if std::env::var("FF_RDP_LIVE_TESTS").is_err() {
        eprintln!(
            "live_cookies_default_surfaces_js_readable_cookie: set FF_RDP_LIVE_TESTS=1 to run"
        );
        return;
    }

    let Some(ff) = LiveFirefox::headless_on_random_port() else {
        eprintln!(
            "live_cookies_default_surfaces_js_readable_cookie: Firefox not available — skipping"
        );
        return;
    };

    let Some((http_port, _server)) = spawn_fixture_server() else {
        eprintln!(
            "live_cookies_default_surfaces_js_readable_cookie: could not bind HTTP server — skipping"
        );
        return;
    };

    let fixture_url = format!("http://127.0.0.1:{http_port}/");

    // Navigate to fixture so the JS cookie is set.
    let nav = Command::new(ff_rdp_bin())
        .args(base_args(ff.port()))
        .args(["navigate", &fixture_url])
        .output()
        .expect("ff-rdp navigate");
    assert!(
        nav.status.success(),
        "live_cookies_default_surfaces_js_readable_cookie: navigate failed — {}",
        String::from_utf8_lossy(&nav.stderr)
    );

    // Run cookies WITHOUT --include-document-cookie — this is the key difference
    // from the iter-82 test: the default must now include document.cookie.
    let out = Command::new(ff_rdp_bin())
        .args(base_args(ff.port()))
        .args(["cookies"])
        .output()
        .expect("ff-rdp cookies (no flags)");
    assert!(
        out.status.success(),
        "live_cookies_default_surfaces_js_readable_cookie: cookies failed — stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );

    let stdout = String::from_utf8_lossy(&out.stdout);
    let json: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap_or_else(|e| {
        panic!(
            "live_cookies_default_surfaces_js_readable_cookie: output is not valid JSON: {e}\n\
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
        "live_cookies_default_surfaces_js_readable_cookie: cookie 'probe' not found in results; \
         results={results:?}"
    );

    eprintln!(
        "live_cookies_default_surfaces_js_readable_cookie: PASS — \
         found 'probe' among {} cookies (no --include-document-cookie flag)",
        results.len()
    );
}
