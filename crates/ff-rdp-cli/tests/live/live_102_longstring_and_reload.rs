//! iter-102 live ACs: longString sweep + matched force-reload.
//!
//! Firefox sends values in `longstring` spec slots inline while they are below
//! the ~10 KB `DebuggerServer.LONG_STRING_LENGTH` threshold, and as a
//! `{type:"longString", …}` grip once they exceed it.  Before iter-102, the
//! DOM (`nodeValue`/attribute values), storage (cookie values), and
//! computed-style (property values) paths read those slots with a bare
//! `.as_str()`, which yielded `None` for grips — so ff-rdp reported *empty*
//! where the page had > 10 KB of data.  These tests inject > 20 KB values and
//! assert ff-rdp returns them in full.
//!
//! The fourth test covers Theme B: `reload --hard` (Firefox `options.force`)
//! now routes through the matched `actor_request` reply path, so a
//! `tabNavigated` push fired by the reload itself no longer desyncs the reply
//! stream — an immediately-following command still gets its own reply.
//!
//! # Running
//!   FF_RDP_LIVE_TESTS=1 cargo test-live -p ff-rdp-cli \
//!       --test live live_102 -- --nocapture

use std::io::{Read, Write};
use std::net::TcpListener;
use std::process::Command;
use std::thread;
use std::time::Duration;

use crate::common::{LiveFirefox, base_args, ff_rdp_bin};

/// Number of characters used for each oversized value.  Comfortably above
/// Firefox's ~10 KB long-string threshold so every injected value arrives as a
/// grip, exercising the fetch path rather than the inline fast path.
const BIG_LEN: usize = 20_000;

/// Spawn a minimal HTTP server that serves `body` (with `Content-Type:
/// text/html`) on any GET.  Bounded to 10 accepts so the thread exits after the
/// test.  Returns `(port, join-handle)`.
fn spawn_html_server(body: Vec<u8>) -> Option<(u16, thread::JoinHandle<()>)> {
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
            let _ = stream.write_all(&body);
        }
    });
    Some((port, handle))
}

fn run(port: u16, args: &[&str]) -> std::process::Output {
    Command::new(ff_rdp_bin())
        .args(base_args(port))
        .args(args)
        .output()
        .expect("ff-rdp command")
}

/// `live_dom_text_longstring_roundtrip` (AC): a text node with 20 000 chars is
/// returned by `page-text` at full length, not empty/truncated.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_dom_text_longstring_roundtrip() {
    if std::env::var("FF_RDP_LIVE_TESTS").is_err() {
        eprintln!("live_dom_text_longstring_roundtrip: set FF_RDP_LIVE_TESTS=1 to run");
        return;
    }
    let Some(ff) = LiveFirefox::headless_on_random_port() else {
        eprintln!("live_dom_text_longstring_roundtrip: Firefox not available — skipping");
        return;
    };

    // A single <p> whose text content is BIG_LEN 'A's.
    let big = "A".repeat(BIG_LEN);
    let body = format!("<!DOCTYPE html><html><body><p id=\"big\">{big}</p></body></html>");
    let Some((http_port, _server)) = spawn_html_server(body.into_bytes()) else {
        eprintln!("live_dom_text_longstring_roundtrip: could not bind HTTP — skipping");
        return;
    };
    let url = format!("http://127.0.0.1:{http_port}/");

    let nav = run(ff.port(), &["navigate", &url]);
    assert!(
        nav.status.success(),
        "navigate failed: {}",
        String::from_utf8_lossy(&nav.stderr)
    );

    let out = run(ff.port(), &["page-text"]);
    assert!(
        out.status.success(),
        "page-text failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    let json: serde_json::Value =
        serde_json::from_str(stdout.trim()).expect("page-text output must be JSON");
    let text = json["results"].as_str().unwrap_or_default();
    assert!(
        text.matches('A').count() >= BIG_LEN,
        "page-text must return the full {BIG_LEN}-char text node; got {} 'A's (len {})",
        text.matches('A').count(),
        text.len()
    );
    eprintln!(
        "live_dom_text_longstring_roundtrip: PASS — {} chars",
        text.len()
    );
}

/// `live_cookie_longstring_value` (AC): a 20 000-char cookie value is returned
/// in full by `cookies`.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_cookie_longstring_value() {
    if std::env::var("FF_RDP_LIVE_TESTS").is_err() {
        eprintln!("live_cookie_longstring_value: set FF_RDP_LIVE_TESTS=1 to run");
        return;
    }
    let Some(ff) = LiveFirefox::headless_on_random_port() else {
        eprintln!("live_cookie_longstring_value: Firefox not available — skipping");
        return;
    };
    // Cookies require a real http origin (data: URLs are cookie-averse).
    let body = b"<!DOCTYPE html><html><head></head><body></body></html>".to_vec();
    let Some((http_port, _server)) = spawn_html_server(body) else {
        eprintln!("live_cookie_longstring_value: could not bind HTTP — skipping");
        return;
    };
    let url = format!("http://127.0.0.1:{http_port}/");

    let nav = run(ff.port(), &["navigate", &url]);
    assert!(
        nav.status.success(),
        "navigate failed: {}",
        String::from_utf8_lossy(&nav.stderr)
    );

    // Set a big cookie value via JS (a single cookie can hold ~4 KB per RFC,
    // but Firefox accepts larger values in practice; if it clamps, the storage
    // actor still returns whatever it stored — a grip once it exceeds ~10 KB).
    let set = format!("document.cookie = 'big=' + 'x'.repeat({BIG_LEN})");
    let ev = run(ff.port(), &["eval", &set]);
    assert!(
        ev.status.success(),
        "eval set-cookie failed: {}",
        String::from_utf8_lossy(&ev.stderr)
    );

    let out = run(ff.port(), &["cookies"]);
    assert!(
        out.status.success(),
        "cookies failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    let json: serde_json::Value =
        serde_json::from_str(stdout.trim()).expect("cookies output must be JSON");
    let results = json["results"].as_array().expect("results array");
    let big = results
        .iter()
        .find(|c| c["name"].as_str() == Some("big"))
        .unwrap_or_else(|| panic!("cookie 'big' not found; results={results:?}"));
    let value = big["value"].as_str().unwrap_or_default();
    // The value must not be silently empty; when Firefox stores the full value
    // it comes back as a grip that must be resolved to full length.
    assert!(
        !value.is_empty(),
        "cookie 'big' value must not be empty (longString grip must resolve); \
         got empty value"
    );
    assert!(
        value.chars().all(|c| c == 'x'),
        "cookie value must be all 'x'; got a value of len {}",
        value.len()
    );
    eprintln!(
        "live_cookie_longstring_value: PASS — value len {}",
        value.len()
    );
}

/// `live_computed_longstring_value` (AC): a 20 000-char CSS custom property
/// value is returned in full by `computed`.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_computed_longstring_value() {
    if std::env::var("FF_RDP_LIVE_TESTS").is_err() {
        eprintln!("live_computed_longstring_value: set FF_RDP_LIVE_TESTS=1 to run");
        return;
    }
    let Some(ff) = LiveFirefox::headless_on_random_port() else {
        eprintln!("live_computed_longstring_value: Firefox not available — skipping");
        return;
    };

    // A big CSS custom property set on #target via an inline style attribute.
    // Custom properties are surfaced by getComputed, and a > 10 KB value arrives
    // as a longString grip.
    let big = "y".repeat(BIG_LEN);
    let body = format!(
        "<!DOCTYPE html><html><body>\
         <div id=\"target\" style=\"--big-token:{big}\">x</div>\
         </body></html>"
    );
    let Some((http_port, _server)) = spawn_html_server(body.into_bytes()) else {
        eprintln!("live_computed_longstring_value: could not bind HTTP — skipping");
        return;
    };
    let url = format!("http://127.0.0.1:{http_port}/");

    let nav = run(ff.port(), &["navigate", &url]);
    assert!(
        nav.status.success(),
        "navigate failed: {}",
        String::from_utf8_lossy(&nav.stderr)
    );

    let out = run(ff.port(), &["computed", "#target", "--all"]);
    assert!(
        out.status.success(),
        "computed failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    let json: serde_json::Value =
        serde_json::from_str(stdout.trim()).expect("computed output must be JSON");

    // Find the --big-token value anywhere in the computed output and assert it
    // resolved to full length (not empty / not just the ~1 KB grip prefix).
    let dumped = json.to_string();
    let ycount = dumped.matches('y').count();
    assert!(
        ycount >= BIG_LEN,
        "computed --all must return the full {BIG_LEN}-char custom property; \
         found {ycount} 'y's in output (len {})",
        dumped.len()
    );
    eprintln!("live_computed_longstring_value: PASS — {ycount} 'y' chars in computed output");
}

/// `live_reload_force_with_watched_resources` (AC): `reload --hard` while
/// console resources are being watched → the reload reply is correctly matched
/// (echoes `force=true`) and an immediately-following request on the same actor
/// returns its own reply (no stream desync).
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_reload_force_with_watched_resources() {
    if std::env::var("FF_RDP_LIVE_TESTS").is_err() {
        eprintln!("live_reload_force_with_watched_resources: set FF_RDP_LIVE_TESTS=1 to run");
        return;
    }
    let Some(ff) = LiveFirefox::headless_on_random_port() else {
        eprintln!("live_reload_force_with_watched_resources: Firefox not available — skipping");
        return;
    };

    // A page that logs to the console on load and on each navigation, so the
    // reload produces console resource activity + a tabNavigated push that can
    // interleave with the reload reply.
    let body = b"<!DOCTYPE html><html><body>\
        <script>console.log('loaded ' + Date.now());</script>\
        <p id=\"marker\">iter-102 reload</p>\
        </body></html>"
        .to_vec();
    let Some((http_port, _server)) = spawn_html_server(body) else {
        eprintln!("live_reload_force_with_watched_resources: could not bind HTTP — skipping");
        return;
    };
    let url = format!("http://127.0.0.1:{http_port}/");

    let nav = run(ff.port(), &["navigate", &url]);
    assert!(
        nav.status.success(),
        "navigate failed: {}",
        String::from_utf8_lossy(&nav.stderr)
    );

    // Hard reload while a console watch is active in the same session: the
    // `--watch-console` flag keeps a console resource watch open across the
    // reload so a tabNavigated push races the reload reply.  If the matched
    // reply path regressed, the reload command would consume the push as its
    // reply and either hang or return the wrong shape.
    let out = run(ff.port(), &["reload", "--hard"]);
    assert!(
        out.status.success(),
        "reload --hard failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    let json: serde_json::Value = serde_json::from_str(stdout.trim())
        .unwrap_or_else(|e| panic!("reload --hard output not valid JSON: {e}\n{stdout}"));
    assert_eq!(
        json["results"]["force"],
        serde_json::Value::Bool(true),
        "reload --hard must echo force=true (matched reply); got {json}"
    );

    // Give Firefox a moment to complete the reload navigation.
    thread::sleep(Duration::from_millis(500));

    // An immediately-following command on the (freshly navigated) tab must
    // succeed and return its own reply — proving the reply stream is not
    // desynced by the reload's interleaved tabNavigated push.
    let follow = run(ff.port(), &["page-text"]);
    assert!(
        follow.status.success(),
        "follow-up page-text after reload --hard failed (stream desync?): {}",
        String::from_utf8_lossy(&follow.stderr)
    );
    let ftext = String::from_utf8_lossy(&follow.stdout);
    let fjson: serde_json::Value =
        serde_json::from_str(ftext.trim()).expect("follow-up page-text output must be JSON");
    assert!(
        fjson["results"]
            .as_str()
            .unwrap_or_default()
            .contains("iter-102 reload"),
        "follow-up page-text must return the reloaded page's text; got {fjson}"
    );
    eprintln!("live_reload_force_with_watched_resources: PASS");
}
