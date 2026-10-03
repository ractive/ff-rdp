//! Live tests for per-command emulation: `screenshot --color-scheme`,
//! `screenshot --media print` and `navigate --user-agent`.
//!
//! Each setting lives on the command's own RDP connection, so every test
//! asserts the effect inside that command (the captured pixels, the request
//! header, a title the page wrote while loading) and then that a later command
//! sees the unemulated page — the setting ended with the connection.
//!
//! # Running
//!
//!   FF_RDP_LIVE_TESTS=1 cargo test -p ff-rdp-cli --test live live_emulation -- --include-ignored

use std::io::{Read as _, Write as _};
use std::net::TcpListener;
use std::process::Command;
use std::time::Duration;

use base64::Engine as _;
use serde_json::Value;

use crate::common::{LiveFirefox, base_args, decode_png, ff_rdp_bin, live_tests_enabled};

/// White normally, black under `prefers-color-scheme: dark`, red in print.
const PAGE: &str = "data:text/html,<style>body{margin:0;height:100vh;background:%23fff}\
    @media (prefers-color-scheme: dark){body{background:%23000}}\
    @media print{body{background:%23f00}}</style><body></body>";

fn run(port: u16, args: &[&str]) -> Value {
    let out = Command::new(ff_rdp_bin())
        .args(base_args(port))
        .arg("--allow-unsafe-urls")
        .args(args)
        .output()
        .expect("spawn ff-rdp");
    assert!(
        out.status.success(),
        "{args:?} failed: stdout={} stderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).unwrap_or_else(|e| panic!("{args:?}: stdout not JSON: {e}"))
}

/// Capture with `extra` flags and return the RGB of the viewport's centre.
fn centre_pixel(port: u16, extra: &[&str]) -> (u8, u8, u8) {
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
    let (r, g, b, _) = image
        .pixel(image.width / 2, image.height / 2)
        .expect("centre pixel");
    (r, g, b)
}

fn eval_bool(port: u16, js: &str) -> bool {
    let v = run(port, &["eval", js]);
    v["results"]
        .as_bool()
        .unwrap_or_else(|| panic!("{js}: not a bool: {v}"))
}

#[test]
#[ignore = "requires Firefox — set FF_RDP_LIVE_TESTS=1"]
fn live_screenshot_color_scheme_dark_paints_dark_styles() {
    if !live_tests_enabled() {
        return;
    }
    let ff = LiveFirefox::headless_on_random_port();
    let port = ff.port();
    run(port, &["navigate", PAGE]);

    assert_eq!(
        centre_pixel(port, &[]),
        (255, 255, 255),
        "baseline is white"
    );
    assert_eq!(
        centre_pixel(port, &["--color-scheme", "dark"]),
        (0, 0, 0),
        "--color-scheme dark must capture the dark styles"
    );
    assert!(
        !eval_bool(port, "matchMedia('(prefers-color-scheme: dark)').matches"),
        "the simulation must end with the screenshot's connection"
    );
    assert_eq!(
        centre_pixel(port, &[]),
        (255, 255, 255),
        "white again after"
    );
}

#[test]
#[ignore = "requires Firefox — set FF_RDP_LIVE_TESTS=1"]
fn live_screenshot_media_print_paints_print_styles() {
    if !live_tests_enabled() {
        return;
    }
    let ff = LiveFirefox::headless_on_random_port();
    let port = ff.port();
    run(port, &["navigate", PAGE]);

    let shot = centre_pixel(port, &["--media", "print"]);
    assert_eq!(shot, (255, 0, 0), "--media print must capture @media print");
    assert!(
        !eval_bool(port, "matchMedia('print').matches"),
        "the simulation must end with the screenshot's connection"
    );
}

const UA: &str = "ff-rdp-live-UA/1.0 (emulation test)";

/// Serve one HTTP request on `listener` with a page that writes
/// `navigator.userAgent` into its title, returning the request head.
fn serve_ua_page(listener: TcpListener) -> std::thread::JoinHandle<String> {
    std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept");
        stream
            .set_read_timeout(Some(Duration::from_secs(10)))
            .expect("read timeout");
        let mut head = Vec::new();
        let mut buf = [0u8; 1024];
        while !head.windows(4).any(|w| w == b"\r\n\r\n") {
            let n = stream.read(&mut buf).expect("read request");
            if n == 0 {
                break;
            }
            head.extend_from_slice(&buf[..n]);
        }
        let body = "<script>document.title = navigator.userAgent</script>";
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\n\
             Connection: close\r\n\r\n{body}",
            body.len()
        );
        stream.write_all(response.as_bytes()).expect("write");
        String::from_utf8_lossy(&head).into_owned()
    })
}

#[test]
#[ignore = "requires Firefox — set FF_RDP_LIVE_TESTS=1"]
fn live_navigate_user_agent_sends_header_and_navigator_value() {
    if !live_tests_enabled() {
        return;
    }
    let ff = LiveFirefox::headless_on_random_port();
    let port = ff.port();
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let url = format!(
        "http://127.0.0.1:{}/",
        listener.local_addr().unwrap().port()
    );
    let server = serve_ua_page(listener);

    let nav = run(port, &["navigate", &url, "--user-agent", UA]);
    assert_eq!(nav["results"]["user_agent"], UA, "{nav}");

    let head = server.join().expect("server thread");
    let header = head
        .lines()
        .find_map(|l| {
            l.split_once(':')
                .filter(|(name, _)| name.eq_ignore_ascii_case("user-agent"))
                .map(|(_, value)| value.trim().to_owned())
        })
        .unwrap_or_else(|| panic!("no User-Agent header: {head}"));
    assert_eq!(header, UA, "the document request must carry the override");

    let title = run(port, &["eval", "document.title"]);
    assert_eq!(
        title["results"], UA,
        "the page must have seen the override in navigator.userAgent"
    );
    assert!(
        !eval_bool(port, &format!("navigator.userAgent === {UA:?}")),
        "the override must end with the navigate command's connection"
    );
}
