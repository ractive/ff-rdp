//! iter-93 live tests — `eval` survives strict Content Security Policy sites.
//!
//! Verifies that `ff-rdp eval` works correctly when the page sets a strict CSP
//! that would block page-`eval()` calls.  The key fix (iter-93): `build_script`
//! no longer wraps user code in an `eval()` call, so Firefox's
//! `Debugger.evalInGlobal` path handles evaluation — which bypasses page CSP.
//!
//! # Running
//!
//!   FF_RDP_LIVE_TESTS=1 cargo test-live -p ff-rdp-cli \
//!       --test live live_eval_csp -- --nocapture
//!
//! For the MDN test, also set `FF_RDP_LIVE_NETWORK_TESTS=1`.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::process::Command;
use std::thread;
use std::time::Duration;

use crate::common::{LiveFirefox, base_args, ff_rdp_bin};

/// HTML fixture body: strict CSP via HTTP header, a known title, and a tall
/// body so `window.scrollTo(0, 100)` produces a non-zero `scrollY`.
const FIXTURE_TITLE: &str = "iter93-csp-fixture";
const FIXTURE_BODY: &[u8] = b"<!DOCTYPE html>\
<html>\
<head><title>iter93-csp-fixture</title></head>\
<body><div style=\"height:5000px\">x</div></body>\
</html>";

/// Content-Security-Policy header value served with the fixture.
const FIXTURE_CSP: &str = "script-src 'self'; object-src 'none'; base-uri 'self'";

/// Spawn a minimal HTTP server that serves `FIXTURE_BODY` with a strict CSP
/// header.  Returns `(port, join-handle)`.  The server accepts up to 20
/// connections before the thread exits.
fn spawn_csp_fixture_server() -> Option<(u16, thread::JoinHandle<()>)> {
    let listener = TcpListener::bind("127.0.0.1:0").ok()?;
    let port = listener.local_addr().ok()?.port();

    let handle = thread::spawn(move || {
        listener.set_nonblocking(false).ok();
        for stream in listener.incoming().take(20) {
            let Ok(mut stream) = stream else { continue };
            let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
            // Drain the HTTP request so the browser doesn't see a reset.
            let mut buf = [0u8; 4096];
            let _ = stream.read(&mut buf);

            let resp = format!(
                "HTTP/1.1 200 OK\r\n\
                 Content-Type: text/html; charset=utf-8\r\n\
                 Content-Length: {}\r\n\
                 Content-Security-Policy: {}\r\n\
                 Cache-Control: no-store\r\n\
                 Connection: close\r\n\r\n",
                FIXTURE_BODY.len(),
                FIXTURE_CSP,
            );
            let _ = stream.write_all(resp.as_bytes());
            let _ = stream.write_all(FIXTURE_BODY);
        }
    });

    Some((port, handle))
}

fn parse_json(stdout: &[u8], stderr: &[u8]) -> serde_json::Value {
    let s = String::from_utf8_lossy(stdout);
    serde_json::from_str(s.trim()).unwrap_or_else(|e| {
        panic!(
            "stdout is not valid JSON: {e}\nstdout={s}\nstderr={}",
            String::from_utf8_lossy(stderr)
        )
    })
}

/// `pre_fix_repro_eval_works_on_strict_csp_site`:
/// Serve a page with `Content-Security-Policy: script-src 'self'; …`, navigate
/// Firefox to it, then eval `document.title`.
///
/// On `origin/main` (before iter-93) this failed with
/// `EvalError: call to eval() blocked by CSP`.  On this branch it must exit 0
/// and return the title string.
#[test]
#[ignore = "requires headless Firefox; set FF_RDP_LIVE_TESTS=1"]
fn pre_fix_repro_eval_works_on_strict_csp_site() {
    if std::env::var("FF_RDP_LIVE_TESTS").is_err() {
        eprintln!("pre_fix_repro_eval_works_on_strict_csp_site: set FF_RDP_LIVE_TESTS=1 to run");
        return;
    }

    let Some((port, _srv)) = spawn_csp_fixture_server() else {
        panic!("could not bind fixture server");
    };
    let fixture_url = format!("http://127.0.0.1:{port}/");

    let Some(ff) = LiveFirefox::headless_on_random_port() else {
        eprintln!("pre_fix_repro_eval_works_on_strict_csp_site: Firefox not available — skipping");
        return;
    };

    let ff_args = || base_args(ff.port());

    let nav = Command::new(ff_rdp_bin())
        .args(ff_args())
        .args(["navigate", &fixture_url])
        .output()
        .expect("navigate to CSP fixture");
    assert!(
        nav.status.success(),
        "navigate failed: {}",
        String::from_utf8_lossy(&nav.stderr)
    );

    let out = Command::new(ff_rdp_bin())
        .args(ff_args())
        .args(["eval", "document.title"])
        .output()
        .expect("eval document.title on CSP page");

    assert!(
        out.status.success(),
        "eval exited non-zero (CSP still blocking?) — stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let json = parse_json(&out.stdout, &out.stderr);
    assert_eq!(
        json["results"].as_str().unwrap_or(""),
        FIXTURE_TITLE,
        "results must equal the fixture title; got: {}",
        json["results"]
    );
    // The eval_path must be "page-await" (Debugger.evalInGlobal path, not chrome fallback).
    assert_eq!(
        json["meta"]["eval_path"].as_str().unwrap_or(""),
        "page-await",
        "eval_path must be page-await"
    );
}

/// `live_eval_returns_window_scroll_y_on_csp_site`:
/// Navigate to the CSP fixture (tall div), scroll 100 px, eval `window.scrollY`.
/// Result must be a number ≥ 1.
#[test]
#[ignore = "requires headless Firefox; set FF_RDP_LIVE_TESTS=1"]
fn live_eval_returns_window_scroll_y_on_csp_site() {
    if std::env::var("FF_RDP_LIVE_TESTS").is_err() {
        eprintln!("live_eval_returns_window_scroll_y_on_csp_site: set FF_RDP_LIVE_TESTS=1 to run");
        return;
    }

    let Some((port, _srv)) = spawn_csp_fixture_server() else {
        panic!("could not bind fixture server");
    };
    let fixture_url = format!("http://127.0.0.1:{port}/");

    let Some(ff) = LiveFirefox::headless_on_random_port() else {
        eprintln!(
            "live_eval_returns_window_scroll_y_on_csp_site: Firefox not available — skipping"
        );
        return;
    };

    let ff_args = || base_args(ff.port());

    let nav = Command::new(ff_rdp_bin())
        .args(ff_args())
        .args(["navigate", &fixture_url])
        .output()
        .expect("navigate");
    assert!(nav.status.success(), "navigate failed");

    // Scroll 100 px then read scrollY in a single eval.
    let out = Command::new(ff_rdp_bin())
        .args(ff_args())
        .args(["eval", "window.scrollTo(0, 100); window.scrollY"])
        .output()
        .expect("eval scrollY");

    assert!(
        out.status.success(),
        "eval exited non-zero — stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let json = parse_json(&out.stdout, &out.stderr);
    let scroll_y = json["results"]
        .as_f64()
        .expect("results must be a number (scrollY)");
    assert!(
        scroll_y >= 1.0,
        "scrollY must be >= 1 after scrollTo(0, 100), got {scroll_y}"
    );
}

/// `live_eval_script_error_still_surfaces`:
/// Eval `throw new Error("boom")` on the CSP fixture page.
/// Must exit non-zero and print the error message to stderr.
#[test]
#[ignore = "requires headless Firefox; set FF_RDP_LIVE_TESTS=1"]
fn live_eval_script_error_still_surfaces() {
    if std::env::var("FF_RDP_LIVE_TESTS").is_err() {
        eprintln!("live_eval_script_error_still_surfaces: set FF_RDP_LIVE_TESTS=1 to run");
        return;
    }

    let Some((port, _srv)) = spawn_csp_fixture_server() else {
        panic!("could not bind fixture server");
    };
    let fixture_url = format!("http://127.0.0.1:{port}/");

    let Some(ff) = LiveFirefox::headless_on_random_port() else {
        eprintln!("live_eval_script_error_still_surfaces: Firefox not available — skipping");
        return;
    };

    let ff_args = || base_args(ff.port());

    let nav = Command::new(ff_rdp_bin())
        .args(ff_args())
        .args(["navigate", &fixture_url])
        .output()
        .expect("navigate");
    assert!(nav.status.success(), "navigate failed");

    let out = Command::new(ff_rdp_bin())
        .args(ff_args())
        .args(["eval", "throw new Error(\"boom\")"])
        .output()
        .expect("eval throw");

    assert!(
        !out.status.success(),
        "eval must exit non-zero when JS throws"
    );

    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("Error") || stderr.contains("boom"),
        "stderr must contain 'Error' or 'boom'; got: {stderr}"
    );
}

/// `live_eval_works_on_real_mdn`:
/// Navigate to `https://developer.mozilla.org`, eval `document.title`,
/// assert the title contains `"MDN"`.
///
/// This is the original session-59 reproducer.
#[test]
#[ignore = "requires headless Firefox + network (developer.mozilla.org); set FF_RDP_LIVE_TESTS=1 and FF_RDP_LIVE_NETWORK_TESTS=1"]
fn live_eval_works_on_real_mdn() {
    if std::env::var("FF_RDP_LIVE_TESTS").is_err() {
        eprintln!("live_eval_works_on_real_mdn: set FF_RDP_LIVE_TESTS=1 to run");
        return;
    }
    if std::env::var("FF_RDP_LIVE_NETWORK_TESTS").is_err() {
        eprintln!("live_eval_works_on_real_mdn: set FF_RDP_LIVE_NETWORK_TESTS=1 to run");
        return;
    }

    let Some(ff) = LiveFirefox::headless_on_random_port() else {
        eprintln!("live_eval_works_on_real_mdn: Firefox not available — skipping");
        return;
    };

    let ff_args = || base_args(ff.port());

    let nav = Command::new(ff_rdp_bin())
        .args(ff_args())
        .args(["navigate", "https://developer.mozilla.org"])
        .output()
        .expect("navigate to MDN");

    if !nav.status.success() {
        eprintln!(
            "live_eval_works_on_real_mdn: navigate failed (network issue?) — {}",
            String::from_utf8_lossy(&nav.stderr)
        );
        return;
    }

    let out = Command::new(ff_rdp_bin())
        .args(ff_args())
        .args(["eval", "document.title"])
        .output()
        .expect("eval document.title on MDN");

    assert!(
        out.status.success(),
        "eval exited non-zero on MDN (CSP still blocking?) — stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let json = parse_json(&out.stdout, &out.stderr);
    let title = json["results"].as_str().unwrap_or("");
    assert!(
        title.contains("MDN"),
        "title must contain 'MDN'; got: {title:?}"
    );
}
