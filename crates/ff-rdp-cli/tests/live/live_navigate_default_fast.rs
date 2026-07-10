/// Live test for Theme C (iter-84): `navigate` with default `--wait both`
/// strategy completes without "no remaining budget" timeout errors.
///
/// Root cause: the events phase consumed the full timeout budget leaving 0ms
/// for the readystate fallback. Fixed by splitting the budget 70/30.
///
/// Self-launches headless Firefox on a random port and navigates to a local
/// HTTP fixture (rather than https://example.com) — a real HTTP round trip
/// still exercises the default `--wait both` budget-splitting code path
/// (unlike a `data:` URL, which resolves instantly and would not meaningfully
/// exercise the events/readystate budget split under test).
///
/// AC: live_navigate_default_fast — completes in ≤ timeout_ms with status:ok
use std::io::{Read, Write};
use std::net::TcpListener;
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};

use crate::common::{LiveFirefox, base_args, ff_rdp_bin, live_tests_enabled};

const FIXTURE_BODY: &[u8] =
    b"<!DOCTYPE html><html><head></head><body><p>navigate fast fixture</p></body></html>";

/// Spawn a minimal HTTP server serving `FIXTURE_BODY` on any GET. Bounded to
/// 10 accepts so the thread exits after the test. Returns `(port, join-handle)`.
fn spawn_html_server() -> Option<(u16, thread::JoinHandle<()>)> {
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
                FIXTURE_BODY.len()
            );
            let _ = stream.write_all(resp.as_bytes());
            let _ = stream.write_all(FIXTURE_BODY);
        }
    });
    Some((port, handle))
}

/// Theme C: navigate with default `--wait both` strategy does not exhaust
/// its budget before the readystate fallback fires.
///
/// Self-launches headless Firefox on a random port.
/// Post-condition: exit 0 within 10 s; no "no remaining budget" in stderr.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_navigate_default_fast_no_budget_exhaustion() {
    if !live_tests_enabled() {
        eprintln!(
            "live_navigate_default_fast_no_budget_exhaustion: set FF_RDP_LIVE_TESTS=1 to run"
        );
        return;
    }

    let Some(ff) = LiveFirefox::headless_on_random_port() else {
        eprintln!(
            "live_navigate_default_fast_no_budget_exhaustion: Firefox not available — skipping"
        );
        return;
    };

    let Some((http_port, _server)) = spawn_html_server() else {
        eprintln!(
            "live_navigate_default_fast_no_budget_exhaustion: could not bind HTTP server — skipping"
        );
        return;
    };
    let url = format!("http://127.0.0.1:{http_port}/");

    let start = Instant::now();
    let mut args = base_args(ff.port());
    // Global --timeout must be placed before the subcommand.
    let out = Command::new(ff_rdp_bin())
        .args({
            args.push("--timeout".to_owned());
            args.push("8000".to_owned());
            args
        })
        .args(["navigate", &url])
        .output()
        .expect("ff-rdp navigate failed");

    let elapsed = start.elapsed().as_millis();
    let stderr = String::from_utf8_lossy(&out.stderr);

    assert!(out.status.success(), "navigate failed: {stderr}");
    assert!(
        !stderr.contains("no remaining budget"),
        "Theme C regression: 'no remaining budget' appeared in stderr: {stderr}"
    );
    assert!(
        elapsed < 10_000,
        "navigate took too long: {elapsed}ms (expected < 10000ms)"
    );
}

/// `--timeout` (global operation timeout, placed before the subcommand) is
/// honored by `navigate` and the command still completes successfully.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_navigate_global_timeout_flag_accepted() {
    if !live_tests_enabled() {
        eprintln!("live_navigate_global_timeout_flag_accepted: set FF_RDP_LIVE_TESTS=1 to run");
        return;
    }

    let Some(ff) = LiveFirefox::headless_on_random_port() else {
        eprintln!("live_navigate_global_timeout_flag_accepted: Firefox not available — skipping");
        return;
    };

    let Some((http_port, _server)) = spawn_html_server() else {
        eprintln!(
            "live_navigate_global_timeout_flag_accepted: could not bind HTTP server — skipping"
        );
        return;
    };
    let url = format!("http://127.0.0.1:{http_port}/");

    let mut args = base_args(ff.port());
    args.push("--timeout".to_owned());
    args.push("5000".to_owned());
    let out = Command::new(ff_rdp_bin())
        .args(args)
        .args(["navigate", &url])
        .output()
        .expect("ff-rdp navigate failed");

    assert!(
        out.status.success(),
        "navigate with --timeout failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}
