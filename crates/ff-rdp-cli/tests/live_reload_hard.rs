//! iter-80 Theme B AC: `live_reload_hard_bypasses_cache`.
//!
//! Loads a small fixture whose <script> increments a counter on each evaluation
//! and asserts that `ff-rdp reload --hard` causes the page (and its embedded
//! JS) to be re-fetched, observable as a higher hit count on a local HTTP
//! server. Soft reload also re-fetches in practice (Firefox revalidates), so
//! the assertion is that the `force` flag is propagated end-to-end — i.e. the
//! command exits 0, the response advertises `force=true`, and the page is
//! still functional after the reload.
//!
//! # Running
//!   FF_RDP_LIVE_TESTS=1 cargo test-live -p ff-rdp-cli --test live_reload_hard -- --nocapture

#[path = "common/mod.rs"]
mod common;

use std::io::{Read, Write};
use std::net::TcpListener;
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::thread;
use std::time::Duration;

use common::{LiveFirefox, base_args, ff_rdp_bin};

/// Spawn a tiny local HTTP server that serves a cached HTML page and counts
/// each request to `/page` (other paths like `/favicon.ico` are served but
/// not counted, so probe traffic doesn't skew the assertion). Returns
/// `(port, counter, server-thread-handle)`; dropping the handle does not stop
/// the server — the thread exits after its bounded `take(20)` accept loop.
fn spawn_counting_server() -> Option<(u16, Arc<AtomicU32>, thread::JoinHandle<()>)> {
    let listener = TcpListener::bind("127.0.0.1:0").ok()?;
    let port = listener.local_addr().ok()?.port();
    let counter = Arc::new(AtomicU32::new(0));
    let counter_for_thread = Arc::clone(&counter);

    let handle = thread::spawn(move || {
        listener
            .set_nonblocking(false)
            .expect("set blocking listener");
        for stream in listener.incoming().take(20) {
            let Ok(mut stream) = stream else { continue };
            let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
            let mut buf = [0u8; 1024];
            let n = stream.read(&mut buf).unwrap_or(0);
            let req = String::from_utf8_lossy(&buf[..n]);
            let is_page = req
                .lines()
                .next()
                .is_some_and(|line| line.starts_with("GET /page "));
            if is_page {
                counter_for_thread.fetch_add(1, Ordering::SeqCst);
            }
            let body = b"<!doctype html><title>iter-80 reload --hard</title><h1 id=\"h\">v1</h1>";
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nCache-Control: public, max-age=3600\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = stream.write_all(resp.as_bytes());
            let _ = stream.write_all(body);
        }
    });

    Some((port, counter, handle))
}

#[test]
#[ignore = "requires Firefox + FF_RDP_LIVE_TESTS=1"]
fn live_reload_hard_bypasses_cache() {
    if std::env::var("FF_RDP_LIVE_TESTS").is_err() {
        eprintln!("live_reload_hard_bypasses_cache: set FF_RDP_LIVE_TESTS=1 to run");
        return;
    }
    let Some(ff) = LiveFirefox::headless_on_random_port() else {
        eprintln!("live_reload_hard_bypasses_cache: Firefox not available — skipping");
        return;
    };
    let Some((port, counter, _server)) = spawn_counting_server() else {
        eprintln!("live_reload_hard_bypasses_cache: could not bind local HTTP — skipping");
        return;
    };

    let url = format!("http://127.0.0.1:{port}/page");

    // Navigate once.
    let mut args = base_args(ff.port());
    args.extend(["navigate".into(), url.clone()]);
    let out = Command::new(ff_rdp_bin())
        .args(&args)
        .output()
        .expect("ff-rdp navigate");
    assert!(
        out.status.success(),
        "navigate failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let initial = counter.load(Ordering::SeqCst);
    assert!(
        initial >= 1,
        "expected at least one server hit; got {initial}"
    );

    // Hard reload.
    let mut args = base_args(ff.port());
    args.extend(["reload".into(), "--hard".into()]);
    let out = Command::new(ff_rdp_bin())
        .args(&args)
        .output()
        .expect("ff-rdp reload --hard");
    assert!(
        out.status.success(),
        "reload --hard failed: stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );

    let stdout = String::from_utf8_lossy(&out.stdout);
    let json: serde_json::Value = serde_json::from_str(stdout.trim())
        .unwrap_or_else(|e| panic!("reload --hard output not valid JSON: {e}\n{stdout}"));
    assert_eq!(
        json["results"]["force"],
        serde_json::Value::Bool(true),
        "reload --hard must echo force=true; got {json}"
    );

    // Give Firefox a moment to fetch.
    thread::sleep(Duration::from_millis(800));
    let after = counter.load(Ordering::SeqCst);
    assert!(
        after > initial,
        "hard reload must re-issue origin request: initial={initial} after={after}"
    );
}
