/// Live test for Theme I (iter-84): a second command after `navigate`
/// connects to the correct tab and does not hit a stale ActorId cached
/// from a previous session.
///
/// The race is: navigate closes+reopens the tab (changes ActorId), then
/// a second command (e.g. `dom stats`) uses the cached ActorId from the
/// pre-navigate state, getting a "No such actor" error from Firefox.
///
/// The race is believed to be triggered specifically by a cross-*site*
/// navigation (different eTLD+1 under Fission process isolation), which
/// forces Firefox to swap the tab to a new content process — the exact
/// scenario that invalidates a cached actor. iter-114 Theme B ports this off
/// the legacy port-6000 `example.com` → `example.org` pair onto two local
/// HTTP servers bound to different host strings (`127.0.0.1` vs
/// `localhost`), which Firefox also treats as distinct origins/sites, so the
/// same process-swap conditions are preserved without external network
/// access.
///
/// AC: live_stale_tab_race — dom stats succeeds immediately after navigate
///     on a fresh Firefox session (no "No such actor" error)
use crate::common::{LiveFirefox, base_args, ff_rdp_bin};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::process::Command;
use std::thread;
use std::time::Duration;

/// Spawn a minimal HTTP server that serves `body` (with `Content-Type:
/// text/html`) on any GET. Bounded to 10 accepts so the thread exits after
/// the test. Returns `(port, join-handle)`.
fn spawn_html_server(body: &'static [u8]) -> Option<(u16, thread::JoinHandle<()>)> {
    let listener = TcpListener::bind("127.0.0.1:0").ok()?;
    let port = listener.local_addr().ok()?.port();
    let handle = thread::spawn(move || {
        listener.set_nonblocking(false).ok();
        for stream in listener.incoming().take(10) {
            let Ok(mut stream) = stream else { continue };
            let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
            let mut buf = [0u8; 2048];
            let _ = stream.read(&mut buf);
            let resp = format!(
                "HTTP/1.1 200 OK\r\n\
                 Content-Type: text/html; charset=utf-8\r\n\
                 Content-Length: {}\r\n\
                 Cache-Control: no-store\r\n\
                 Connection: close\r\n\r\n",
                body.len()
            );
            let _ = stream.write_all(resp.as_bytes());
            let _ = stream.write_all(body);
        }
    });
    Some((port, handle))
}

/// Theme I: running `navigate` then immediately `dom stats` does not produce
/// a "No such actor" error due to a stale tab handle.
///
/// Post-condition: `dom stats` exits 0 after `navigate` completes.
#[test]
#[ignore = "requires FF_RDP_LIVE_TESTS=1 and a live Firefox instance"]
fn live_stale_tab_race_no_such_actor_after_navigate() {
    if std::env::var("FF_RDP_LIVE_TESTS").is_err() {
        eprintln!(
            "live_stale_tab_race_no_such_actor_after_navigate: set FF_RDP_LIVE_TESTS=1 to run"
        );
        return;
    }

    let Some(ff) = LiveFirefox::headless_on_random_port() else {
        eprintln!(
            "live_stale_tab_race_no_such_actor_after_navigate: Firefox not available — skipping"
        );
        return;
    };

    let Some((port_a, _server_a)) =
        spawn_html_server(b"<!DOCTYPE html><html><body><p id=\"a\">site A</p></body></html>")
    else {
        eprintln!(
            "live_stale_tab_race_no_such_actor_after_navigate: could not bind HTTP server A — skipping"
        );
        return;
    };
    let Some((port_b, _server_b)) =
        spawn_html_server(b"<!DOCTYPE html><html><body><p id=\"b\">site B</p></body></html>")
    else {
        eprintln!(
            "live_stale_tab_race_no_such_actor_after_navigate: could not bind HTTP server B — skipping"
        );
        return;
    };

    // Two different host strings (127.0.0.1 vs localhost) on different ports
    // are distinct sites under Fission, so this reproduces the cross-site
    // process swap the original example.com → example.org pair relied on.
    let url_a = format!("http://127.0.0.1:{port_a}/");
    let url_b = format!("http://localhost:{port_b}/");

    // First navigate — establishes a tab actor.
    let nav1 = Command::new(ff_rdp_bin())
        .args(base_args(ff.port()))
        .args(["navigate", &url_a])
        .output()
        .expect("navigate 1 failed");
    assert!(
        nav1.status.success(),
        "navigate 1 failed: {}",
        String::from_utf8_lossy(&nav1.stderr)
    );

    // Sanity check: confirm we actually landed on site A before racing the
    // second navigate, so a real re-navigate (not a no-op) is exercised.
    let text1 = Command::new(ff_rdp_bin())
        .args(base_args(ff.port()))
        .args(["page-text"])
        .output()
        .expect("page-text after navigate 1 failed");
    assert!(
        text1.status.success(),
        "page-text after navigate 1 failed: {}",
        String::from_utf8_lossy(&text1.stderr)
    );
    let text1_json: serde_json::Value =
        serde_json::from_str(String::from_utf8_lossy(&text1.stdout).trim())
            .expect("page-text output must be JSON");
    assert!(
        text1_json["results"]
            .as_str()
            .unwrap_or_default()
            .contains("site A"),
        "expected to land on site A before the cross-site navigate; got {text1_json}"
    );

    // Second navigate — cross-site (different host string), which forces the
    // Fission process swap that changes the tab's ActorId.
    let nav2 = Command::new(ff_rdp_bin())
        .args(base_args(ff.port()))
        .args(["navigate", &url_b])
        .output()
        .expect("navigate 2 failed");
    assert!(
        nav2.status.success(),
        "navigate 2 failed: {}",
        String::from_utf8_lossy(&nav2.stderr)
    );

    // dom stats after re-navigate — must not use a stale actor ID.
    let stats = Command::new(ff_rdp_bin())
        .args(base_args(ff.port()))
        .args(["dom", "stats"])
        .output()
        .expect("dom stats failed");

    let stderr = String::from_utf8_lossy(&stats.stderr);
    assert!(
        stats.status.success(),
        "Theme I regression: dom stats failed after cross-site double-navigate: {stderr}"
    );
    assert!(
        !stderr.contains("No such actor"),
        "Theme I regression: 'No such actor' in stderr — stale tab cache bug: {stderr}"
    );

    eprintln!("live_stale_tab_race_no_such_actor_after_navigate: PASS");
}
