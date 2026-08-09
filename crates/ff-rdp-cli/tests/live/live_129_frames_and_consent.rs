//! Live tests for iteration 129 — consent handling + cross-origin frame reach.
//!
//! Covers Theme B (frame-aware `click`), Theme C (native consent
//! acceptance), and Theme D (scroll-lock warning). Theme A
//! (`enumerate_frame_targets` itself) is covered at the `ff-rdp-core` layer
//! in `crates/ff-rdp-core/tests/live_129_frame_targets.rs`.
//!
//! # Running
//!
//!   FF_RDP_LIVE_TESTS=1 cargo test -p ff-rdp-cli --test live live_129 -- --nocapture
//!   FF_RDP_LIVE_NETWORK_TESTS=1 cargo test -p ff-rdp-cli --test live live_129_sourcepoint -- --nocapture
//!
//! Under heavy machine load a fresh Firefox launch can occasionally miss the
//! default wait even though the same command succeeds in isolation — see the
//! iteration-129 plan's "Live-test environment note". If these report
//! "Firefox not available", retry once with `FF_RDP_LIVE_LAUNCH_TIMEOUT_SECS=90`
//! before treating it as a real failure.

use std::process::{Command, Output};

use crate::common::{LiveFirefox, ff_rdp_bin};

/// A `data:` fixture: a top document (unique origin) embedding a genuinely
/// cross-origin `https://example.com` iframe. Matches the fixture the
/// frame-targets research spike verified against live Firefox
/// (`kb/research/frame-targets.md`).
const CROSS_ORIGIN_FIXTURE: &str =
    r#"data:text/html,<h1>top</h1><iframe src="https://example.com"></iframe>"#;

fn base_args(port: u16) -> Vec<String> {
    vec![
        "--host".to_owned(),
        "127.0.0.1".to_owned(),
        "--port".to_owned(),
        port.to_string(),
        "--timeout".to_owned(),
        "30000".to_owned(),
        "--no-daemon".to_owned(),
    ]
}

fn parse_json(output: &Output) -> serde_json::Value {
    let s = String::from_utf8_lossy(&output.stdout);
    serde_json::from_str(s.trim()).unwrap_or_else(|e| {
        panic!(
            "stdout is not valid JSON: {e}\nstdout={s}\nstderr={}",
            String::from_utf8_lossy(&output.stderr)
        )
    })
}

fn navigate_to_fixture(port: u16) -> Output {
    Command::new(ff_rdp_bin())
        .args(base_args(port))
        .args(["navigate", CROSS_ORIGIN_FIXTURE, "--allow-unsafe-urls"])
        .output()
        .expect("navigate to cross-origin fixture")
}

/// AC: `live_129_click_cross_origin_frame` — `click` actuates an element
/// that exists only inside the cross-origin `example.com` frame (the click's
/// observable effect — landing on the anchor and reporting its text — is
/// asserted), with `meta.frame_url` reporting the frame.
#[test]
#[ignore = "requires Firefox + network — FF_RDP_LIVE_TESTS=1"]
fn live_129_click_cross_origin_frame() {
    if std::env::var("FF_RDP_LIVE_TESTS").is_err() {
        eprintln!("live_129_click_cross_origin_frame: set FF_RDP_LIVE_TESTS=1 to run");
        return;
    }
    let Some(ff) = LiveFirefox::headless_on_random_port() else {
        eprintln!("live_129_click_cross_origin_frame: Firefox not available — skipping");
        return;
    };
    let port = ff.port();

    let nav = navigate_to_fixture(port);
    if !nav.status.success() {
        eprintln!(
            "live_129_click_cross_origin_frame: navigate failed — {}",
            String::from_utf8_lossy(&nav.stderr)
        );
        return;
    }

    // The example.com fixture's only <a> lives inside the iframe — the top
    // document has none. No --no-wait: exercises the default auto-wait path
    // (iter-129's frame-aware pre-check), not just the --no-wait fast path.
    let click = Command::new(ff_rdp_bin())
        .args(base_args(port))
        .args(["click", "a"])
        .output()
        .expect("click a");

    assert!(
        click.status.success(),
        "click 'a' must succeed — stdout={} stderr={}",
        String::from_utf8_lossy(&click.stdout),
        String::from_utf8_lossy(&click.stderr)
    );
    let json = parse_json(&click);
    assert_eq!(json["results"]["clicked"], true, "click result: {json}");
    assert_eq!(
        json["results"]["tag"], "A",
        "click must land on the anchor inside the iframe: {json}"
    );
    assert_eq!(
        json["meta"]["frame_url"], "https://example.com/",
        "meta.frame_url must report the frame the click actually landed in: {json}"
    );

    eprintln!("live_129_click_cross_origin_frame: PASSED — {json}");
}

/// AC: `live_129_click_zero_match_error` — a selector matching nothing
/// anywhere fails fast (well under the 10s auto-wait timeout) with the
/// "matched in 0 of N frames (<urls>)" error. Uses `--no-wait` — the
/// auto-wait path's own (pre-existing, unchanged by this iteration) timeout
/// error already covers the "genuinely missing, keep polling" case; this AC
/// is about `do_click`'s frame-scan error message itself.
#[test]
#[ignore = "requires Firefox + network — FF_RDP_LIVE_TESTS=1"]
fn live_129_click_zero_match_error() {
    if std::env::var("FF_RDP_LIVE_TESTS").is_err() {
        eprintln!("live_129_click_zero_match_error: set FF_RDP_LIVE_TESTS=1 to run");
        return;
    }
    let Some(ff) = LiveFirefox::headless_on_random_port() else {
        eprintln!("live_129_click_zero_match_error: Firefox not available — skipping");
        return;
    };
    let port = ff.port();

    let nav = navigate_to_fixture(port);
    if !nav.status.success() {
        eprintln!(
            "live_129_click_zero_match_error: navigate failed — {}",
            String::from_utf8_lossy(&nav.stderr)
        );
        return;
    }

    let started = std::time::Instant::now();
    let click = Command::new(ff_rdp_bin())
        .args(base_args(port))
        .args(["click", ".nonexistent-selector-xyz", "--no-wait"])
        .output()
        .expect("click nonexistent selector");
    let elapsed = started.elapsed();

    assert!(
        !click.status.success(),
        "click on a nonexistent selector must fail: {}",
        String::from_utf8_lossy(&click.stdout)
    );
    assert!(
        elapsed < std::time::Duration::from_secs(8),
        "must fail fast, not pay the 10s auto-wait timeout: took {elapsed:?}"
    );
    let stderr = String::from_utf8_lossy(&click.stderr);
    let stdout = String::from_utf8_lossy(&click.stdout);
    let combined = format!("{stdout}{stderr}");
    assert!(
        combined.contains("matched in 0 of") && combined.contains("frames"),
        "error must name how many frames were tried: {combined}"
    );
    assert!(
        combined.contains("example.com"),
        "error must list the tried frame URLs: {combined}"
    );

    eprintln!("live_129_click_zero_match_error: PASSED in {elapsed:?} — {combined}");
}

/// AC: `live_129_consent_envelope` — the consent flow reports `cmp:null` on
/// a CMP-free page (example.com). The Guardian half of this AC
/// (`cmp:"sourcepoint"`) is `live_129_sourcepoint_consent` below —
/// network-gated separately since it depends on a specific real site's
/// current CMP configuration, not just generic network access.
#[test]
#[ignore = "requires Firefox + network — FF_RDP_LIVE_TESTS=1"]
fn live_129_consent_envelope_no_cmp() {
    if std::env::var("FF_RDP_LIVE_TESTS").is_err() {
        eprintln!("live_129_consent_envelope_no_cmp: set FF_RDP_LIVE_TESTS=1 to run");
        return;
    }
    let Some(ff) = LiveFirefox::headless_on_random_port() else {
        eprintln!("live_129_consent_envelope_no_cmp: Firefox not available — skipping");
        return;
    };
    let port = ff.port();

    let nav = Command::new(ff_rdp_bin())
        .args(base_args(port))
        .args(["navigate", "https://example.com"])
        .output()
        .expect("navigate example.com");
    if !nav.status.success() {
        eprintln!(
            "live_129_consent_envelope_no_cmp: navigate failed — {}",
            String::from_utf8_lossy(&nav.stderr)
        );
        return;
    }

    let consent = Command::new(ff_rdp_bin())
        .args(base_args(port))
        .args(["consent", "accept"])
        .output()
        .expect("consent accept");
    assert!(
        consent.status.success(),
        "consent accept must succeed even with no CMP present — stdout={} stderr={}",
        String::from_utf8_lossy(&consent.stdout),
        String::from_utf8_lossy(&consent.stderr)
    );
    let json = parse_json(&consent);
    assert!(
        json["results"].get("cmp").is_some(),
        "cmp key must always be present: {json}"
    );
    assert!(
        json["results"].get("action").is_some(),
        "action key must always be present: {json}"
    );
    assert!(
        json["results"]["cmp"].is_null(),
        "expected cmp:null: {json}"
    );
    assert!(
        json["results"]["action"].is_null(),
        "expected action:null: {json}"
    );

    eprintln!("live_129_consent_envelope_no_cmp: PASSED — {json}");
}

/// AC: `live_129_sourcepoint_consent` (network-gated) — navigate
/// theguardian.com with `--auto-consent` active →
/// `document.documentElement.className` does NOT contain `sp-message-open`,
/// and `scroll bottom` reaches a `scrollHeight` > 2x the viewport height.
/// Also covers the `cmp:"sourcepoint"` half of `live_129_consent_envelope`.
#[test]
#[ignore = "requires Firefox + network — FF_RDP_LIVE_NETWORK_TESTS=1"]
fn live_129_sourcepoint_consent() {
    if std::env::var("FF_RDP_LIVE_NETWORK_TESTS").is_err() {
        eprintln!("live_129_sourcepoint_consent: set FF_RDP_LIVE_NETWORK_TESTS=1 to run");
        return;
    }
    let Some(ff) = LiveFirefox::headless_on_random_port() else {
        eprintln!("live_129_sourcepoint_consent: Firefox not available — skipping");
        return;
    };
    let port = ff.port();

    let nav = Command::new(ff_rdp_bin())
        .args(base_args(port))
        .args(["navigate", "https://www.theguardian.com", "--auto-consent"])
        .output()
        .expect("navigate --auto-consent theguardian.com");
    if !nav.status.success() {
        eprintln!(
            "live_129_sourcepoint_consent: navigate failed (site may be unreachable) — {}",
            String::from_utf8_lossy(&nav.stderr)
        );
        return;
    }
    let nav_json = parse_json(&nav);
    let consent = &nav_json["results"]["consent"];
    assert!(
        consent.get("cmp").is_some() && consent.get("action").is_some(),
        "results.consent must always carry both keys: {nav_json}"
    );
    if consent["cmp"] != "sourcepoint" {
        // theguardian.com's CMP configuration is outside our control; report
        // clearly and skip rather than fail the whole suite on a site change.
        eprintln!(
            "live_129_sourcepoint_consent: theguardian.com did not present a \
             recognised Sourcepoint frame this run (consent={consent}) — skipping \
             the rest of this AC rather than failing on a live-site change"
        );
        return;
    }
    assert_eq!(
        consent["action"], "accepted",
        "Sourcepoint frame was detected but not accepted: {nav_json}"
    );

    let class_check = Command::new(ff_rdp_bin())
        .args(base_args(port))
        .args([
            "eval",
            r#"document.documentElement.className.includes("sp-message-open")"#,
        ])
        .output()
        .expect("eval sp-message-open check");
    assert!(
        class_check.status.success(),
        "eval class check must succeed"
    );
    let class_json = parse_json(&class_check);
    assert_eq!(
        class_json["results"], false,
        "html.sp-message-open must be cleared after consent is accepted: {class_json}"
    );

    let viewport_height = Command::new(ff_rdp_bin())
        .args(base_args(port))
        .args(["eval", "window.innerHeight"])
        .output()
        .expect("eval window.innerHeight");
    let viewport_height = parse_json(&viewport_height)["results"]
        .as_f64()
        .expect("innerHeight must be numeric");

    let scroll = Command::new(ff_rdp_bin())
        .args(base_args(port))
        .args(["scroll", "bottom", "--jq", ".results.scrollHeight"])
        .output()
        .expect("scroll bottom");
    assert!(scroll.status.success(), "scroll bottom must succeed");
    let scroll_height: f64 = String::from_utf8_lossy(&scroll.stdout)
        .trim()
        .parse()
        .unwrap_or_else(|e| panic!("scrollHeight not numeric: {e}"));
    assert!(
        scroll_height > viewport_height * 2.0,
        "page must be scrollable well past the viewport once the CMP overlay is \
         gone (scrollHeight={scroll_height}, viewport={viewport_height})"
    );

    eprintln!(
        "live_129_sourcepoint_consent: PASSED — consent={consent}, scrollHeight={scroll_height}, viewport={viewport_height}"
    );
}

/// AC: `live_129_scroll_lock_warning` — on a fixture with `html{overflow:hidden}`,
/// `scroll bottom` emits a warning identifying the scroll lock.
#[test]
#[ignore = "requires Firefox — FF_RDP_LIVE_TESTS=1"]
fn live_129_scroll_lock_warning() {
    if std::env::var("FF_RDP_LIVE_TESTS").is_err() {
        eprintln!("live_129_scroll_lock_warning: set FF_RDP_LIVE_TESTS=1 to run");
        return;
    }
    let Some(ff) = LiveFirefox::headless_on_random_port() else {
        eprintln!("live_129_scroll_lock_warning: Firefox not available — skipping");
        return;
    };
    let port = ff.port();

    let fixture = r#"data:text/html,<html style="overflow:hidden" class="sp-message-open"><body><h1>locked</h1></body></html>"#;
    let nav = Command::new(ff_rdp_bin())
        .args(base_args(port))
        .args(["navigate", fixture, "--allow-unsafe-urls"])
        .output()
        .expect("navigate to overflow:hidden fixture");
    if !nav.status.success() {
        eprintln!(
            "live_129_scroll_lock_warning: navigate failed — {}",
            String::from_utf8_lossy(&nav.stderr)
        );
        return;
    }

    let scroll = Command::new(ff_rdp_bin())
        .args(base_args(port))
        .args(["scroll", "bottom"])
        .output()
        .expect("scroll bottom");
    assert!(
        scroll.status.success(),
        "scroll bottom must still succeed (not error) on a locked page: {}",
        String::from_utf8_lossy(&scroll.stderr)
    );
    let json = parse_json(&scroll);
    assert!(
        json["results"].get("warning").is_some(),
        "warning key must always be present: {json}"
    );
    let warning = json["results"]["warning"]
        .as_str()
        .unwrap_or_else(|| panic!("warning must be a non-null string on a locked page: {json}"));
    assert!(
        warning.contains("overflow:hidden"),
        "warning must name the overflow:hidden cause: {warning}"
    );
    assert!(
        warning.contains("sp-message-open"),
        "warning must name the locking element's class: {warning}"
    );

    eprintln!("live_129_scroll_lock_warning: PASSED — {warning}");
}

/// Sanity check: on a page with no scroll lock, `warning` stays present but
/// `null` — the always-present-key discipline applies to the happy path too.
#[test]
#[ignore = "requires Firefox — FF_RDP_LIVE_TESTS=1"]
fn live_129_scroll_no_lock_warning_is_null() {
    if std::env::var("FF_RDP_LIVE_TESTS").is_err() {
        eprintln!("live_129_scroll_no_lock_warning_is_null: set FF_RDP_LIVE_TESTS=1 to run");
        return;
    }
    let Some(ff) = LiveFirefox::headless_on_random_port() else {
        eprintln!("live_129_scroll_no_lock_warning_is_null: Firefox not available — skipping");
        return;
    };
    let port = ff.port();

    let nav = Command::new(ff_rdp_bin())
        .args(base_args(port))
        .args(["navigate", "https://example.com"])
        .output()
        .expect("navigate example.com");
    if !nav.status.success() {
        eprintln!(
            "live_129_scroll_no_lock_warning_is_null: navigate failed — {}",
            String::from_utf8_lossy(&nav.stderr)
        );
        return;
    }

    let scroll = Command::new(ff_rdp_bin())
        .args(base_args(port))
        .args(["scroll", "bottom"])
        .output()
        .expect("scroll bottom");
    assert!(scroll.status.success());
    let json = parse_json(&scroll);
    assert!(
        json["results"].get("warning").is_some(),
        "warning key must always be present: {json}"
    );
    assert!(
        json["results"]["warning"].is_null(),
        "expected warning:null on an unlocked page: {json}"
    );

    eprintln!("live_129_scroll_no_lock_warning_is_null: PASSED");
}
