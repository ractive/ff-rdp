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

const BBC_FAILURE_CONTEXT_MAX_BYTES: usize = 16 * 1024;
const BBC_FAILURE_CONTEXT_TRUNCATION_MARKER: &str = "\n...[diagnostic truncated]";

fn cap_bbc_failure_context(note: String) -> String {
    if note.len() <= BBC_FAILURE_CONTEXT_MAX_BYTES {
        return note;
    }

    let prefix_limit =
        BBC_FAILURE_CONTEXT_MAX_BYTES.saturating_sub(BBC_FAILURE_CONTEXT_TRUNCATION_MARKER.len());
    let prefix_end = note
        .char_indices()
        .take_while(|(index, character)| *index + character.len_utf8() <= prefix_limit)
        .map(|(index, character)| index + character.len_utf8())
        .last()
        .unwrap_or(0);
    format!(
        "{}{}",
        &note[..prefix_end],
        BBC_FAILURE_CONTEXT_TRUNCATION_MARKER
    )
}

const BBC_FAILURE_CONTEXT_JS: &str = r#"(function() {
      var MAX_MATCHES = 12;
      var MAX_TEXT_LENGTH = 240;
      var MAX_ATTRIBUTE_LENGTH = 160;
      function truncate(value, limit) {
        value = String(value || '');
        return value.length <= limit ? value : value.slice(0, limit) + '...[truncated]';
      }
      function shape(el) {
        var r = el.getBoundingClientRect();
        return {
          tag: el.tagName,
          id: truncate(el.id, MAX_ATTRIBUTE_LENGTH),
          role: truncate(el.getAttribute('role'), MAX_ATTRIBUTE_LENGTH),
          className: truncate(el.className, MAX_ATTRIBUTE_LENGTH),
          src: truncate(el.getAttribute('src'), MAX_ATTRIBUTE_LENGTH),
          width: r.width,
          height: r.height,
          text: truncate(el.innerText || el.textContent, MAX_TEXT_LENGTH)
        };
      }
      function cookieSummary() {
        var raw = document.cookie || '';
        var names = raw.split(';').map(function(part) {
          return part.split('=')[0].trim();
        }).filter(Boolean);
        return {
          present: raw.length > 0,
          count: names.length,
          names: names.slice(0, MAX_MATCHES).map(function(name) {
            return truncate(name, MAX_ATTRIBUTE_LENGTH);
          }),
          omitted: Math.max(0, names.length - MAX_MATCHES),
          rawLength: raw.length
        };
      }
      function bounded(selector, limit) {
        var all = Array.from(document.querySelectorAll(selector));
        return {items: all.slice(0, limit).map(shape), omitted: Math.max(0, all.length - limit)};
      }
      return JSON.stringify({url: truncate(location.href, MAX_ATTRIBUTE_LENGTH),
        documentURL: truncate(document.URL, MAX_ATTRIBUTE_LENGTH),
        title: truncate(document.title, MAX_TEXT_LENGTH),
        readyState: document.readyState,
        language: navigator.language,
        languages: Array.from(navigator.languages || []).slice(0, MAX_MATCHES),
        documentLanguage: truncate(document.documentElement.lang, MAX_ATTRIBUTE_LENGTH),
        timeZone: Intl.DateTimeFormat().resolvedOptions().timeZone,
        cookies: cookieSummary(),
        native: bounded('#bbccookies-continue-button', MAX_MATCHES),
        banners: bounded('[id*="cookie"],[id*="consent"],[role="dialog"],iframe', MAX_MATCHES)
      });
    })()"#;

// Classify the command's report, not the site's unknowable decision-time DOM.
// Only NativeAccepted can advance to the separate post-action effect check.
#[derive(Debug, PartialEq)]
enum BbcConsentOutcome {
    NativeAccepted,
    OtherCmp,
    NoCmpReported,
    NativeNotActioned,
    InvalidOrFailed,
}

fn bbc_consent_outcome(success: bool, envelope: &serde_json::Value) -> BbcConsentOutcome {
    let result = if success {
        &envelope["results"]
    } else {
        envelope
    };
    if !success
        && envelope["error_type"] == "consent_no_cmp"
        && result.get("cmp") == Some(&serde_json::Value::Null)
        && result.get("action") == Some(&serde_json::Value::Null)
        && result["status"] == "no_cmp_detected"
    {
        return BbcConsentOutcome::NoCmpReported;
    }
    match result["cmp"].as_str() {
        Some("bbc")
            if success
                && envelope.get("error_type").is_none()
                && result["action"] == "accepted"
                && result["status"] == "accepted" =>
        {
            BbcConsentOutcome::NativeAccepted
        }
        Some("bbc") => BbcConsentOutcome::NativeNotActioned,
        Some(_) => BbcConsentOutcome::OtherCmp,
        None => BbcConsentOutcome::InvalidOrFailed,
    }
}

fn bbc_control_dismissed(sample: &serde_json::Value) -> bool {
    match sample["present"].as_bool() {
        Some(false) => true,
        Some(true) => match (sample["w"].as_f64(), sample["h"].as_f64()) {
            (Some(width), Some(height))
                if width.is_finite() && height.is_finite() && width >= 0.0 && height >= 0.0 =>
            {
                width == 0.0 || height == 0.0
            }
            _ => false,
        },
        None => false,
    }
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
    let (ff, json) = LiveFirefox::headless_on_random_port_with_args(&["--auto-consent"]);
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
    let (ff, _) = LiveFirefox::headless_on_random_port_with_args(&["--auto-consent"]);

    let out = Command::new(ff_rdp_bin())
        .args(base_args(ff.port()))
        .arg("tabs")
        .output()
        .expect("run tabs");
    assert!(
        out.status.success(),
        "{TEST}: tabs failed — {}",
        crate::common::output_note(&out)
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
#[ignore = "requires Firefox + FF_RDP_LIVE_TESTS=1 + FF_RDP_LIVE_NETWORK_TESTS=1 + FF_RDP_LIVE_SITES_TESTS=1 (third-party site)"]
fn live_144_bbc_cmp_dismissed() {
    const TEST: &str = "live_144_bbc_cmp_dismissed";
    if !crate::common::live_sites_tests_enabled() {
        eprintln!("{TEST}: set FF_RDP_LIVE_SITES_TESTS=1 to run (third-party site)");
        return;
    }
    if !live_tests_enabled() {
        eprintln!("{TEST}: set FF_RDP_LIVE_TESTS=1 to run");
        return;
    }
    if !live_network_tests_enabled() {
        eprintln!("{TEST}: set FF_RDP_LIVE_NETWORK_TESTS=1 to run");
        return;
    }
    let ff = LiveFirefox::headless_on_random_port();

    let nav = Command::new(ff_rdp_bin())
        .args(base_args(ff.port()))
        .args(["navigate", "https://www.bbc.com/news"])
        .output()
        .expect("run navigate");
    assert!(
        nav.status.success(),
        "{TEST}: navigation failed; no dismissal was verified — {}\npage after failure: {}",
        crate::common::output_note(&nav),
        bbc_failure_context(ff.port())
    );

    let out = Command::new(ff_rdp_bin())
        .args(base_args(ff.port()))
        .args(["consent", "accept"])
        .output()
        .expect("run consent accept");
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap_or_else(|error| {
        panic!(
            "{TEST}: invalid consent JSON ({error}) — {}\npage after failure (later observation): {}",
            crate::common::output_note(&out),
            bbc_failure_context(ff.port())
        )
    });
    let outcome = bbc_consent_outcome(out.status.success(), &json);
    assert_eq!(
        outcome,
        BbcConsentOutcome::NativeAccepted,
        "{TEST}: native BBC dismissal unverified; command outcome {outcome:?} — {}\n\
         navigate: {}\npage after failure (later observation): {}",
        crate::common::output_note(&out),
        crate::common::output_note(&nav),
        bbc_failure_context(ff.port())
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
    assert!(
        bbc_control_dismissed(&inner),
        "{TEST}: native post-action effect missing or invalid: {inner}\n\
         page after failure (later observation): {}",
        bbc_failure_context(ff.port())
    );
}

// Read only after a failed command: an extra pre-consent round trip could hide
// the readiness race this real-site test is intended to expose. This is a
// subsequent observation, not an atomic snapshot of the failed consent call.
fn bbc_failure_context(port: u16) -> String {
    match Command::new(ff_rdp_bin())
        .args(base_args(port))
        .args(["eval", BBC_FAILURE_CONTEXT_JS])
        .output()
    {
        Ok(out) => cap_bbc_failure_context(crate::common::output_note(&out)),
        Err(err) => cap_bbc_failure_context(format!("could not collect page context: {err}")),
    }
}

// allow-ungated-live: Firefox-free formatting regression; no browser or env gate is needed.
#[test]
fn bbc_failure_context_preserves_utf8_and_caps_bytes() {
    let short = "short diagnostic é".to_owned();
    assert_eq!(cap_bbc_failure_context(short.clone()), short);
    let oversized = format!("prefix-{}", "é".repeat(BBC_FAILURE_CONTEXT_MAX_BYTES));
    let capped = cap_bbc_failure_context(oversized);
    assert!(capped.len() <= BBC_FAILURE_CONTEXT_MAX_BYTES);
    assert!(capped.ends_with(BBC_FAILURE_CONTEXT_TRUNCATION_MARKER));
    let prefix = capped
        .strip_suffix(BBC_FAILURE_CONTEXT_TRUNCATION_MARKER)
        .expect("truncation marker");
    assert!(prefix.starts_with("prefix-"));
    assert!(prefix["prefix-".len()..].chars().all(|ch| ch == 'é'));
    assert!(BBC_FAILURE_CONTEXT_MAX_BYTES - capped.len() < 'é'.len_utf8());
}

// allow-ungated-live: pure command-contract unit controls, not invented RDP/site fixtures.
#[test]
fn bbc_consent_requires_native_accepted_action() {
    use serde_json::json;
    let native = json!({"results":{"cmp":"bbc","action":"accepted","status":"accepted"}});
    assert_eq!(
        bbc_consent_outcome(true, &native),
        BbcConsentOutcome::NativeAccepted
    );
    for field in ["cmp", "action", "status"] {
        let mut missing = native.clone();
        missing["results"].as_object_mut().unwrap().remove(field);
        assert_ne!(
            bbc_consent_outcome(true, &missing),
            BbcConsentOutcome::NativeAccepted
        );
        let mut null = native.clone();
        null["results"][field] = serde_json::Value::Null;
        assert_ne!(
            bbc_consent_outcome(true, &null),
            BbcConsentOutcome::NativeAccepted
        );
    }
    assert_ne!(
        bbc_consent_outcome(false, &native),
        BbcConsentOutcome::NativeAccepted
    );
    let other = json!({"results":{"cmp":"sourcepoint","action":"accepted","status":"accepted"}});
    assert_eq!(
        bbc_consent_outcome(true, &other),
        BbcConsentOutcome::OtherCmp
    );
    let no_cmp =
        json!({"error_type":"consent_no_cmp","cmp":null,"action":null,"status":"no_cmp_detected"});
    assert_eq!(
        bbc_consent_outcome(false, &no_cmp),
        BbcConsentOutcome::NoCmpReported
    );
    assert_ne!(
        bbc_consent_outcome(true, &no_cmp),
        BbcConsentOutcome::NativeAccepted
    );
    let no_action = json!({"error_type":"consent_not_actioned","cmp":"bbc","action":null,"status":"detected_not_actioned"});
    assert_eq!(
        bbc_consent_outcome(false, &no_action),
        BbcConsentOutcome::NativeNotActioned
    );
}

// allow-ungated-live: pure effect-oracle controls; malformed samples must not prove dismissal.
#[test]
fn bbc_dismissal_requires_valid_post_action_effect() {
    use serde_json::json;
    for sample in [
        json!({"present":false}),
        json!({"present":true,"w":0,"h":10}),
        json!({"present":true,"w":10,"h":0}),
    ] {
        assert!(bbc_control_dismissed(&sample), "{sample}");
    }
    for sample in [
        json!({}),
        json!({"present":null}),
        json!({"present":true}),
        json!({"present":true,"w":0}),
        json!({"present":true,"w":-1,"h":0}),
        json!({"present":true,"w":10,"h":10}),
    ] {
        assert!(!bbc_control_dismissed(&sample), "{sample}");
    }
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
    let ff = LiveFirefox::headless_on_random_port();

    let nav = Command::new(ff_rdp_bin())
        .args(base_args(ff.port()))
        .args(["navigate", "--allow-unsafe-urls", HEADER_URL])
        .output()
        .expect("run navigate");
    assert!(
        nav.status.success(),
        "{TEST}: navigate failed — {}",
        crate::common::output_note(&nav)
    );

    let out = Command::new(ff_rdp_bin())
        .args(base_args(ff.port()))
        .args(["screenshot", "--full-page", "--base64"])
        .output()
        .expect("run screenshot");
    assert!(
        out.status.success(),
        "{TEST}: screenshot --full-page failed — {}",
        crate::common::output_note(&out)
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
