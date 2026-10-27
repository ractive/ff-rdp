//! Live tests: `a11y summary` and the bare `ff-rdp` home view list only the
//! interactive elements a user can see.
//!
//! The page models the comparis.ch repro: a consent box Consent-O-Matic had
//! faded to `opacity:0` held hundreds of vendor toggles that filled the
//! 50-entry cap ahead of the real navigation. The fixture adds the other ways
//! a subtree disappears — `display:none`, `visibility:hidden`, and a
//! `display:none` wrapper inside a shadow root that light-DOM links are
//! slotted through — plus the two cases that must stay: a custom checkbox
//! whose own `opacity:0` is covered by a visible label, and `aria-hidden`
//! content that is still on screen.
//!
//! # Running
//!
//!   FF_RDP_LIVE_TESTS=1 cargo test -p ff-rdp-cli --test live live_a11y_hidden -- --include-ignored

use std::fmt::Write as _;
use std::process::{Command, Output};

use serde_json::Value;

use crate::common::{LiveFirefox, ff_rdp_bin, live_tests_enabled};

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

/// 60 hidden vendor links ahead of the visible navigation, so without the
/// filter the default cap of 50 is all vendors.
fn page() -> String {
    let mut vendors = String::new();
    for i in 0..60 {
        write!(vendors, "<a href=\"#v{i}\">Vendor {i}</a>").expect("write to String");
    }
    format!(
        "<!doctype html><html lang=en><head><title>Hidden</title></head><body>\
         <div id=cmp style=\"opacity:0\">{vendors}<button>Hidden accept</button></div>\
         <div style=\"display:none\"><a href=\"/none\">Display none link</a></div>\
         <div style=\"visibility:hidden\"><a href=\"/vis\">Visibility hidden link</a></div>\
         <shadow-box><a href=\"/slotted\">Slotted hidden link</a></shadow-box>\
         <nav><a href=\"/home\">Visible home</a><a href=\"/about\">Visible about</a></nav>\
         <label><input type=checkbox name=terms style=\"opacity:0;position:absolute\">Accept terms</label>\
         <div aria-hidden=true><a href=\"/aria\">Aria hidden link</a></div>\
         <script>customElements.define('shadow-box', class extends HTMLElement {{\
           constructor() {{ super(); this.attachShadow({{mode: 'open'}}).innerHTML =\
             '<div style=\"display:none\"><slot></slot></div>'; }} }});</script>\
         </body></html>"
    )
}

fn names(view: &Value) -> Vec<String> {
    view["interactive"]
        .as_array()
        .unwrap_or_else(|| panic!("no interactive list: {view}"))
        .iter()
        .map(|e| e["name"].as_str().unwrap_or_default().to_owned())
        .collect()
}

fn assert_only_visible(names: &[String], context: &str) {
    for hidden in [
        "Vendor 0",
        "Hidden accept",
        "Display none link",
        "Visibility hidden link",
        "Slotted hidden link",
    ] {
        assert!(
            !names.iter().any(|n| n == hidden),
            "{context}: hidden {hidden:?} listed: {names:?}"
        );
    }
    for shown in [
        "Visible home",
        "Visible about",
        "Accept terms",
        "Aria hidden link",
    ] {
        assert!(
            names.iter().any(|n| n == shown),
            "{context}: visible {shown:?} missing: {names:?}"
        );
    }
}

#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_a11y_hidden_interactive_excluded_from_summary_and_home() {
    if !live_tests_enabled() {
        eprintln!(
            "live_a11y_hidden_interactive_excluded_from_summary_and_home: set FF_RDP_LIVE_TESTS=1"
        );
        return;
    }
    let ff = LiveFirefox::headless_on_random_port();
    let port = ff.port();
    run_json(port, &["navigate", &data_url(&page())]);

    let summary = run_json(port, &["a11y", "summary"]);
    let results = &summary["results"];
    let listed = names(results);
    assert_only_visible(&listed, "a11y summary");
    assert_eq!(
        listed.len(),
        4,
        "only the four visible controls: {listed:?}"
    );
    assert!(
        results.get("interactive_truncated").is_none(),
        "hidden vendors must not reach the cap: {summary}"
    );

    let home = run_json(port, &["home", "--format", "json"]);
    assert_only_visible(&names(&home["results"]["page"]), "home view");
}
