//! Live tests for `screenshot --viewport WxH` (the iframe-wrapper phone-width
//! capture on the live tab).
//!
//! A local HTTP server serves a page whose body is blue, green under
//! `@media (max-width: 480px)` and magenta when that is combined with
//! `prefers-color-scheme: dark`; the live window is wider than 480 px, so
//! only a capture laid out at the requested width shows green. The same page
//! is served with `X-Frame-Options: DENY`, with CSP `frame-ancestors 'none'`
//! and with CSP `frame-src 'none'` (the page may not frame anything, itself
//! included) to prove the refused-framing errors.
//!
//! # Running
//!
//!   FF_RDP_LIVE_TESTS=1 cargo test -p ff-rdp-cli --test live live_screenshot_viewport -- --include-ignored

use std::io::{Read as _, Write as _};
use std::net::{TcpListener, TcpStream};
use std::process::Command;
use std::time::Duration;

use base64::Engine as _;
use serde_json::Value;

use crate::common::{LiveFirefox, base_args, decode_png, ff_rdp_bin, live_tests_enabled};

const PAGE: &str = "<!doctype html><html><head><style>\
    body{margin:0;height:3000px;background:#00f}\
    @media (max-width:480px){body{background:#0f0}}\
    @media (max-width:480px) and (prefers-color-scheme:dark){body{background:#f0f}}\
    img{display:block;margin-top:600px;width:100px;height:100px}\
    </style></head><body><img loading=lazy src=\"/img.svg\" alt=\"\"></body></html>";

const SVG: &str = "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"100\" height=\"100\">\
    <rect width=\"100\" height=\"100\" fill=\"#f00\"/></svg>";

fn respond(mut stream: TcpStream) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(10)));
    let mut head = Vec::new();
    let mut buf = [0u8; 1024];
    while !head.windows(4).any(|w| w == b"\r\n\r\n") {
        match stream.read(&mut buf) {
            Ok(0) | Err(_) => return,
            Ok(n) => head.extend_from_slice(&buf[..n]),
        }
    }
    let head = String::from_utf8_lossy(&head);
    let path = head.split_whitespace().nth(1).unwrap_or("/");
    let (content_type, body, extra) = match path {
        "/img.svg" => ("image/svg+xml", SVG, ""),
        "/deny" => ("text/html", PAGE, "X-Frame-Options: DENY\r\n"),
        "/no-ancestors" => (
            "text/html",
            PAGE,
            "Content-Security-Policy: frame-ancestors 'none'\r\n",
        ),
        "/no-frame-src" => (
            "text/html",
            PAGE,
            "Content-Security-Policy: frame-src 'none'\r\n",
        ),
        _ => ("text/html", PAGE, ""),
    };
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\n\
         Cache-Control: no-store\r\n{extra}Connection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.write_all(response.as_bytes());
}

/// Serve [`PAGE`] and friends on an ephemeral port; returns the base URL.
fn serve() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let base = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            std::thread::spawn(move || respond(stream));
        }
    });
    base
}

fn ff_rdp(port: u16, args: &[&str]) -> std::process::Output {
    Command::new(ff_rdp_bin())
        .args(base_args(port))
        .args(args)
        .output()
        .expect("spawn ff-rdp")
}

fn run(port: u16, args: &[&str]) -> Value {
    let out = ff_rdp(port, args);
    assert!(
        out.status.success(),
        "{args:?} failed: stdout={} stderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).unwrap_or_else(|e| panic!("{args:?}: stdout not JSON: {e}"))
}

fn wrappers_left(port: u16) -> u64 {
    let v = run(
        port,
        &[
            "eval",
            "document.querySelectorAll('[data-ffrdp-viewport]').length",
        ],
    );
    v["results"]
        .as_u64()
        .unwrap_or_else(|| panic!("not a number: {v}"))
}

/// `screenshot --base64 <extra>`: the envelope `results` and the decoded PNG.
fn shoot(port: u16, extra: &[&str]) -> (Value, crate::common::DecodedImage) {
    let mut args = vec!["screenshot", "--base64"];
    args.extend_from_slice(extra);
    let shot = run(port, &args);
    let b64 = shot["results"]["base64"]
        .as_str()
        .unwrap_or_else(|| panic!("no base64: {shot}"));
    let png = base64::engine::general_purpose::STANDARD
        .decode(b64)
        .expect("base64");
    let image = decode_png(&png);
    (shot["results"].clone(), image)
}

fn rgb(image: &crate::common::DecodedImage, x: u32, y: u32) -> (u8, u8, u8) {
    let pixel = image
        .pixel(x, y)
        .unwrap_or_else(|| panic!("no pixel at {x},{y}"));
    (pixel.0, pixel.1, pixel.2)
}

#[test]
#[ignore = "requires Firefox — set FF_RDP_LIVE_TESTS=1"]
fn live_screenshot_viewport_lays_out_at_the_requested_width() {
    if !live_tests_enabled() {
        return;
    }
    let ff = LiveFirefox::headless_on_random_port();
    let port = ff.port();
    let base = serve();
    run(port, &["navigate", &format!("{base}/")]);
    // Scroll the live tab: the capture rect must follow the wrapper, and the
    // tab's scroll position must survive the command.
    run(port, &["eval", "window.scrollTo(0, 300); window.scrollY"]);

    let (_, live) = shoot(port, &[]);
    assert_eq!(
        rgb(&live, live.width / 2, live.height / 2),
        (0, 0, 255),
        "the live window is wider than 480 px, so the page is blue there"
    );

    let (results, image) = shoot(port, &["--viewport", "390x844"]);
    let dppx = results["viewport"]["dppx"]
        .as_f64()
        .unwrap_or_else(|| panic!("no dppx: {results}"));
    assert_eq!(results["capture"], "live-viewport-iframe", "{results}");
    assert_eq!(f64::from(image.width), (390.0 * dppx).round(), "{results}");
    assert_eq!(f64::from(image.height), (844.0 * dppx).round(), "{results}");
    assert_eq!(results["images_settled"], true, "{results}");
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let px = |x: f64, y: f64| rgb(&image, (x * dppx) as u32, (y * dppx) as u32);
    assert_eq!(
        px(200.0, 300.0),
        (0, 255, 0),
        "(max-width: 480px) must evaluate against the 390 px frame"
    );
    assert_eq!(
        px(50.0, 650.0),
        (255, 0, 0),
        "the lazy image in the first viewport must be loaded"
    );

    assert_eq!(wrappers_left(port), 0, "the wrapper must be removed");
    let scroll = run(port, &["eval", "window.scrollY"]);
    assert_eq!(
        scroll["results"], 300,
        "the tab's scroll position is untouched"
    );
}

/// An open modal `<dialog>` (and its `::backdrop`) is in the top layer; the
/// wrapper must go above it, or the live dialog paints over the capture.
#[test]
#[ignore = "requires Firefox — set FF_RDP_LIVE_TESTS=1"]
fn live_screenshot_viewport_sits_above_an_open_modal_dialog() {
    if !live_tests_enabled() {
        return;
    }
    let ff = LiveFirefox::headless_on_random_port();
    let port = ff.port();
    let base = serve();
    run(port, &["navigate", &format!("{base}/")]);
    run(
        port,
        &[
            "eval",
            "document.body.insertAdjacentHTML('beforeend', '<dialog id=dlg style=\"background:#ff0;\
             width:1000px;height:400px\">dialog</dialog>'); dlg.showModal(); dlg.open",
        ],
    );

    let (results, image) = shoot(port, &["--viewport", "390x844"]);
    let dppx = results["viewport"]["dppx"].as_f64().unwrap_or(1.0);
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let centre = rgb(&image, (200.0 * dppx) as u32, (300.0 * dppx) as u32);
    assert_eq!(
        centre,
        (0, 255, 0),
        "the frame must paint above the live modal dialog and its backdrop"
    );
    assert_eq!(wrappers_left(port), 0, "the wrapper must be removed");
    let open = run(port, &["eval", "dlg.open"]);
    assert_eq!(open["results"], true, "the live dialog stays open");
}

#[test]
#[ignore = "requires Firefox — set FF_RDP_LIVE_TESTS=1"]
fn live_screenshot_viewport_composes_with_color_scheme() {
    if !live_tests_enabled() {
        return;
    }
    let ff = LiveFirefox::headless_on_random_port();
    let port = ff.port();
    let base = serve();
    run(port, &["navigate", &format!("{base}/")]);

    let (results, image) = shoot(port, &["--viewport", "390x844", "--color-scheme", "dark"]);
    assert_eq!(results["emulation"]["color_scheme"], "dark", "{results}");
    assert_eq!(
        results["emulation"]["settled"], true,
        "the settle wait runs before the frame is injected: {results}"
    );
    assert_eq!(
        rgb(&image, image.width / 2, 10),
        (255, 0, 255),
        "dark + narrow styles must both apply inside the frame"
    );
    assert_eq!(wrappers_left(port), 0, "the wrapper must be removed");
}

#[test]
#[ignore = "requires Firefox — set FF_RDP_LIVE_TESTS=1"]
fn live_screenshot_viewport_reports_refused_framing() {
    if !live_tests_enabled() {
        return;
    }
    let ff = LiveFirefox::headless_on_random_port();
    let port = ff.port();
    let base = serve();
    for (path, expected) in [
        ("/deny", "X-Frame-Options: DENY"),
        ("/no-ancestors", "frame-ancestors 'none'"),
        ("/no-frame-src", "own Content-Security-Policy (frame-src)"),
    ] {
        run(port, &["navigate", &format!("{base}{path}")]);
        let out = ff_rdp(port, &["screenshot", "--viewport", "390x844", "--base64"]);
        // The error envelope may land on either stream; check both.
        let stderr = format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(!out.status.success(), "{path}: must fail, output={stderr}");
        assert!(stderr.contains(expected), "{path}: {stderr}");
        assert!(stderr.contains("--window-size"), "{path}: {stderr}");
        assert_eq!(
            wrappers_left(port),
            0,
            "{path}: the wrapper must be removed"
        );
    }
}
