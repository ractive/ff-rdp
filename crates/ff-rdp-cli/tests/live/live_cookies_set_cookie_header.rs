/// Live test for Theme L (iter-84): `cookies` surfaces cookies set via
/// `Set-Cookie` response headers on the navigation that just completed,
/// even when Firefox has not yet flushed them to the StorageActor.
///
/// A local single-shot HTTP fixture server sends `Set-Cookie: probe=1` on
/// every GET (mirroring what `httpbin.org/cookies/set?probe=1` used to do),
/// removing the dependency on a real external network endpoint.
///
/// AC: live_cookies_set_cookie_header — results contains {name:"probe",value:"1"}
///     within one invocation of `cookies`
use std::io::{Read, Write};
use std::net::TcpListener;
use std::process::Command;
use std::thread;
use std::time::Duration;

use crate::common::{LiveFirefox, base_args, ff_rdp_bin, live_tests_enabled};

/// Minimal HTML body served alongside the `Set-Cookie` header.
const FIXTURE_BODY: &[u8] = b"<!DOCTYPE html><html><head></head><body>probe</body></html>";

/// Spawn a minimal single-shot HTTP server on a random port that responds to
/// any `GET` with `Set-Cookie: probe=1` plus a minimal HTML body — mirroring
/// what `httpbin.org/cookies/set?probe=1` used to do via its redirect.
///
/// Bounded to 10 accepts so the thread exits after the test. Returns
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
                 Set-Cookie: probe=1\r\n\
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

/// Theme L: cookies set via `Set-Cookie` response header (now served by a
/// local fixture server rather than httpbin.org) appear in `cookies list`
/// output immediately after navigation.
///
/// Self-launches headless Firefox on a random port; no external network
/// access required.
/// Post-condition: cookie `probe=1` present in results.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_cookies_set_cookie_header_visible_after_navigate() {
    if !live_tests_enabled() {
        eprintln!(
            "live_cookies_set_cookie_header_visible_after_navigate: set FF_RDP_LIVE_TESTS=1 to run"
        );
        return;
    }

    let Some(ff) = LiveFirefox::headless_on_random_port() else {
        eprintln!(
            "live_cookies_set_cookie_header_visible_after_navigate: Firefox not available — skipping"
        );
        return;
    };

    let Some((http_port, _server)) = spawn_fixture_server() else {
        eprintln!(
            "live_cookies_set_cookie_header_visible_after_navigate: could not bind HTTP server — skipping"
        );
        return;
    };

    let fixture_url = format!("http://127.0.0.1:{http_port}/");

    // This URL sets a cookie via a Set-Cookie response header.
    let nav = Command::new(ff_rdp_bin())
        .args(base_args(ff.port()))
        .args(["navigate", &fixture_url])
        .output()
        .expect("ff-rdp navigate failed");
    assert!(
        nav.status.success(),
        "navigate failed: {}",
        String::from_utf8_lossy(&nav.stderr)
    );

    // `cookies` takes no `list` subcommand (that syntax predates a CLI
    // restructuring) — it is a flat `ff-rdp cookies [OPTIONS]` command.
    let out = Command::new(ff_rdp_bin())
        .args(base_args(ff.port()))
        .args(["cookies"])
        .output()
        .expect("ff-rdp cookies failed");

    assert!(
        out.status.success(),
        "cookies failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let stdout = String::from_utf8_lossy(&out.stdout);
    let json: serde_json::Value =
        serde_json::from_str(&stdout).expect("cookies output is not valid JSON");

    let results = json["results"].as_array().expect("results is not array");

    let probe_cookie = results
        .iter()
        .find(|c| c.get("name").and_then(|v| v.as_str()) == Some("probe"));

    assert!(
        probe_cookie.is_some(),
        "Theme L regression: cookie 'probe' not found in cookies list after \
         navigating to the local Set-Cookie fixture — Set-Cookie header \
         cookie may not have been flushed to StorageActor yet"
    );

    if let Some(cookie) = probe_cookie {
        assert_eq!(
            cookie.get("value").and_then(|v| v.as_str()),
            Some("1"),
            "cookie 'probe' has wrong value: {cookie:?}"
        );
    }
}
