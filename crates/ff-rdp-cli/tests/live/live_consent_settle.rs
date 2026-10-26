//! Live tests for the 2026-10-26 consent follow-up: `consent accept` waits for
//! the banner to hide, and `navigate` / `screenshot` / `scroll` warn while a
//! known banner covers the page.
//!
//! The fixture is a `data:` page that imitates consentmanager's markup
//! (`#cmpbox` with an `a.cmpboxbtnyes` accept control and a `body` scroll
//! lock) and, like the real CMP measured on the comparis intranet page, hides
//! the box and lifts the lock on a *later* task — 600 ms after the click.
//!
//! # Running
//!
//!   FF_RDP_LIVE_TESTS=1 cargo test -p ff-rdp-cli --test live live_consent_settle -- --include-ignored

use std::process::{Command, Output};

use crate::common::{LiveFirefox, ff_rdp_bin, output_note};

/// `#` would start the data URL's fragment, so the markup avoids it (ids are
/// read with `getElementById`, the colour is a keyword).
fn fixture(hide_after_click: bool) -> String {
    let on_click = if hide_after_click {
        "setTimeout(function(){document.getElementById('cmpbox').style.display='none';\
         document.body.style.overflow='auto';},600)"
    } else {
        "void 0"
    };
    format!(
        "data:text/html,<html><body style=\"overflow:hidden;margin:0\">\
         <div style=\"height:3000px\">page</div>\
         <div id=\"cmpbox\" style=\"position:fixed;top:40px;left:40px;width:400px;height:300px;\
         background:white;border:2px solid black;z-index:10\">We value your consent \
         <a class=\"cmpboxbtnyes\" onclick=\"{on_click}\">I Accept</a></div></body></html>"
    )
}

fn ff(port: u16, timeout_ms: &str, args: &[&str]) -> Output {
    Command::new(ff_rdp_bin())
        .args(["--host", "127.0.0.1", "--port", &port.to_string()])
        .args(["--timeout", timeout_ms])
        .args(args)
        .output()
        .expect("spawn ff-rdp")
}

fn json_of(out: &Output) -> serde_json::Value {
    serde_json::from_slice(&out.stdout)
        .unwrap_or_else(|e| panic!("stdout is not JSON ({e}): {}", output_note(out)))
}

fn consent_warning(json: &serde_json::Value) -> Option<&serde_json::Value> {
    json["results"]["warnings"]
        .as_array()?
        .iter()
        .find(|w| w["type"] == "consent_overlay")
}

/// The whole flow on one Firefox: warn on navigate / screenshot / scroll,
/// accept, and the very next read sees the banner gone and the lock lifted.
#[test]
#[ignore = "requires Firefox — FF_RDP_LIVE_TESTS=1"]
fn live_consent_settle_warns_then_accept_waits_for_the_hide() {
    if std::env::var("FF_RDP_LIVE_TESTS").is_err() {
        eprintln!("live_consent_settle: set FF_RDP_LIVE_TESTS=1 to run");
        return;
    }
    let firefox = LiveFirefox::headless_on_random_port();
    let port = firefox.port();
    let page = fixture(true);

    let nav = ff(port, "30000", &["navigate", &page, "--allow-unsafe-urls"]);
    assert!(nav.status.success(), "{}", output_note(&nav));
    let nav = json_of(&nav);
    let warning = consent_warning(&nav).unwrap_or_else(|| panic!("navigate warning: {nav}"));
    assert_eq!(warning["consent_overlay"], "consentmanager", "{nav}");
    assert_eq!(warning["visible"], true, "{nav}");
    assert_eq!(warning["scroll_locked"], true, "{nav}");
    assert_eq!(warning["hint"], "ff-rdp consent accept", "{nav}");

    let shot = ff(port, "30000", &["screenshot", "--base64"]);
    assert!(shot.status.success(), "{}", output_note(&shot));
    let shot = json_of(&shot);
    assert!(
        consent_warning(&shot).is_some(),
        "screenshot warning: {}",
        shot["results"]["warnings"]
    );

    let scroll = ff(port, "30000", &["scroll", "by", "--dy", "200"]);
    assert!(scroll.status.success(), "{}", output_note(&scroll));
    let scroll = json_of(&scroll);
    assert!(
        consent_warning(&scroll).is_some(),
        "scroll warning: {scroll}"
    );
    assert!(
        scroll["results"].get("consent_overlay").is_none(),
        "the folded probe field is lifted out: {scroll}"
    );

    let accept = ff(port, "10000", &["consent", "accept"]);
    assert!(accept.status.success(), "{}", output_note(&accept));
    let accept = json_of(&accept);
    assert_eq!(accept["results"]["cmp"], "consentmanager", "{accept}");
    assert_eq!(accept["results"]["status"], "accepted", "{accept}");

    // Immediately after: the box is hidden and the lock lifted — before this
    // change the click returned while both were still in place.
    let state = ff(
        port,
        "30000",
        &[
            "eval",
            "getComputedStyle(document.getElementById('cmpbox')).display + '|' + getComputedStyle(document.body).overflow",
        ],
    );
    assert!(state.status.success(), "{}", output_note(&state));
    assert_eq!(
        json_of(&state)["results"],
        "none|auto",
        "{}",
        output_note(&state)
    );

    let shot = ff(port, "30000", &["screenshot", "--base64"]);
    assert!(shot.status.success(), "{}", output_note(&shot));
    let shot = json_of(&shot);
    assert!(
        shot["results"].get("warnings").is_none(),
        "no warning once dismissed: {}",
        shot["results"]["warnings"]
    );
}

/// A click that never hides the banner is `clicked_not_dismissed` (exit 1)
/// once `--timeout` runs out, not `accepted`.
#[test]
#[ignore = "requires Firefox — FF_RDP_LIVE_TESTS=1"]
fn live_consent_settle_banner_that_stays_is_not_dismissed() {
    if std::env::var("FF_RDP_LIVE_TESTS").is_err() {
        eprintln!("live_consent_settle: set FF_RDP_LIVE_TESTS=1 to run");
        return;
    }
    let firefox = LiveFirefox::headless_on_random_port();
    let port = firefox.port();
    let page = fixture(false);
    let nav = ff(port, "30000", &["navigate", &page, "--allow-unsafe-urls"]);
    assert!(nav.status.success(), "{}", output_note(&nav));

    let started = std::time::Instant::now();
    let accept = ff(port, "1500", &["consent", "accept"]);
    let elapsed = started.elapsed();
    assert_eq!(accept.status.code(), Some(1), "{}", output_note(&accept));
    let json = json_of(&accept);
    assert_eq!(json["status"], "clicked_not_dismissed", "{json}");
    assert_eq!(json["error_type"], "consent_not_dismissed", "{json}");
    assert_eq!(json["action"], "clicked", "{json}");
    assert!(
        elapsed >= std::time::Duration::from_millis(1500),
        "waited out the timeout: {elapsed:?}"
    );
}
