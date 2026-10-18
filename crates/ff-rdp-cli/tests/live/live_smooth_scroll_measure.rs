//! Scroll-then-measure on pages with CSS `scroll-behavior: smooth`
//! (backlog issue `click-offscreen-with-smooth-scroll`).
//!
//! `click`'s hit test, `scroll text`, `scroll top/bottom` and `scroll container`
//! all scroll and then read a position in the same JS turn. Without
//! `behavior: 'instant'` the page's `scroll-behavior: smooth` turns that scroll
//! into an animation, and the read sees the pre-animation position: `click`
//! reported `click_offscreen` on the first try and `scroll text` returned a
//! rect that was still below the fold.
use std::process::Command;

use crate::common::{LiveFirefox, base_args, ff_rdp_bin, live_tests_enabled, output_note};

/// `html` and the `.box` container both scroll smoothly; the button and the
/// needle text sit ~2000px below the fold.
const SMOOTH_PAGE: &str = "data:text/html,<!doctype html><style>html{scroll-behavior:smooth}\
body{margin:0}.box{height:200px;overflow:auto;scroll-behavior:smooth}</style>\
<div class=box><div style='height:2000px'>inner</div></div>\
<div style='height:2000px'>spacer</div>\
<button onclick=\"window.__clicks=(window.__clicks||0)+1\">Far button</button>\
<p>smooth-needle-text</p><div style='height:2000px'>tail</div>";

fn run(port: u16, args: &[&str]) -> (bool, serde_json::Value, String) {
    let out = Command::new(ff_rdp_bin())
        .args(base_args(port))
        .args(args)
        .output()
        .expect("spawn ff-rdp");
    let note = output_note(&out);
    let json = serde_json::from_slice(&out.stdout).unwrap_or(serde_json::Value::Null);
    (out.status.success(), json, note)
}

fn eval_f64(port: u16, expr: &str) -> f64 {
    let (ok, json, note) = run(port, &["eval", expr]);
    assert!(ok, "eval {expr}: {note}");
    json["results"]
        .as_f64()
        .unwrap_or_else(|| panic!("eval {expr} must return a number: {json}"))
}

fn open_smooth_page(port: u16) {
    let (ok, _, note) = run(port, &["navigate", "--allow-unsafe-urls", SMOOTH_PAGE]);
    assert!(ok, "navigate: {note}");
    assert_eq!(
        eval_f64(port, "window.scrollY"),
        0.0,
        "page must start at the top"
    );
}

#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_smooth_scroll_click_far_below_fold_succeeds_first_try() {
    if !live_tests_enabled() {
        return;
    }
    let ff = LiveFirefox::headless_on_random_port();
    let port = ff.port();
    open_smooth_page(port);

    let (ok, json, note) = run(port, &["click", "button"]);
    assert!(
        ok,
        "first click on a smooth-scroll page must not report click_offscreen: {note}"
    );
    assert_eq!(json["results"]["clicked"], true, "{json}");
    assert_eq!(json["results"]["offscreen"], false, "{json}");
    assert_eq!(
        eval_f64(port, "window.__clicks || 0"),
        1.0,
        "the button's handler must have run exactly once"
    );
}

#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_smooth_scroll_text_returns_settled_rect() {
    if !live_tests_enabled() {
        return;
    }
    let ff = LiveFirefox::headless_on_random_port();
    let port = ff.port();
    open_smooth_page(port);

    let (ok, json, note) = run(port, &["scroll", "text", "smooth-needle-text"]);
    assert!(ok, "scroll text: {note}");
    let r = &json["results"];
    let top = r["target"]["rect"]["top"].as_f64().expect("rect.top");
    let height = r["target"]["rect"]["height"].as_f64().expect("rect.height");
    let vh = r["viewport"]["height"].as_f64().expect("viewport.height");
    let centre = top + height / 2.0;
    assert!(
        (centre - vh / 2.0).abs() <= 2.0,
        "rect must be the settled, centred one (centre {centre}, viewport {vh}): {json}"
    );
    // The reported scroll position is where the page actually is.
    let reported_y = r["viewport"]["y"].as_f64().expect("viewport.y");
    assert!(reported_y > 1000.0, "{json}");
    assert_eq!(eval_f64(port, "window.scrollY"), reported_y);
}

#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_smooth_scroll_bottom_and_container_report_settled_position() {
    if !live_tests_enabled() {
        return;
    }
    let ff = LiveFirefox::headless_on_random_port();
    let port = ff.port();
    open_smooth_page(port);

    let (ok, json, note) = run(port, &["scroll", "bottom"]);
    assert!(ok, "scroll bottom: {note}");
    assert_eq!(json["results"]["atEnd"], true, "{json}");
    let y = json["results"]["viewport"]["y"]
        .as_f64()
        .expect("viewport.y");
    assert!(
        y > 1000.0,
        "scroll bottom must report the settled position: {json}"
    );
    assert_eq!(eval_f64(port, "window.scrollY"), y);

    let (ok, json, note) = run(port, &["scroll", "container", ".box", "--to-end"]);
    assert!(ok, "scroll container: {note}");
    assert_eq!(json["results"]["atEnd"], true, "{json}");
    let after = json["results"]["after"]["scrollTop"]
        .as_f64()
        .expect("after.scrollTop");
    assert!(
        after > 1000.0,
        "container must report its settled scrollTop: {json}"
    );
    assert_eq!(
        eval_f64(port, "document.querySelector('.box').scrollTop"),
        after
    );
}
