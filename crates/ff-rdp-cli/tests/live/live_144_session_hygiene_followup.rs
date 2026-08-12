//! Live tests for iteration 144 — session hygiene follow-up (carried over
//! from [[iteration-142-session-hygiene]] Theme C/D, plus the deferred
//! Theme F locale item — see `kb/iterations/iteration-144-session-hygiene-followup.md`).
//!
//! Covers:
//! - `launch --auto-consent`'s renamed `auto_consent_extension_installed`
//!   field (Theme C part 1)
//! - `consent accept`'s BBC-style native-CMP adapter (Theme C part 2,
//!   network-gated — the local part of the match rule is unit-tested in
//!   `commands/consent.rs`)
//! - `tabs` filtering the leaked `Consent-O-Matic Options` tab (Theme C
//!   part 3)
//! - `screenshot --full-page` freezing fixed/sticky elements so a header
//!   is captured exactly once (Theme D)
//!
//! Theme F (console locale reproducibility) has no live test here: this
//! implementation environment has only an en-US Firefox available (macOS,
//! no non-English langpack), matching the exact "no non-English Firefox
//! available" case the iteration plan pre-authorizes deferring rather than
//! guessing at — see `kb/iterations/iteration-147-console-locale-repro.md`.
//!
//! # Theme D reproduction note
//!
//! The freeze-fixed/sticky fix in `commands/screenshot.rs` implements the
//! iteration plan's suggested mitigation, but the specific duplicate-header
//! symptom reported in dogfooding session 63 could **not** be reproduced in
//! this implementation environment despite a deliberate before/after
//! attempt: a `position:fixed` header and a `position:sticky` header (both
//! alone and nested, mirroring BBC's own `header{sticky}` +
//! `nav{sticky;top:80px}` structure) were captured at page heights from
//! 2 000 to 20 000 px — spanning common GPU texture-tile boundaries
//! (2048/4096/8192/16384) — and a live capture of the real
//! `https://www.bbc.com/news` itself, with no row-level pixel match for a
//! repeated header band in any case, on or off the fix. The test below
//! therefore verifies the invariant the AC states (no repeated header band)
//! as a forward-looking pixel-level regression guard against a local,
//! deterministic fixture, rather than a reproduced-then-fixed defect.
//!
//! # Running
//!
//!   FF_RDP_LIVE_TESTS=1 cargo test -p ff-rdp-cli --test live live_144 -- --include-ignored
//!   FF_RDP_LIVE_TESTS=1 FF_RDP_LIVE_NETWORK_TESTS=1 cargo test -p ff-rdp-cli \
//!       --test live live_144_bbc_cmp_dismissed -- --include-ignored --nocapture

use std::process::Command;

use base64::Engine as _;

use crate::common::{
    LiveFirefox, base_args, decode_png, ff_rdp_bin, live_network_tests_enabled, live_tests_enabled,
};

fn parse_json(out: &std::process::Output, test: &str) -> serde_json::Value {
    let s = String::from_utf8_lossy(&out.stdout);
    serde_json::from_str(s.trim()).unwrap_or_else(|e| {
        panic!(
            "{test}: stdout is not valid JSON: {e}\nstdout={s}\nstderr={}",
            String::from_utf8_lossy(&out.stderr)
        )
    })
}

/// `live_144_auto_consent_field_honest`:
///
/// `launch --auto-consent`'s JSON reports `results.auto_consent_extension_installed`
/// (never claims a dismiss happened — `launch` returns before any page
/// loads, so it structurally cannot know whether anything will be
/// dismissed) and no longer reports the old `auto_consent` field name, which
/// prior to iter-144 was set unconditionally `true` from the CLI flag and
/// was misread as "a banner was dismissed" (iteration-142 dogfooding
/// finding).
#[test]
#[ignore = "requires Firefox + FF_RDP_LIVE_TESTS=1"]
fn live_144_auto_consent_field_honest() {
    const TEST: &str = "live_144_auto_consent_field_honest";
    if !live_tests_enabled() {
        eprintln!("{TEST}: set FF_RDP_LIVE_TESTS=1 to run");
        return;
    }
    let Some((ff, json)) = LiveFirefox::headless_on_random_port_with_args(&["--auto-consent"])
    else {
        eprintln!("{TEST}: Firefox not available — skipping");
        return;
    };
    let _ = &ff; // keep the guard alive for the duration of the test

    let results = &json["results"];
    assert_eq!(
        results["auto_consent_extension_installed"], true,
        "{TEST}: launch --auto-consent must report auto_consent_extension_installed=true: {json}"
    );
    assert!(
        results.get("auto_consent").is_none(),
        "{TEST}: the old auto_consent field must be gone (renamed to \
         auto_consent_extension_installed, which can only claim the extension \
         was installed, never that anything was dismissed): {json}"
    );
}

/// `live_144_no_consent_o_matic_tab_leak`:
///
/// After `launch --auto-consent`, `tabs` must not list a
/// `Consent-O-Matic Options` entry — that synthetic extension tab is
/// filtered before sort/limit/total are computed.
#[test]
#[ignore = "requires Firefox + FF_RDP_LIVE_TESTS=1"]
fn live_144_no_consent_o_matic_tab_leak() {
    const TEST: &str = "live_144_no_consent_o_matic_tab_leak";
    if !live_tests_enabled() {
        eprintln!("{TEST}: set FF_RDP_LIVE_TESTS=1 to run");
        return;
    }
    let Some((ff, _)) = LiveFirefox::headless_on_random_port_with_args(&["--auto-consent"]) else {
        eprintln!("{TEST}: Firefox not available — skipping");
        return;
    };

    let out = Command::new(ff_rdp_bin())
        .args(base_args(ff.port()))
        .arg("tabs")
        .output()
        .expect("run tabs");
    assert!(
        out.status.success(),
        "{TEST}: tabs failed — {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let json = parse_json(&out, TEST);
    let titles: Vec<String> = json["results"]
        .as_array()
        .unwrap_or_else(|| panic!("{TEST}: results is not an array: {json}"))
        .iter()
        .map(|t| t["title"].as_str().unwrap_or_default().to_owned())
        .collect();
    assert!(
        !titles.iter().any(|t| t == "Consent-O-Matic Options"),
        "{TEST}: Consent-O-Matic's options tab leaked into `tabs`: {titles:?}"
    );
}

/// `live_144_bbc_cmp_dismissed`:
///
/// `consent accept` recognizes and clicks BBC's own (non-iframe) cookie
/// banner at `#bbccookies-continue-button`, and the control is genuinely
/// gone afterward (zero-size bounding rect), not just blindly clicked.
///
/// Network-gated: navigates to the real `www.bbc.com`.
#[test]
#[ignore = "requires Firefox + FF_RDP_LIVE_TESTS=1 + FF_RDP_LIVE_NETWORK_TESTS=1"]
fn live_144_bbc_cmp_dismissed() {
    const TEST: &str = "live_144_bbc_cmp_dismissed";
    if !live_tests_enabled() {
        eprintln!("{TEST}: set FF_RDP_LIVE_TESTS=1 to run");
        return;
    }
    if !live_network_tests_enabled() {
        eprintln!("{TEST}: set FF_RDP_LIVE_NETWORK_TESTS=1 to run");
        return;
    }
    let Some(ff) = LiveFirefox::headless_on_random_port() else {
        eprintln!("{TEST}: Firefox not available — skipping");
        return;
    };

    let nav = Command::new(ff_rdp_bin())
        .args(base_args(ff.port()))
        .args(["navigate", "https://www.bbc.com/news"])
        .output()
        .expect("run navigate");
    if !nav.status.success() {
        eprintln!(
            "{TEST}: navigate to www.bbc.com failed (network unavailable?) — skipping: {}",
            String::from_utf8_lossy(&nav.stderr)
        );
        return;
    }

    let out = Command::new(ff_rdp_bin())
        .args(base_args(ff.port()))
        .args(["consent", "accept"])
        .output()
        .expect("run consent accept");
    assert!(
        out.status.success(),
        "{TEST}: consent accept failed — {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let json = parse_json(&out, TEST);
    assert_eq!(
        json["results"]["cmp"], "bbc",
        "{TEST}: expected the native BBC adapter to match: {json}"
    );
    assert_eq!(
        json["results"]["action"], "accepted",
        "{TEST}: expected a real click, not just a match: {json}"
    );

    // Confirm the control is genuinely gone, not just blindly clicked.
    let eval = Command::new(ff_rdp_bin())
        .args(base_args(ff.port()))
        .args([
            "eval",
            "(function(){var el=document.querySelector('#bbccookies-continue-button');\
             if(!el) return JSON.stringify({present:false});\
             var r=el.getBoundingClientRect();\
             return JSON.stringify({present:true,w:r.width,h:r.height});})()",
        ])
        .output()
        .expect("run eval");
    assert!(eval.status.success(), "{TEST}: eval failed");
    let eval_json = parse_json(&eval, TEST);
    let inner: serde_json::Value = serde_json::from_str(
        eval_json["results"]
            .as_str()
            .unwrap_or_else(|| panic!("{TEST}: eval results not a string: {eval_json}")),
    )
    .unwrap_or_else(|e| panic!("{TEST}: eval result not JSON: {e}"));
    let still_visible = inner["present"].as_bool().unwrap_or(false)
        && inner["w"].as_f64().unwrap_or(0.0) > 0.0
        && inner["h"].as_f64().unwrap_or(0.0) > 0.0;
    assert!(
        !still_visible,
        "{TEST}: the accept control is still visible after `consent accept` claimed \
         it was accepted: {inner}"
    );
}

/// `live_144_full_page_no_duplicate_header`:
///
/// A `position: fixed` header on a page tall enough to matter (8 000 px)
/// appears exactly once — as one contiguous run of matching rows — in a
/// `--full-page` capture. See the module doc for why this is a
/// forward-looking regression guard rather than a reproduced-then-fixed
/// defect: the historic duplicate could not be reproduced in this
/// environment.
const HEADER_URL: &str = "data:text/html,<html><body style='margin:0'>\
    <header style='position:fixed;top:0;left:0;width:100%25;height:60px;\
    background:red;z-index:9999'></header>\
    <div style='height:8000px;background:blue;padding-top:60px'></div>\
    </body></html>";

#[test]
#[ignore = "requires Firefox + FF_RDP_LIVE_TESTS=1"]
fn live_144_full_page_no_duplicate_header() {
    const TEST: &str = "live_144_full_page_no_duplicate_header";
    if !live_tests_enabled() {
        eprintln!("{TEST}: set FF_RDP_LIVE_TESTS=1 to run");
        return;
    }
    let Some(ff) = LiveFirefox::headless_on_random_port() else {
        eprintln!("{TEST}: Firefox not available — skipping");
        return;
    };

    let nav = Command::new(ff_rdp_bin())
        .args(base_args(ff.port()))
        .args(["navigate", "--allow-unsafe-urls", HEADER_URL])
        .output()
        .expect("run navigate");
    assert!(
        nav.status.success(),
        "{TEST}: navigate failed — {}",
        String::from_utf8_lossy(&nav.stderr)
    );

    let out = Command::new(ff_rdp_bin())
        .args(base_args(ff.port()))
        .args(["screenshot", "--full-page", "--base64"])
        .output()
        .expect("run screenshot");
    assert!(
        out.status.success(),
        "{TEST}: screenshot --full-page failed — {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let json = parse_json(&out, TEST);
    let b64 = json["results"]["base64"]
        .as_str()
        .unwrap_or_else(|| panic!("{TEST}: results.base64 missing: {json}"));
    let png_bytes = base64::engine::general_purpose::STANDARD
        .decode(b64)
        .unwrap_or_else(|e| panic!("{TEST}: base64 decode failed: {e}"));

    let img = decode_png(&png_bytes);
    assert!(
        img.height > 8000,
        "{TEST}: expected a full-page capture taller than the 8000px body ({}px): height={}",
        8000,
        img.height
    );

    let red = (255u8, 0u8, 0u8);
    let runs = img.color_row_run_count(red, 40, 0.9);
    assert_eq!(
        runs, 1,
        "{TEST}: expected the fixed red header to appear as exactly one contiguous \
         row-run in the full-page capture, found {runs} — a repeated header band \
         (image: {}x{})",
        img.width, img.height
    );
}
