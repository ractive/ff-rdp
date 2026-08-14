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
///
/// iter-160 Theme D: exit 1 unless a banner was actually accepted. `--allow-no-cmp`
/// (`allow_no_cmp`) restores exit 0 for the no-CMP outcome only.
pub fn run(cli: &Cli, allow_no_cmp: bool) -> Result<(), AppError> {
    let mut ctx = connect_and_get_target(cli)?;
    let result = detect_and_accept(&mut ctx)?;
    let status = status_of(&result);

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

    // iter-160 Theme D: the envelope is printed either way — a caller that
    // wants the detail of *why* nothing was dismissed still gets `cmp`,
    // `action` and `status` — but a non-accepting pass is a failed action and
    // must not be reported as exit 0. Before this, `consent accept` returned
    // `Ok(())` unconditionally, so a page whose banner was still up and still
    // swallowing clicks looked identical to one that had been dismissed.
    OutputPipeline::from_cli(cli)?.finalize(&envelope)?;
    if let Some(err) = consent_exit_error(status, allow_no_cmp) {
        return Err(err);
    }
    Ok(())
}

/// The error `consent accept` must return for `status`, or `None` when it
/// should exit 0 (iter-160 Theme D).
///
/// `allow_no_cmp` opts *only* the "nothing recognised was on the page" outcome
/// back into exit 0. A CMP that was found and could not be actioned stays a
/// failure regardless: the caller asked for the banner to be dismissed, the
/// banner is still there, and something is wrong with the page or the accept
/// table that the caller needs to see.
fn consent_exit_error(status: ConsentStatus, allow_no_cmp: bool) -> Option<AppError> {
    let error_type = status.error_type()?;
    if allow_no_cmp && status == ConsentStatus::NoCmpDetected {
        return None;
    }
    let message = match status {
        ConsentStatus::Accepted => return None,
        ConsentStatus::DetectedNotActioned => {
            "a known consent-management platform was detected but its accept control could \
             not be actioned — the banner is still on screen.\n\
             hint: inspect the CMP frame with `ff-rdp dom --frames`, or click the control \
             directly with `ff-rdp click --frame <url-substring> <selector>`."
                .to_owned()
        }
        ConsentStatus::NoCmpDetected => {
            "no consent-management platform this build recognises was found on the page — \
             nothing was dismissed.\n\
             hint: if the page has a banner ff-rdp does not know, dismiss it with `ff-rdp \
             click <selector>`; pass --allow-no-cmp to exit 0 when a speculative \
             `consent accept` legitimately has nothing to do."
                .to_owned()
        }
    };
    Some(AppError::Unsupported {
        error_type,
        message,
        details: Some(json!({"status": status.as_str()})),
    })
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

/// One entry in the *native* (same-origin, non-iframe) CMP recognition
/// table — for sites whose consent control lives directly in the top
/// document rather than behind a recognisable cross-origin iframe (iter-144
/// Theme C). Matched against the top-level target's own URL, then clicked
/// via a fixed CSS selector rather than label matching, since a site-owned
/// control's id/class is more stable across locales than its visible text
/// (see `accept_all_js`'s label-based table for the iframe-hosted case).
struct NativeCmpEntry {
    /// Machine-readable CMP name, reported verbatim as `results.cmp`.
    name: &'static str,
    /// Case-insensitive substrings checked against the top-level target's
    /// URL. Any match makes this entry's `selector` the one tried.
    host_url_substrings: &'static [&'static str],
    /// CSS selector for the accept control, evaluated against the top
    /// document.
    selector: &'static str,
}

/// Native CMP table. BBC's own cookie banner (`kb/iterations/
/// iteration-144-session-hygiene-followup.md` Theme C) sits in the top
/// document at `#bbccookies-continue-button` rather than in an iframe.
/// Verified live (2026-08-12) at www.bbc.com/news: BBC also runs a
/// Sourcepoint iframe CMP (`CMP_TABLE` above) that renders *in front of*
/// this control on first paint — the element exists but has a zero-size
/// bounding rect until the Sourcepoint overlay is dismissed, which is why
/// [`native_accept_js`] requires a non-zero rect rather than just DOM
/// presence. `detect_and_accept` tries this table before `CMP_TABLE`, so a
/// second `consent accept` call (after the first dismissed Sourcepoint)
/// reaches it.
const NATIVE_CMP_TABLE: &[NativeCmpEntry] = &[NativeCmpEntry {
    name: "bbc",
    host_url_substrings: &["bbc.com", "bbc.co.uk"],
    selector: "#bbccookies-continue-button",
}];

/// Result of a consent-detection pass. Both fields are always present in the
/// JSON form (`to_json`) — `null`/`null` when no known CMP was found, never
/// omitted, so `--jq '.results.action'` never throws regardless of whether a
/// CMP was present on the page (iter-128 `hint` lesson, applied here from the
/// start per the iteration-129 plan).
pub(crate) struct ConsentResult {
    cmp: Option<&'static str>,
    action: Option<&'static str>,
}

/// The three outcomes a consent pass can have, as one word each (iter-160
/// Theme D).
///
/// All three already existed in the code and were flattened into a single
/// exit 0 with a two-key envelope — so `consent accept` reported success with
/// the banner still on screen, still intercepting clicks (which is also the
/// Theme A failure mode: the two compound in the field).
///
/// This is the *only* place the vocabulary is defined; all three producers
/// (`consent accept`, `navigate --auto-consent`, and `--with-network
/// --auto-consent`'s two call sites) go through `ConsentResult::status` so they
/// cannot drift into separate wordings for the same outcome.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ConsentStatus {
    /// A known CMP was found and its accept control was clicked.
    Accepted,
    /// A known CMP was recognised, but no accept control could be actioned
    /// inside it.
    DetectedNotActioned,
    /// No CMP this build recognises was present.
    NoCmpDetected,
}

impl ConsentStatus {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Accepted => "accepted",
            Self::DetectedNotActioned => "detected_not_actioned",
            Self::NoCmpDetected => "no_cmp_detected",
        }
    }

    /// The `error_type` discriminant for a non-accepting outcome, or `None`
    /// when the outcome is a success and the command must exit 0.
    fn error_type(self) -> Option<&'static str> {
        match self {
            Self::Accepted => None,
            Self::DetectedNotActioned => Some("consent_not_actioned"),
            Self::NoCmpDetected => Some("consent_no_cmp"),
        }
    }
}

impl ConsentResult {
    fn none() -> Self {
        Self {
            cmp: None,
            action: None,
        }
    }

    /// Classify this result. Derived from `cmp`/`action` rather than stored
    /// alongside them, so the status and the two legacy keys can never
    /// disagree about the same pass.
    fn status(&self) -> ConsentStatus {
        match (self.cmp, self.action) {
            (Some(_), Some(_)) => ConsentStatus::Accepted,
            (Some(_), None) => ConsentStatus::DetectedNotActioned,
            (None, _) => ConsentStatus::NoCmpDetected,
        }
    }

    fn to_json(&self) -> Value {
        json!({
            "cmp": self.cmp,
            "action": self.action,
            "status": self.status().as_str(),
        })
    }
}

/// Read the `status` field back out of a `ConsentResult::to_json` value.
///
/// `detect_and_accept` returns a `Value` (its two `navigate` callers embed it
/// verbatim), so `consent accept`'s exit-code decision has to re-read the field
/// rather than keep the enum. An unparseable/absent status is treated as
/// `NoCmpDetected` — the conservative reading, since the only way to get here
/// without a status is a shape this module did not produce.
fn status_of(result: &Value) -> ConsentStatus {
    match result.get("status").and_then(Value::as_str) {
        Some("accepted") => ConsentStatus::Accepted,
        Some("detected_not_actioned") => ConsentStatus::DetectedNotActioned,
        _ => ConsentStatus::NoCmpDetected,
    }
}

/// JS that finds and clicks a control whose accessible label matches a
/// known "accept all" phrasing, evaluated inside a specific frame's console
/// actor. Mirrors `build_click_js`'s not-found error shape so failures read
/// consistently across ff-rdp's eval-based commands.
fn accept_all_js() -> String {
    format!(
        r#"(function() {{
  var re = /^(accept all|accept all cookies|accept all and continue|accept all and close|accept all and subscribe|i accept|i agree|allow all)$/i;
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

/// JS that clicks a fixed CSS selector directly in the top document,
/// requiring a non-zero bounding rect first — a same-origin control can be
/// present in the DOM while genuinely inert (e.g. covered/collapsed behind
/// another overlay, see [`NATIVE_CMP_TABLE`]'s BBC doc comment), and
/// clicking it in that state would falsely report `action: "accepted"`
/// without dismissing anything a user would perceive.
fn native_accept_js(selector: &str) -> String {
    let escaped = selector.replace('\\', "\\\\").replace('\'', "\\'");
    format!(
        r"(function() {{
  var target = document.querySelector('{escaped}');
  if (!target) throw new Error('Element not found: no native consent control at the known selector');
  var r = target.getBoundingClientRect();
  if (r.width === 0 || r.height === 0) throw new Error('Element not found: native consent control present but not visible (covered or collapsed)');
  var label = (target.getAttribute('aria-label') || target.textContent || '').trim();
  target.click();
  return '{JSON_SENTINEL}' + JSON.stringify({{accepted: true, label: label}});
}})()"
    )
}

/// Try every [`NATIVE_CMP_TABLE`] entry whose host substring matches the
/// tab's current top-level URL, clicking the first one found visible.
///
/// Returns `Ok(None)` when no entry's host matches, or the matching entry's
/// selector wasn't found/visible — the caller falls through to the
/// iframe-based [`CMP_TABLE`] path in either case, so a same-origin miss
/// never masks a real cross-origin CMP.
fn try_native_cmp(
    ctx: &mut ConnectedTab,
    console_actor: &ff_rdp_core::ActorId,
    top_level_url: &str,
) -> Result<Option<ConsentResult>, AppError> {
    let Some(entry) = match_native_cmp(top_level_url) else {
        return Ok(None);
    };

    let js = native_accept_js(entry.selector);
    let eval_result = WebConsoleActor::evaluate_js_async(ctx.transport_mut(), console_actor, &js)
        .map_err(AppError::from)?;

    if eval_result.exception.is_none() {
        Ok(Some(ConsentResult {
            cmp: Some(entry.name),
            action: Some("accepted"),
        }))
    } else {
        // Present-but-invisible or genuinely absent — either way, not this
        // call's job to report; let the iframe table take a turn.
        Ok(None)
    }
}

/// Detect a known CMP on the current tab and click its "accept all" control.
///
/// Enumerates frame targets (via the iter-129 Theme A opt-in path). First
/// tries [`NATIVE_CMP_TABLE`] (same-origin, top-document controls) via
/// [`try_native_cmp`]; if that finds nothing actionable, matches each
/// non-top frame's URL against [`CMP_TABLE`] and evaluates the accept-click
/// JS on the first match's own console actor.
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

    // iter-144 Theme C: try the same-origin table before the iframe table —
    // see NATIVE_CMP_TABLE's doc comment for why order matters on BBC.
    if let Some(top) = targets.iter().find(|t| t.is_top_level)
        && let (Some(console_actor), Some(url)) = (&top.console_actor, &top.url)
        && let Some(result) = try_native_cmp(ctx, console_actor, url)?
    {
        return Ok(result.to_json());
    }

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

/// Returns the first [`NativeCmpEntry`] whose `host_url_substrings` matches
/// `url` (case-insensitive). Pure and side-effect-free, mirroring
/// [`match_cmp`] — factored out of [`try_native_cmp`] so the host-matching
/// rule is unit-testable without a live connection.
fn match_native_cmp(url: &str) -> Option<&'static NativeCmpEntry> {
    let lower = url.to_ascii_lowercase();
    NATIVE_CMP_TABLE
        .iter()
        .find(|e| e.host_url_substrings.iter().any(|s| lower.contains(s)))
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

    /// iter-144 Theme C: BBC's live Sourcepoint iframe shows an "I agree"
    /// button, not any of the previous exact-phrase matches — this is the
    /// regex fix, unit-testable via the label-matching regex embedded in
    /// the generated JS string (a full DOM match needs a live browser, see
    /// `live_144_bbc_cmp_dismissed`).
    #[test]
    fn accept_all_js_regex_matches_i_agree() {
        let js = accept_all_js();
        let re_line = js
            .lines()
            .find(|l| l.contains("var re ="))
            .expect("accept_all_js must declare `var re`");
        assert!(
            re_line.to_ascii_lowercase().contains("i agree"),
            "accept_all_js regex must match the bare \"I agree\" label observed live on BBC: {re_line}"
        );
    }

    // ── native CMP matching (iter-144 Theme C) ──────────────────────────

    /// AC: `live_144_bbc_cmp_dismissed` (matching-rule half) — bbc.com and
    /// bbc.co.uk both resolve to the same native entry.
    #[test]
    fn match_native_cmp_matches_bbc_hosts() {
        assert_eq!(
            match_native_cmp("https://www.bbc.com/news").map(|e| e.name),
            Some("bbc")
        );
        assert_eq!(
            match_native_cmp("https://www.bbc.co.uk/news").map(|e| e.name),
            Some("bbc")
        );
    }

    #[test]
    fn match_native_cmp_is_case_insensitive() {
        assert_eq!(
            match_native_cmp("HTTPS://WWW.BBC.COM/NEWS").map(|e| e.name),
            Some("bbc")
        );
    }

    /// AC: `live_144_bbc_cmp_dismissed` (no-match half) — a non-BBC host
    /// must not match, so the native table can't false-positive on
    /// unrelated sites and mask a real iframe-based CMP.
    #[test]
    fn match_native_cmp_no_match_for_unrelated_url() {
        assert_eq!(
            match_native_cmp("https://example.com/").map(|e| e.name),
            None
        );
    }

    #[test]
    fn native_accept_js_requires_visible_rect_and_carries_selector() {
        let js = native_accept_js("#bbccookies-continue-button");
        assert!(js.contains("#bbccookies-continue-button"));
        assert!(js.contains("Element not found:"));
        assert!(js.contains("getBoundingClientRect"));
        assert!(js.contains(JSON_SENTINEL));
    }

    /// A selector containing a single quote must not break out of the JS
    /// string literal it's interpolated into.
    #[test]
    fn native_accept_js_escapes_single_quotes_in_selector() {
        let js = native_accept_js("a[data-x='y']");
        assert!(js.contains(r"a[data-x=\'y\']"));
    }

    // ── iter-160 Theme D: three outcomes, three statuses, two exit codes ───

    /// AC `unit_160_consent_status_values_are_distinct`.
    #[test]
    fn unit_160_consent_status_values_are_distinct() {
        let cases = [
            (
                ConsentResult {
                    cmp: Some("sourcepoint"),
                    action: Some("accepted"),
                },
                "accepted",
                true,
            ),
            (
                ConsentResult {
                    cmp: Some("sourcepoint"),
                    action: None,
                },
                "detected_not_actioned",
                false,
            ),
            (ConsentResult::none(), "no_cmp_detected", false),
        ];

        let mut seen = std::collections::HashSet::new();
        for (result, expected_status, exits_zero) in cases {
            let json = result.to_json();
            assert_eq!(json["status"], json!(expected_status), "{json}");
            assert!(seen.insert(expected_status), "duplicate status value");

            // `cmp`/`action` keep their always-present-key discipline: the
            // keys exist even when null, so `--jq '.results.action'` never
            // throws (iter-128's `hint` lesson, iter-129's plan).
            let obj = json.as_object().expect("object");
            assert!(obj.contains_key("cmp"), "cmp key dropped: {json}");
            assert!(obj.contains_key("action"), "action key dropped: {json}");

            // Only `accepted` exits 0 — and `--allow-no-cmp` is not in play here.
            let err = consent_exit_error(status_of(&json), false);
            assert_eq!(
                err.is_none(),
                exits_zero,
                "status {expected_status} exit-code mapping wrong"
            );
            if let Some(err) = err {
                assert_eq!(err.exit_code(), 1);
            }
        }
        assert_eq!(seen.len(), 3);
    }

    #[test]
    fn unit_160_allow_no_cmp_only_forgives_the_no_cmp_outcome() {
        assert!(consent_exit_error(ConsentStatus::NoCmpDetected, true).is_none());
        // A CMP that was found and could not be actioned is still a failure:
        // the caller asked for the banner to go away and it is still there.
        let err = consent_exit_error(ConsentStatus::DetectedNotActioned, true)
            .expect("must still fail with --allow-no-cmp");
        assert_eq!(err.error_type(), "consent_not_actioned");
    }

    #[test]
    fn unit_160_consent_error_types_are_stable_discriminants() {
        assert_eq!(
            consent_exit_error(ConsentStatus::NoCmpDetected, false)
                .expect("error")
                .error_type(),
            "consent_no_cmp"
        );
        assert_eq!(
            consent_exit_error(ConsentStatus::DetectedNotActioned, false)
                .expect("error")
                .error_type(),
            "consent_not_actioned"
        );
    }

    /// The `no_cmp_detected` message must name the opt-out, so the caller who
    /// legitimately wants exit 0 learns the flag from the failure itself.
    #[test]
    fn unit_160_no_cmp_error_names_allow_no_cmp() {
        let err = consent_exit_error(ConsentStatus::NoCmpDetected, false).expect("error");
        assert!(err.to_string().contains("--allow-no-cmp"), "{err}");
    }

    #[test]
    fn unit_160_status_of_defaults_to_no_cmp_for_an_unknown_shape() {
        assert_eq!(status_of(&json!({})), ConsentStatus::NoCmpDetected);
        assert_eq!(
            status_of(&json!({"status": "accepted"})),
            ConsentStatus::Accepted
        );
    }
}
