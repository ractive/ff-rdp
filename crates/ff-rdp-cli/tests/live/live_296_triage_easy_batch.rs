//! Live tests for iteration 296 — easy fixes from the 2026-10-26 triage.
//!
//! Each test drives a `data:` page (never a `file:` URL: content navigation to
//! `file:` crashes Firefox 158, which is the other half of this iteration).
//!
//! # Running
//!
//!   FF_RDP_LIVE_TESTS=1 cargo test -p ff-rdp-cli --test live live_296 -- --include-ignored

use std::process::{Command, Output};

use serde_json::Value;

use crate::common::{LiveFirefox, decode_png, ff_rdp_bin, live_tests_enabled};

fn run(port: u16, args: &[&str]) -> Output {
    Command::new(ff_rdp_bin())
        .args(["--host", "127.0.0.1", "--port", &port.to_string()])
        .args(["--timeout", "20000", "--allow-unsafe-urls"])
        .args(args)
        .output()
        .unwrap_or_else(|e| panic!("spawn ff-rdp {args:?}: {e}"))
}

fn run_json(port: u16, args: &[&str]) -> Value {
    let out = run(port, args);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success(),
        "command {args:?} failed: stdout={stdout} stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_str(stdout.trim())
        .unwrap_or_else(|e| panic!("output for {args:?} not JSON: {e}\n{stdout}"))
}

fn data_url(html: &str) -> String {
    format!(
        "data:text/html;charset=utf-8,{}",
        html.replace('%', "%25").replace('#', "%23")
    )
}

fn navigate(port: u16, html: &str) {
    run_json(port, &["navigate", &data_url(html)]);
}

const BANNER_PAGE: &str = "<!doctype html><html lang=en><head><title>Banner</title></head><body>\
    <header>Site header</header>\
    <nav><header>Menu header</header><a href=\"/a\">A</a></nav>\
    <main><h1>Repro</h1>\
    <section><header><h2>Section header</h2></header><footer>Section footer</footer></section>\
    <article><header><h2>Article header</h2></header><p>Text</p></article>\
    </main><footer>Site footer</footer></body></html>";

/// A page-level `<header>`/`<footer>` are the banner/contentinfo landmarks;
/// the ones scoped to `nav`/`section`/`article` are not (HTML-AAM).
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_296_a11y_summary_one_banner_for_nested_headers() {
    if !live_tests_enabled() {
        eprintln!("live_296_a11y_summary_one_banner_for_nested_headers: set FF_RDP_LIVE_TESTS=1");
        return;
    }
    let ff = LiveFirefox::headless_on_random_port();
    let port = ff.port();
    navigate(port, BANNER_PAGE);
    let summary = run_json(port, &["a11y", "summary"]);
    let roles: Vec<&str> = summary["results"]["landmarks"]
        .as_array()
        .unwrap_or_else(|| panic!("no landmarks: {summary}"))
        .iter()
        .filter_map(|l| l["role"].as_str())
        .collect();
    let count = |role: &str| roles.iter().filter(|r| **r == role).count();
    assert_eq!(count("banner"), 1, "{roles:?}");
    assert_eq!(count("contentinfo"), 1, "{roles:?}");
}

/// Headings come back in document order, not grouped by level.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_296_a11y_summary_headings_in_document_order() {
    if !live_tests_enabled() {
        eprintln!("live_296_a11y_summary_headings_in_document_order: set FF_RDP_LIVE_TESTS=1");
        return;
    }
    let ff = LiveFirefox::headless_on_random_port();
    let port = ff.port();
    navigate(
        port,
        "<!doctype html><title>Outline</title><main><h1>Title</h1><h2>A</h2><h3>A.1</h3>\
         <h3>A.2</h3><h2>B</h2><h3>B.1</h3></main>",
    );
    let summary = run_json(port, &["a11y", "summary"]);
    let outline: Vec<String> = summary["results"]["headings"]
        .as_array()
        .unwrap_or_else(|| panic!("no headings: {summary}"))
        .iter()
        .map(|h| format!("{}:{}", h["level"], h["text"].as_str().unwrap_or("")))
        .collect();
    assert_eq!(
        outline,
        ["1:Title", "2:A", "3:A.1", "3:A.2", "2:B", "3:B.1"],
        "{summary}"
    );
}

/// `page.title` is `document.title`; Readability's `og:title` preference
/// lands in `page.reader_title`.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_296_with_page_title_is_document_title() {
    if !live_tests_enabled() {
        eprintln!("live_296_with_page_title_is_document_title: set FF_RDP_LIVE_TESTS=1");
        return;
    }
    let ff = LiveFirefox::headless_on_random_port();
    let port = ff.port();
    let html = "<!doctype html><html lang=en><head><meta charset=utf-8><title>Document title</title>\
        <meta property=\"og:title\" content=\"OG title\"></head><body><main><h1>Repro</h1>\
        <p>Some paragraph text so the reader has an article to parse.</p></main></body></html>";
    let nav = run_json(port, &["navigate", &data_url(html), "--with-page"]);
    let page = &nav["results"]["page"];
    assert_eq!(page["title"], "Document title", "{page}");
    if let Some(reader_title) = page.get("reader_title") {
        assert_eq!(reader_title, "OG title", "{page}");
    }
}

const STICKY_PAGE: &str = "<!doctype html><html><head><meta charset=utf-8><title>sticky</title><style>\
    body{margin:0} header{position:sticky;top:0;height:72px;background:rgb(204,0,0);\
    display:flex;justify-content:space-between;align-items:center;padding:0 5vw}\
    .hero{height:300px;background:rgb(0,170,0)} .tall{height:3000px;background:rgb(200,200,200)}\
    </style></head><body><header><nav>Nav item</nav><button>Login</button></header>\
    <div class=hero></div><div class=tall></div></body></html>";

fn png_pixel(path: &std::path::Path, x: u32, y: u32) -> (u8, u8, u8) {
    let image = decode_png(&std::fs::read(path).expect("read png"));
    let (red, green, blue, _) = image
        .pixel(x, y)
        .unwrap_or_else(|| panic!("({x},{y}) outside {}x{}", image.width, image.height));
    (red, green, blue)
}

/// `--full-page` on a sticky header keeps the header full width and the
/// content below it, as the viewport capture shows them; the live layout is
/// restored afterwards.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_296_full_page_keeps_sticky_header_layout() {
    if !live_tests_enabled() {
        eprintln!("live_296_full_page_keeps_sticky_header_layout: set FF_RDP_LIVE_TESTS=1");
        return;
    }
    let ff = LiveFirefox::headless_on_random_port();
    let port = ff.port();
    navigate(port, STICKY_PAGE);
    let dir = tempfile::tempdir().expect("tempdir");
    let full = dir.path().join("full.png");
    let shot = run_json(
        port,
        &[
            "screenshot",
            "--full-page",
            "-o",
            full.to_str().expect("utf-8 path"),
        ],
    );
    let width = shot["results"]["width"].as_u64().expect("width");
    let height = shot["results"]["height"].as_u64().expect("height");
    assert!(
        height >= 3372,
        "full page must include header+hero+tall: {shot}"
    );
    let red = (204, 0, 0);
    let green = (0, 170, 0);
    let right = u32::try_from(width).expect("width fits") - 20;
    // Header spans the full width (it used to shrink-wrap to its content).
    assert_eq!(
        png_pixel(&full, right, 10),
        red,
        "header right edge: {shot}"
    );
    // Content starts below the 72px header (it used to slide up under it).
    assert_eq!(png_pixel(&full, 10, 60), red, "header at y=60: {shot}");
    assert_eq!(png_pixel(&full, 10, 80), green, "hero at y=80: {shot}");
    // The freeze is undone: the header is sticky again.
    let pos = run_json(
        port,
        &[
            "eval",
            "getComputedStyle(document.querySelector('header')).position",
        ],
    );
    assert_eq!(pos["results"], "sticky", "{pos}");
}

/// `--output-root DIR` without `-o` writes inside DIR, and `--window-size W×H
/// --full-page` writes a W-wide PNG taller than H.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_296_output_root_default_and_window_size_full_page() {
    if !live_tests_enabled() {
        eprintln!(
            "live_296_output_root_default_and_window_size_full_page: set FF_RDP_LIVE_TESTS=1"
        );
        return;
    }
    let ff = LiveFirefox::headless_on_random_port();
    let port = ff.port();
    navigate(port, STICKY_PAGE);
    let root = tempfile::tempdir().expect("tempdir");
    let root_str = root.path().to_str().expect("utf-8 path");
    let shot = run_json(port, &["screenshot", "--output-root", root_str]);
    let path = std::path::PathBuf::from(shot["results"]["path"].as_str().expect("path"));
    assert!(path.exists(), "{shot}");
    assert_eq!(
        path.parent().map(std::path::Path::to_path_buf),
        Some(root.path().canonicalize().expect("canonical root")),
        "{shot}"
    );

    let mobile = root.path().join("mobile.png");
    let shot = run_json(
        port,
        &[
            "screenshot",
            "--window-size",
            "390x844",
            "--full-page",
            "-o",
            mobile.to_str().expect("utf-8 path"),
        ],
    );
    assert_eq!(shot["results"]["width"], 390, "{shot}");
    assert!(
        shot["results"]["height"].as_u64().unwrap_or(0) > 844,
        "full-page batch capture must be taller than the window: {shot}"
    );
}
