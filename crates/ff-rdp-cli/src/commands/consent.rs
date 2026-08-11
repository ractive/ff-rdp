//! Native consent-management-platform (CMP) detection and acceptance
//! (iter-129 Theme C).
//!
//! Consent-O-Matic (installed by `ff-rdp launch --auto-consent`) does not
//! record consent for Sourcepoint's headless-hostile CMP — see
//! `dogfooding-session-62` finding 1. This module is the CLI-native
//! fallback: it enumerates the tab's frame targets (iter-129 Theme A),
//! recognises a known CMP by matching a frame's URL against a small table,
//! and clicks that frame's "accept all" control via the frame's own
//! `consoleActor` — the exact mechanism `click`'s frame-scan fallback
//! (Theme B) uses, applied to a fixed selector set instead of a scan.
//!
//! Wired into two entry points:
//! - `ff-rdp consent accept` — explicit, on-demand.
//! - `ff-rdp navigate --auto-consent` — best-effort, post-navigate.

use ff_rdp_core::WebConsoleActor;
use serde_json::{Value, json};

use crate::cli::args::Cli;
use crate::error::AppError;
use crate::output;
use crate::output_pipeline::OutputPipeline;

use super::connect_tab::{ConnectedTab, connect_and_get_target};
use super::js_helpers::JSON_SENTINEL;

/// `ff-rdp consent accept` — explicit, on-demand consent acceptance.
pub fn run(cli: &Cli) -> Result<(), AppError> {
    let mut ctx = connect_and_get_target(cli)?;
    let result = detect_and_accept(&mut ctx)?;

    let mut meta = json!({});
    crate::connection_meta::merge_into_if_verbose(
        &mut meta,
        &cli.host,
        cli.port,
        None,
        cli.is_verbose(),
    );
    // iter-134: always present, not gated by --verbose — an
    // agent can tell how this command executed without a
    // separate `daemon status` round-trip.
    crate::connection_meta::merge_route(&mut meta, ctx.via_daemon);
    let envelope = output::envelope(&result, 1, &meta);

    OutputPipeline::from_cli(cli)?.finalize(&envelope)
}

/// One entry in the CMP recognition table: a name plus the URL substrings
/// that identify that CMP's consent iframe.
struct CmpEntry {
    /// Machine-readable CMP name, reported verbatim as `results.cmp`.
    name: &'static str,
    /// Case-insensitive substrings checked against a non-top frame target's
    /// URL. Any match identifies the frame as this CMP's consent overlay.
    frame_url_substrings: &'static [&'static str],
}

/// CMP recognition table. Sourcepoint first per the plan — the CMP gating
/// theguardian.com and confirmed reachable end-to-end in
/// `kb/research/frame-targets.md`. Extend by adding entries; the detection
/// and accept-click logic is CMP-agnostic (label-based button matching).
const CMP_TABLE: &[CmpEntry] = &[CmpEntry {
    name: "sourcepoint",
    // theguardian.com hosts its Sourcepoint frame at
    // sourcepoint.theguardian.com; sp-prod.net / privacy-mgmt.com are
    // Sourcepoint's own multi-tenant CMP domains used by other sites.
    frame_url_substrings: &["sourcepoint", "sp-prod.net", "privacy-mgmt.com"],
}];

/// Result of a consent-detection pass. Both fields are always present in the
/// JSON form (`to_json`) — `null`/`null` when no known CMP was found, never
/// omitted, so `--jq '.results.action'` never throws regardless of whether a
/// CMP was present on the page (iter-128 `hint` lesson, applied here from the
/// start per the iteration-129 plan).
struct ConsentResult {
    cmp: Option<&'static str>,
    action: Option<&'static str>,
}

impl ConsentResult {
    fn none() -> Self {
        Self {
            cmp: None,
            action: None,
        }
    }

    fn to_json(&self) -> Value {
        json!({"cmp": self.cmp, "action": self.action})
    }
}

/// JS that finds and clicks a control whose accessible label matches a
/// known "accept all" phrasing, evaluated inside a specific frame's console
/// actor. Mirrors `build_click_js`'s not-found error shape so failures read
/// consistently across ff-rdp's eval-based commands.
fn accept_all_js() -> String {
    format!(
        r#"(function() {{
  var re = /^(accept all|accept all cookies|accept all and continue|accept all and close|accept all and subscribe|i accept|allow all)$/i;
  var candidates = Array.prototype.slice.call(document.querySelectorAll('button, [role="button"], a'));
  var target = null;
  for (var i = 0; i < candidates.length; i++) {{
    var el = candidates[i];
    var label = (el.getAttribute('aria-label') || el.textContent || '').trim();
    if (re.test(label)) {{ target = el; break; }}
  }}
  if (!target) throw new Error('Element not found: no accept-all control matched known consent labels');
  var label = (target.getAttribute('aria-label') || target.textContent || '').trim();
  target.click();
  return '{JSON_SENTINEL}' + JSON.stringify({{accepted: true, label: label}});
}})()"#
    )
}

/// Detect a known CMP on the current tab and click its "accept all" control.
///
/// Enumerates frame targets (via the iter-129 Theme A opt-in path), matches
/// each non-top frame's URL against [`CMP_TABLE`], and evaluates the
/// accept-click JS on the first match's own console actor.
///
/// Returns `{"cmp": null, "action": null}` when no known CMP frame is found.
/// Returns `{"cmp": "<name>", "action": null}` when a CMP frame is found but
/// no matching accept control could be located in it (CMP detected, not
/// actionable — still an always-present pair of keys, never omitted).
/// Returns `{"cmp": "<name>", "action": "accepted"}` on success.
pub(crate) fn detect_and_accept(ctx: &mut ConnectedTab) -> Result<Value, AppError> {
    // iter-137 Theme A: route through the connection-aware entry point.  The
    // former direct-only enumeration was a no-op through the daemon (the
    // daemon subscribed to frame targets at startup, so a second
    // `watchTargets` re-delivers nothing), which is why `consent accept`
    // reported `{"cmp":null,"action":null}` on a Sourcepoint site unless the
    // caller passed `--no-daemon`.
    let targets = crate::commands::frame_targets::fetch_frame_targets(ctx)?;

    let Some((cmp_name, target)) = targets.iter().find_map(|t| {
        if t.is_top_level {
            return None;
        }
        match_cmp(t.url.as_deref().unwrap_or_default()).map(|name| (name, t))
    }) else {
        return Ok(ConsentResult::none().to_json());
    };

    let Some(console_actor) = target.console_actor.as_ref() else {
        // Frame matched by URL but Firefox didn't ship a consoleActor for it
        // (should not happen per the frame-targets research, but stay
        // defensive) — report the CMP as found, not actioned.
        return Ok(ConsentResult {
            cmp: Some(cmp_name),
            action: None,
        }
        .to_json());
    };

    let js = accept_all_js();
    let eval_result = WebConsoleActor::evaluate_js_async(ctx.transport_mut(), console_actor, &js)
        .map_err(AppError::from)?;

    let action = if eval_result.exception.is_none() {
        Some("accepted")
    } else {
        // Frame matched but no known-label button was found inside it —
        // report detection without a false "accepted" claim.
        None
    };
    Ok(ConsentResult {
        cmp: Some(cmp_name),
        action,
    }
    .to_json())
}

/// Returns the first [`CmpEntry`] whose `frame_url_substrings` matches `url`
/// (case-insensitive). Pure and side-effect-free — factored out of
/// `detect_and_accept`'s frame loop purely so the matching rule itself is
/// unit-testable without a live connection.
fn match_cmp(url: &str) -> Option<&'static str> {
    let lower = url.to_ascii_lowercase();
    CMP_TABLE
        .iter()
        .find(|cmp| cmp.frame_url_substrings.iter().any(|s| lower.contains(s)))
        .map(|cmp| cmp.name)
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── CMP matching ─────────────────────────────────────────────────────

    /// AC: `live_129_consent_envelope` (matching-rule half) — the
    /// Guardian's Sourcepoint host matches.
    #[test]
    fn match_cmp_matches_guardian_sourcepoint_host() {
        assert_eq!(
            match_cmp("https://sourcepoint.theguardian.com/index.html?message_id=1"),
            Some("sourcepoint")
        );
    }

    #[test]
    fn match_cmp_matches_generic_sourcepoint_domains() {
        assert_eq!(
            match_cmp("https://ccpa-notice.sp-prod.net/?message_id=1"),
            Some("sourcepoint")
        );
        assert_eq!(
            match_cmp("https://cdn.privacy-mgmt.com/index.html"),
            Some("sourcepoint")
        );
    }

    #[test]
    fn match_cmp_is_case_insensitive() {
        assert_eq!(
            match_cmp("HTTPS://SOURCEPOINT.THEGUARDIAN.COM/"),
            Some("sourcepoint")
        );
    }

    /// AC: `live_129_consent_envelope` (no-match half) — a CMP-free page
    /// (e.g. example.com) must not match.
    #[test]
    fn match_cmp_no_match_for_unrelated_url() {
        assert_eq!(match_cmp("https://example.com/"), None);
    }

    // ── ConsentResult always-present-key discipline ─────────────────────

    #[test]
    fn consent_result_none_has_both_keys_null() {
        let v = ConsentResult::none().to_json();
        assert!(v.get("cmp").is_some(), "cmp key must be present: {v}");
        assert!(v.get("action").is_some(), "action key must be present: {v}");
        assert!(v["cmp"].is_null());
        assert!(v["action"].is_null());
    }

    #[test]
    fn consent_result_accepted_shape() {
        let v = ConsentResult {
            cmp: Some("sourcepoint"),
            action: Some("accepted"),
        }
        .to_json();
        assert_eq!(v["cmp"], "sourcepoint");
        assert_eq!(v["action"], "accepted");
    }

    #[test]
    fn consent_result_detected_not_actioned_shape() {
        // cmp found, but action stays null — both keys still present.
        let v = ConsentResult {
            cmp: Some("sourcepoint"),
            action: None,
        }
        .to_json();
        assert_eq!(v["cmp"], "sourcepoint");
        assert!(v.get("action").is_some());
        assert!(v["action"].is_null());
    }

    // ── accept_all_js shape ──────────────────────────────────────────────

    #[test]
    fn accept_all_js_contains_not_found_marker_and_sentinel() {
        let js = accept_all_js();
        assert!(js.contains("Element not found:"));
        assert!(js.contains(JSON_SENTINEL));
        assert!(js.contains("accept all"));
    }
}
