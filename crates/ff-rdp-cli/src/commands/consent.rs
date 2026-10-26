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
//!
//! An accept click is not a dismissal: consentmanager hides `#cmpbox` and
//! lifts its `body { overflow: hidden }` scroll lock on a later task, so a
//! `screenshot` taken right after the click still showed the banner. After a
//! click, [`wait_for_dismissal`] polls the overlay until it is hidden, bounded
//! by the command's `--timeout` (2026-10-26 consent follow-up).
//!
//! The same tables also drive a read-only overlay probe
//! ([`overlay_probe_expr`]) that `navigate`, `screenshot` and `scroll` use to
//! warn that a known banner is still covering the page and to point at
//! `ff-rdp consent accept`.

use std::time::{Duration, Instant};

use ff_rdp_core::{ActorId, Grip, WebConsoleActor};
use serde_json::{Value, json};

use crate::cli::args::Cli;
use crate::error::AppError;
use crate::output;
use crate::output_pipeline::OutputPipeline;

use super::connect_tab::{ConnectedTab, connect_and_get_target};

/// `ff-rdp consent accept` — explicit, on-demand consent acceptance.
///
/// iter-160 Theme D: exit 1 unless a banner was actually accepted. `--allow-no-cmp`
/// (`allow_no_cmp`) restores exit 0 for the no-CMP outcome only.
pub fn run(cli: &Cli, allow_no_cmp: bool) -> Result<(), AppError> {
    let mut ctx = connect_and_get_target(cli)?;
    let result = detect_and_accept(&mut ctx, Duration::from_millis(cli.timeout))?;
    let status = status_of(&result);

    let mut meta = json!({});
    crate::connection_meta::merge_into_if_verbose(
        &mut meta,
        &cli.host,
        cli.port,
        None,
        cli.is_verbose(),
    );

    // iter-160 Theme D: a non-accepting pass is a failed action and must not be
    // reported as exit 0. Before this, `consent accept` returned `Ok(())`
    // unconditionally, so a page whose banner was still up and still swallowing
    // clicks looked identical to one that had been dismissed.
    //
    // Return **before** printing the success envelope: `main` emits the error
    // envelope for every `Err` it receives, so printing here first would put two
    // JSON documents on stdout — exactly the double-envelope iter-153 removed
    // from `launch --replace`. Nothing is lost, because `cmp`, `action` and
    // `status` ride along in the error's `details` and land at the error
    // envelope's top level.
    if let Some(err) = consent_exit_error(status, allow_no_cmp, &result) {
        return Err(err);
    }

    let envelope = output::envelope(&result, 1, &meta);
    OutputPipeline::from_cli(cli)?.finalize(&envelope)
}

/// The error `consent accept` must return for `status`, or `None` when it
/// should exit 0 (iter-160 Theme D).
///
/// `allow_no_cmp` opts *only* the "nothing recognised was on the page" outcome
/// back into exit 0. A CMP that was found and could not be actioned stays a
/// failure regardless: the caller asked for the banner to be dismissed, the
/// acceptance is unconfirmed. An unexpected evaluation failure may follow a
/// partial action, so it does not prove the banner is still visible.
///
/// `result` is the `ConsentResult::to_json` value; its three keys are merged
/// into the error envelope so the failing caller reads `cmp`/`action`/`status`
/// out of the same object it parses `error_type` from, and the command does not
/// have to emit a second JSON document to carry them.
fn consent_exit_error(
    status: ConsentStatus,
    allow_no_cmp: bool,
    result: &Value,
) -> Option<AppError> {
    let error_type = status.error_type()?;
    if allow_no_cmp && status == ConsentStatus::NoCmpDetected {
        return None;
    }
    let message = match status {
        ConsentStatus::Accepted => return None,
        ConsentStatus::DetectedNotActioned => {
            "a known consent-management platform was detected, but acceptance could not be confirmed.\n\
             hint: to act on a known control, use \
             `ff-rdp click --frame <url-substring> <selector>`."
                .to_owned()
        }
        ConsentStatus::ClickedNotDismissed => {
            "the consent banner's accept control was clicked, but the banner was still visible \
             when --timeout ran out — it was not dismissed.\n\
             hint: retry with a longer --timeout, or check what covers the page with \
             `ff-rdp screenshot`."
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
    let mut details = result.clone();
    if let Some(obj) = details.as_object_mut() {
        obj.insert("status".to_owned(), json!(status.as_str()));
    }
    Some(AppError::Unsupported {
        error_type,
        message,
        details: Some(details),
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
    /// URL. Any match makes this entry's `selector` the one tried. Empty means
    /// "any host": a multi-tenant CMP recognised by its own markup, which the
    /// `selector` both detects and actions.
    host_url_substrings: &'static [&'static str],
    /// CSS selector for the accept control, evaluated against the top
    /// document.
    selector: &'static str,
    /// CSS selector for the element whose visibility *is* the banner: the
    /// overlay probe reports it, and an accept is only `accepted` once it is
    /// hidden. Where the banner's container is not known, the accept control
    /// itself stands in (it disappears with the banner).
    overlay_selector: &'static str,
}

impl NativeCmpEntry {
    /// Whether an ambiguous evaluation of this entry counts as a pre-action
    /// miss: true for an any-host entry (empty `host_url_substrings`), whose
    /// probe runs on every page whether or not the CMP is there.
    fn ambiguity_is_miss(&self) -> bool {
        self.host_url_substrings.is_empty()
    }
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
///
/// consentmanager.net (dogfooding-session-64 #23) is a multi-tenant CMP
/// (comparis.ch among many) that injects `#cmpbox` into the top document —
/// no iframe, so `CMP_TABLE` never saw it. Its accept control is
/// `a.cmpboxbtnyes` ("I Accept" / "Akzeptieren", so the selector, not the
/// label, is what is stable). Verified live 2026-10-04 on
/// www.comparis.ch/hypotheken: the click hides `#cmpbox` on the next task and
/// `__tcfapi('getTCData')` reports `eventStatus: "useractioncomplete"`.
const NATIVE_CMP_TABLE: &[NativeCmpEntry] = &[
    NativeCmpEntry {
        name: "bbc",
        host_url_substrings: &["bbc.com", "bbc.co.uk"],
        selector: "#bbccookies-continue-button",
        overlay_selector: "#bbccookies-continue-button",
    },
    // 2026-10-26: `#cmpbox` (the dialog) gets `display: none` and the body's
    // inline `overflow: hidden` becomes `overflow: auto` between ~25 ms and
    // ~1 s after the click (measured on the comparis intranet page), not in
    // the click's own task.
    NativeCmpEntry {
        name: "consentmanager",
        host_url_substrings: &[],
        selector: "#cmpbox a.cmpboxbtnyes",
        overlay_selector: "#cmpbox",
    },
];

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
    /// A known CMP was found, its accept control was clicked, and the banner
    /// was then observed hidden.
    Accepted,
    /// The accept control was clicked, but the banner was still visible when
    /// the settle budget (`--timeout`) ran out. `action` is `"clicked"`.
    ClickedNotDismissed,
    /// A known CMP was recognised, but acceptance was not confirmed. This
    /// includes ambiguous evaluation failures after a possible partial action.
    DetectedNotActioned,
    /// No CMP this build recognises was present.
    NoCmpDetected,
}

impl ConsentStatus {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Accepted => "accepted",
            Self::ClickedNotDismissed => "clicked_not_dismissed",
            Self::DetectedNotActioned => "detected_not_actioned",
            Self::NoCmpDetected => "no_cmp_detected",
        }
    }

    /// The `error_type` discriminant for a non-accepting outcome, or `None`
    /// when the outcome is a success and the command must exit 0.
    fn error_type(self) -> Option<&'static str> {
        match self {
            Self::Accepted => None,
            Self::ClickedNotDismissed => Some("consent_not_dismissed"),
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
            (Some(_), Some(ACTION_CLICKED)) => ConsentStatus::ClickedNotDismissed,
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
        Some("clicked_not_dismissed") => ConsentStatus::ClickedNotDismissed,
        Some("detected_not_actioned") => ConsentStatus::DetectedNotActioned,
        _ => ConsentStatus::NoCmpDetected,
    }
}

/// JS that finds and clicks a known accept-all label in one frame. Only a
/// known pre-action miss returns false; unexpected exceptions are not caught.
///
/// Two tiers:
/// 1. A known accept-all label (`Accept all`, `I agree`, ...) in any frame —
///    the GDPR/TCF variants — plus `Accept and Continue`, the only button on
///    BBC's US `us_pm` notice (consent-probe run 37326616943). It is an
///    explicit accept label, so it lives here rather than in the close tier.
/// 2. Only when the frame's own document is a US opt-out privacy notice
///    (Sourcepoint's `us_pm` notice: `location.pathname` contains `/us_pm/`
///    or `location.search` contains `is_usnat_notice=true`), a visible
///    close control (`title`, `aria-label` or text equal to `Closer` or
///    `Close`). The Guardian's close-only variant of that notice has no
///    accept control at all: under the US opt-out regime consent is the
///    default state, the only choice offered is "Do not sell or share my
///    personal information", and closing the notice keeps the default — so
///    a close click is the acceptance a user would perform, and is reported
///    as `accepted`. `Closer` is Sourcepoint's literal label for the close X,
///    observed on theguardian.com from a US runner on 2026-10-05
///    (consent-probe run 37313988449). The tier never runs in a GDPR frame:
///    closing a GDPR banner is not acceptance. Labels such as `Continue`,
///    `OK` or `Dismiss` are deliberately excluded: on the privacy-manager
///    page reached via "Do not sell", such a button confirms an opt-out,
///    which must not be reported as `accepted`.
fn accept_all_js() -> String {
    r#"(function() {
  var re = /^(accept all|accept all cookies|accept all and continue|accept all and close|accept all and subscribe|accept and continue|i accept|i agree|allow all)$/i;
  var candidates = Array.prototype.slice.call(document.querySelectorAll('button, [role="button"], a'));
  var target = null;
  for (var i = 0; i < candidates.length; i++) {
    var el = candidates[i];
    var label = (el.getAttribute('aria-label') || el.textContent || '').trim();
    if (re.test(label)) { target = el; break; }
  }
  if (!target) {
    var usNotice = location.pathname.indexOf('/us_pm/') !== -1 || location.search.indexOf('is_usnat_notice=true') !== -1;
    if (!usNotice) return false;
    var closeRe = /^(close|closer)$/i;
    var buttons = Array.prototype.slice.call(document.querySelectorAll('button, [role="button"]'));
    for (var j = 0; j < buttons.length; j++) {
      var b = buttons[j];
      var names = [b.getAttribute('title'), b.getAttribute('aria-label'), b.textContent];
      var hit = false;
      for (var k = 0; k < names.length; k++) {
        if (names[k] && closeRe.test(names[k].trim())) { hit = true; break; }
      }
      if (!hit) continue;
      var r = b.getBoundingClientRect();
      if (r.width === 0 || r.height === 0) continue;
      target = b;
      break;
    }
  }
  if (!target) return false;
  target.click();
  return true;
})()"#.to_owned()
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
  if (!target) return false;
  var r = target.getBoundingClientRect();
  if (r.width === 0 || r.height === 0) return false;
  target.click();
  return true;
}})()"
    )
}

/// A false Boolean is the only evaluation result that permits another action.
/// A true Boolean confirms the click call returned; site persistence requires
/// separate observation. Exceptions and other grips leave acceptance unknown.
enum AcceptAttempt {
    Miss,
    Accepted,
    Unconfirmed,
}

fn evaluate_accept(
    ctx: &mut ConnectedTab,
    console_actor: &ff_rdp_core::ActorId,
    js: &str,
) -> Result<AcceptAttempt, AppError> {
    let result = WebConsoleActor::evaluate_js_async(ctx.transport_mut(), console_actor, js)
        .map_err(AppError::from)?;
    if result.exception.is_some() {
        return Ok(AcceptAttempt::Unconfirmed);
    }
    Ok(match result.result {
        Grip::Value(Value::Bool(false)) => AcceptAttempt::Miss,
        Grip::Value(Value::Bool(true)) => AcceptAttempt::Accepted,
        _ => AcceptAttempt::Unconfirmed,
    })
}

/// Try every [`NATIVE_CMP_TABLE`] entry whose host substring matches the
/// tab's current top-level URL, in table order, clicking the first one found
/// visible.
///
/// Returns `Ok(None)` when no entry's host matches, or no matching entry's
/// selector was found/visible — the caller falls through to the
/// iframe-based [`CMP_TABLE`] path in either case, so a same-origin miss
/// never masks a real cross-origin CMP.
fn try_native_cmp(
    ctx: &mut ConnectedTab,
    console_actor: &ff_rdp_core::ActorId,
    top_level_url: &str,
) -> Result<Option<ConsentResult>, AppError> {
    for entry in matching_native_cmps(top_level_url) {
        let js = native_accept_js(entry.selector);
        match evaluate_accept(ctx, console_actor, &js)? {
            AcceptAttempt::Miss => {}
            AcceptAttempt::Accepted => {
                return Ok(Some(ConsentResult {
                    cmp: Some(entry.name),
                    action: Some(ACTION_ACCEPTED),
                }));
            }
            // An any-host probe runs on pages that never had this CMP, so an
            // ambiguous result there (a dead document mid-navigation, say) is
            // far likelier a failed probe than a partial click; treating it as
            // terminal would report a CMP the site does not use and skip the
            // real iframe CMP.
            AcceptAttempt::Unconfirmed if entry.ambiguity_is_miss() => {}
            // An exception may occur after a partial action. Never try another
            // entry or an iframe after an ambiguous host-matched action,
            // including an unexpected result.
            AcceptAttempt::Unconfirmed => {
                return Ok(Some(ConsentResult {
                    cmp: Some(entry.name),
                    action: None,
                }));
            }
        }
    }
    Ok(None)
}

/// Detect a known CMP on the current tab and click its "accept all" control.
///
/// Enumerates frame targets (via the iter-129 Theme A opt-in path). First
/// tries [`NATIVE_CMP_TABLE`] (same-origin, top-document controls) via
/// [`try_native_cmp`]; if that finds nothing actionable, matches each
/// non-top frame's URL against [`CMP_TABLE`] and evaluates the accept-click
/// JS on each match's own console actor until an action succeeds. Only a known
/// pre-action miss permits continuing; an ambiguous evaluation stops the pass.
///
/// Returns `{"cmp": null, "action": null}` when no known CMP frame is found.
/// Returns `{"cmp": "<name>", "action": null}` when a CMP frame is found but
/// acceptance could not be confirmed (including an ambiguous partial action).
/// These keys remain present for every outcome, never omitted.
/// Returns `{"cmp": "<name>", "action": "accepted"}` once the click landed and
/// the banner was then observed hidden, and `{"cmp": "<name>", "action":
/// "clicked"}` (status `clicked_not_dismissed`) when it was still visible after
/// `settle_budget` — see [`settle_action`].
pub(crate) fn detect_and_accept(
    ctx: &mut ConnectedTab,
    settle_budget: Duration,
) -> Result<Value, AppError> {
    // One frame-target enumeration per connection — see `fetch_frame_targets`.
    let targets = crate::commands::frame_targets::fetch_frame_targets(ctx)?;
    let top = targets.iter().find(|t| t.is_top_level);
    let top_console = top.and_then(|t| t.console_actor.clone());

    // iter-144 Theme C: try the same-origin table before the iframe table —
    // see NATIVE_CMP_TABLE's doc comment for why order matters on BBC.
    if let Some(top) = top
        && let (Some(console_actor), Some(url)) = (&top.console_actor, &top.url)
        && let Some(mut result) = try_native_cmp(ctx, console_actor, url)?
    {
        if let (Some(cmp), Some(ACTION_ACCEPTED)) = (result.cmp, result.action) {
            result.action = Some(settle_action(ctx, console_actor, cmp, settle_budget));
        }
        return Ok(result.to_json());
    }

    let mut result = ConsentResult::none();
    let js = accept_all_js();
    for target in targets.iter().filter(|target| !target.is_top_level) {
        let Some(cmp_name) = match_cmp(target.url.as_deref().unwrap_or_default()) else {
            continue;
        };
        if result.cmp.is_none() {
            result.cmp = Some(cmp_name);
        }
        let Some(console_actor) = target.console_actor.as_ref() else {
            // Recognition without a usable console is not an action. A later
            // recognized frame can still contain the actionable control.
            continue;
        };
        let action = match evaluate_accept(ctx, console_actor, &js)? {
            AcceptAttempt::Miss => continue,
            // The iframe CMP has the same race as the native one: the click
            // runs in the CMP frame, and the top document removes or hides
            // that frame later. Settle on the top document's view of it.
            // Without a top console there is nothing to observe it with, so
            // the click result stands, as before.
            AcceptAttempt::Accepted => Some(match &top_console {
                Some(top_console) => settle_action(ctx, top_console, cmp_name, settle_budget),
                None => ACTION_ACCEPTED,
            }),
            AcceptAttempt::Unconfirmed => None,
        };
        return Ok(ConsentResult {
            cmp: Some(cmp_name),
            action,
        }
        .to_json());
    }
    Ok(result.to_json())
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

/// Every [`NativeCmpEntry`] whose `host_url_substrings` matches `url`
/// (case-insensitive), in table order; an entry with no substrings matches
/// every host. Pure and side-effect-free, mirroring [`match_cmp`] — factored
/// out of [`try_native_cmp`] so the host-matching rule is unit-testable
/// without a live connection.
fn matching_native_cmps(url: &str) -> impl Iterator<Item = &'static NativeCmpEntry> {
    let lower = url.to_ascii_lowercase();
    NATIVE_CMP_TABLE.iter().filter(move |e| {
        e.host_url_substrings.is_empty() || e.host_url_substrings.iter().any(|s| lower.contains(s))
    })
}

// ── settle: an accept is a dismissal only once the banner is hidden ─────────

/// `action` of a click whose banner was then observed hidden.
const ACTION_ACCEPTED: &str = "accepted";
/// `action` of a click whose banner was still visible when the settle budget
/// ran out (status `clicked_not_dismissed`).
const ACTION_CLICKED: &str = "clicked";

/// How often the settle loop re-reads the overlay.
const SETTLE_POLL: Duration = Duration::from_millis(100);

/// How long the settle loop keeps waiting for a scroll lock to lift once the
/// overlay itself is hidden. Bounded separately from `--timeout` because a
/// page may keep `overflow: hidden` for reasons of its own; the banner being
/// gone is what makes the accept `accepted`.
const UNLOCK_GRACE: Duration = Duration::from_secs(2);

/// One read of the overlay the settle loop is waiting on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SettleSample {
    /// The banner element is shown and intersects the viewport.
    visible: bool,
    /// `<html>` or `<body>` has `overflow-y: hidden`.
    scroll_locked: bool,
}

/// What the settle loop concluded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Settle {
    /// The overlay was observed hidden. `scroll_locked` is whether the page
    /// was still scroll-locked when the loop stopped waiting for the lock.
    Dismissed { scroll_locked: bool },
    /// The overlay was still visible (or could not be read) at the deadline.
    StillVisible,
}

/// Poll `sample` until the overlay is hidden, within `budget`.
///
/// Once the overlay is hidden, keep polling up to `unlock_grace` longer (still
/// inside `budget`) for the scroll lock to lift, so the caller's next `scroll`
/// or `screenshot` does not see a page that is still frozen. A sample of
/// `None` (the evaluation failed) counts as "not yet hidden": an unreadable
/// overlay is not evidence that it went away.
fn wait_for_dismissal(
    mut sample: impl FnMut() -> Option<SettleSample>,
    budget: Duration,
    poll: Duration,
    unlock_grace: Duration,
) -> Settle {
    let deadline = Instant::now() + budget;
    let mut hidden_since: Option<Instant> = None;
    loop {
        let read = sample();
        let now = Instant::now();
        match read {
            Some(SettleSample {
                visible: false,
                scroll_locked: false,
            }) => {
                return Settle::Dismissed {
                    scroll_locked: false,
                };
            }
            Some(SettleSample { visible: false, .. }) => {
                let since = *hidden_since.get_or_insert(now);
                if now >= deadline || now.duration_since(since) >= unlock_grace {
                    return Settle::Dismissed {
                        scroll_locked: true,
                    };
                }
            }
            _ => {
                hidden_since = None;
                if now >= deadline {
                    return Settle::StillVisible;
                }
            }
        }
        std::thread::sleep(poll.min(deadline.saturating_duration_since(now)));
    }
}

/// Wait for `cmp`'s overlay to hide after its accept control was clicked, and
/// return the `action` the result should carry.
fn settle_action(
    ctx: &mut ConnectedTab,
    top_console: &ActorId,
    cmp: &'static str,
    budget: Duration,
) -> &'static str {
    let js = format!("JSON.stringify({})", overlay_probe_expr(Some(cmp)));
    let settle = wait_for_dismissal(
        || {
            let result = WebConsoleActor::evaluate_js_async(ctx.transport_mut(), top_console, &js)
                .ok()?;
            if result.exception.is_some() {
                return None;
            }
            let state = overlay_state_from_grip(&result.result)?;
            Some(SettleSample {
                visible: state.visible,
                scroll_locked: state.scroll_locked,
            })
        },
        budget,
        SETTLE_POLL,
        UNLOCK_GRACE,
    );
    tracing::debug!(cmp, ?settle, "consent: settle after accept click");
    match settle {
        Settle::Dismissed { .. } => ACTION_ACCEPTED,
        Settle::StillVisible => ACTION_CLICKED,
    }
}

// ── overlay probe: is a known banner covering the page right now? ───────────

/// Marker embedded in the overlay probe so a reader of a protocol log (or a
/// mock server) can tell it from the accept-click JS.
pub(crate) const OVERLAY_PROBE_MARKER: &str = "ff-rdp:consent-overlay-probe";

/// The command every consent-overlay warning and hint points at.
pub(crate) const CONSENT_ACCEPT_CMD: &str = "ff-rdp consent accept";

/// The `warnings[].type` of a consent-overlay warning.
const OVERLAY_WARNING_TYPE: &str = "consent_overlay";

/// The `{natives, frames}` tables the probe JS walks, as a JS literal. With
/// `only`, just that CMP's entry (the settle loop watches one banner).
fn overlay_tables_js(only: Option<&str>) -> String {
    let keep = |name: &str| only.is_none_or(|o| o == name);
    let natives: Vec<Value> = NATIVE_CMP_TABLE
        .iter()
        .filter(|e| keep(e.name))
        .map(|e| json!({"name": e.name, "hosts": e.host_url_substrings, "sel": e.overlay_selector}))
        .collect();
    let frames: Vec<Value> = CMP_TABLE
        .iter()
        .filter(|e| keep(e.name))
        .map(|e| json!({"name": e.name, "subs": e.frame_url_substrings}))
        .collect();
    json!({"natives": natives, "frames": frames}).to_string()
}

/// A JS expression (no trailing semicolon) evaluating to `{cmp, visible,
/// scroll_locked}` for the known CMPs in [`NATIVE_CMP_TABLE`] and
/// [`CMP_TABLE`] (or only `only`'s entry).
///
/// - `cmp`: the first CMP whose overlay is shown and intersects the viewport,
///   else the first whose overlay merely exists, else `null`. A native CMP's
///   overlay is its `overlay_selector`; an iframe CMP's is any `<iframe>` in
///   the top document whose `src` matches its `frame_url_substrings`.
/// - `visible`: whether `cmp` is the shown one.
/// - `scroll_locked`: `<html>` or `<body>` has `overflow-y: hidden`.
///
/// Read-only, no frame-target enumeration: one evaluation in the top document,
/// cheap enough to fold into JS a command already runs (`scroll`) or to run
/// once after `navigate` / before `screenshot`.
pub(crate) fn overlay_probe_expr(only: Option<&str>) -> String {
    let tables = overlay_tables_js(only);
    format!(
        r"(function() {{ /* {OVERLAY_PROBE_MARKER} */
  var t = {tables};
  function shown(el) {{
    var cs = getComputedStyle(el);
    if (cs.display === 'none' || cs.visibility === 'hidden') return false;
    var r = el.getBoundingClientRect();
    return r.width > 0 && r.height > 0 && r.bottom > 0 && r.right > 0 &&
      r.top < window.innerHeight && r.left < window.innerWidth;
  }}
  function lock(el) {{ return !!el && getComputedStyle(el).overflowY === 'hidden'; }}
  var href = String(location.href).toLowerCase();
  var present = null, visible = null;
  function note(name, el) {{
    if (!present) present = name;
    if (!visible && shown(el)) visible = name;
  }}
  t.natives.forEach(function(n) {{
    if (n.hosts.length && !n.hosts.some(function(h) {{ return href.indexOf(h) !== -1; }})) return;
    var el = document.querySelector(n.sel);
    if (el) note(n.name, el);
  }});
  var frames = document.querySelectorAll('iframe');
  t.frames.forEach(function(f) {{
    for (var i = 0; i < frames.length; i++) {{
      var src = String(frames[i].src || '').toLowerCase();
      if (f.subs.some(function(s) {{ return src.indexOf(s) !== -1; }})) note(f.name, frames[i]);
    }}
  }});
  return {{cmp: visible || present, visible: !!visible,
    scroll_locked: lock(document.documentElement) || lock(document.body)}};
}})()"
    )
}

/// The probe's raw reading, as parsed from its JSON.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
struct OverlayState {
    cmp: Option<String>,
    #[serde(default)]
    visible: bool,
    #[serde(default)]
    scroll_locked: bool,
}

fn overlay_state_from_value(value: &Value) -> Option<OverlayState> {
    // The standalone probe returns `JSON.stringify(...)`, i.e. a string; the
    // folded form (`scroll`) is already an object inside the command's JSON.
    match value {
        Value::String(s) => serde_json::from_str(s).ok(),
        Value::Object(_) => serde_json::from_value(value.clone()).ok(),
        _ => None,
    }
}

fn overlay_state_from_grip(grip: &Grip) -> Option<OverlayState> {
    match grip {
        Grip::Value(v) => overlay_state_from_value(v),
        _ => None,
    }
}

/// A known CMP banner that is covering the page or holding its scroll lock.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ConsentOverlay {
    cmp: String,
    visible: bool,
    scroll_locked: bool,
}

impl ConsentOverlay {
    /// Warn when a known CMP is present and either visible or the page is
    /// scroll-locked (consentmanager locks `body` while `#cmpbox` is up).
    fn from_state(state: OverlayState) -> Option<Self> {
        let cmp = state.cmp?;
        (state.visible || state.scroll_locked).then_some(Self {
            cmp,
            visible: state.visible,
            scroll_locked: state.scroll_locked,
        })
    }

    pub(crate) fn cmp(&self) -> &str {
        &self.cmp
    }

    /// The `results.warnings[]` entry for this overlay.
    fn warning(&self) -> Value {
        let what = match (self.visible, self.scroll_locked) {
            (true, true) => "is covering the page and has locked scrolling",
            (true, false) => "is covering the page",
            _ => "has locked scrolling",
        };
        json!({
            "type": OVERLAY_WARNING_TYPE,
            "consent_overlay": self.cmp,
            "visible": self.visible,
            "scroll_locked": self.scroll_locked,
            "message": format!(
                "a {} consent banner {what}: screenshots show it, clicks may land on it and \
                 scrolling may not move the page. Dismiss it with `{CONSENT_ACCEPT_CMD}`.",
                self.cmp
            ),
            "hint": CONSENT_ACCEPT_CMD,
        })
    }
}

/// Run the overlay probe on `console_actor` (the top document's console).
///
/// Best-effort: any failure — a dead actor, an exception, an unexpected
/// result — is `None`, because the probe only decides whether to add a
/// warning and must never fail the command it rides on.
pub(crate) fn probe_overlay(
    ctx: &mut ConnectedTab,
    console_actor: &ActorId,
) -> Option<ConsentOverlay> {
    let js = format!("JSON.stringify({})", overlay_probe_expr(None));
    let result = WebConsoleActor::evaluate_js_async(ctx.transport_mut(), console_actor, &js).ok()?;
    if result.exception.is_some() {
        return None;
    }
    ConsentOverlay::from_state(overlay_state_from_grip(&result.result)?)
}

/// Remove the folded probe's `consent_overlay` field from a command's JS
/// result and return the overlay it reports, if any (`scroll`).
pub(crate) fn take_overlay_field(results: &mut Value) -> Option<ConsentOverlay> {
    let raw = results.as_object_mut()?.remove(OVERLAY_WARNING_TYPE)?;
    ConsentOverlay::from_state(overlay_state_from_value(&raw)?)
}

/// Append `overlay`'s warning to `results.warnings` (created when absent).
pub(crate) fn attach_overlay_warning(results: &mut Value, overlay: &ConsentOverlay) {
    let Some(obj) = results.as_object_mut() else {
        return;
    };
    let warnings = obj
        .entry("warnings")
        .or_insert_with(|| Value::Array(Vec::new()));
    if let Some(list) = warnings.as_array_mut() {
        list.push(overlay.warning());
    }
}

/// The CMP named by a consent-overlay warning in `results.warnings`, for the
/// command's hint context.
pub(crate) fn overlay_cmp_in(results: &Value) -> Option<&str> {
    results
        .get("warnings")?
        .as_array()?
        .iter()
        .find(|w| w.get("type").and_then(Value::as_str) == Some(OVERLAY_WARNING_TYPE))?
        .get(OVERLAY_WARNING_TYPE)?
        .as_str()
}

/// Drop the consent-overlay warning from `results.warnings` (and the key when
/// it empties) — for `navigate --auto-consent`, whose accept ran after the
/// probe and dismissed the banner the warning describes.
pub(crate) fn clear_overlay_warning(results: &mut Value) {
    let Some(obj) = results.as_object_mut() else {
        return;
    };
    let empty = match obj.get_mut("warnings").and_then(Value::as_array_mut) {
        Some(list) => {
            list.retain(|w| w.get("type").and_then(Value::as_str) != Some(OVERLAY_WARNING_TYPE));
            list.is_empty()
        }
        None => false,
    };
    if empty {
        obj.remove("warnings");
    }
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
    fn accept_all_js_has_explicit_pre_action_miss_and_success() {
        let js = accept_all_js();
        assert!(js.contains("return false"));
        assert!(js.contains("target.click();\n  return true;"));
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

    /// The first tier still matches the GDPR labels observed from Europe
    /// (BBC "I agree", Guardian "Accept all") after the US tier was added.
    #[test]
    fn accept_all_js_first_tier_keeps_gdpr_labels() {
        let js = accept_all_js();
        let re_line = js
            .lines()
            .find(|l| l.contains("var re ="))
            .expect("accept_all_js must declare `var re`");
        let lower = re_line.to_ascii_lowercase();
        assert!(lower.contains("i agree"), "{re_line}");
        assert!(lower.contains("accept all"), "{re_line}");
        assert!(
            lower.contains("|accept and continue|"),
            "BBC's US us_pm notice label must be a tier-1 accept label: {re_line}"
        );
        assert!(
            !lower.contains("closer"),
            "close labels must not be in the first tier: {re_line}"
        );
    }

    /// Sourcepoint's US opt-out notice (`us_pm`) has no accept control; the
    /// second tier clicks its close X ("Closer", observed 2026-10-05) — but
    /// only behind the `us_pm` / `is_usnat_notice` guard, never in a GDPR frame.
    #[test]
    fn accept_all_js_us_tier_is_guarded_and_matches_closer() {
        let js = accept_all_js();
        assert!(js.contains("'/us_pm/'"), "{js}");
        assert!(js.contains("'is_usnat_notice=true'"), "{js}");
        assert!(js.contains("if (!usNotice) return false;"), "{js}");
        let close_line = js
            .lines()
            .find(|l| l.contains("var closeRe ="))
            .expect("accept_all_js must declare `var closeRe`");
        assert!(
            close_line.contains("/^(close|closer)$/i"),
            "close regex must stay limited to close|closer: {close_line}"
        );
        for opt_out_confirm in ["continue", "ok", "dismiss", "got it"] {
            assert!(
                !close_line.to_ascii_lowercase().contains(opt_out_confirm),
                "{opt_out_confirm} could confirm an opt-out: {close_line}"
            );
        }
        assert!(js.contains("getBoundingClientRect()"), "{js}");
        // The guard precedes the close-label scan.
        let guard = js.find("if (!usNotice) return false;").expect("guard");
        let scan = js.find("var closeRe").expect("closeRe");
        assert!(guard < scan);
    }

    // ── native CMP matching (iter-144 Theme C) ──────────────────────────

    /// AC: `live_144_bbc_cmp_dismissed` (matching-rule half) — bbc.com and
    /// bbc.co.uk both resolve to the same native entry.
    fn native_names(url: &str) -> Vec<&'static str> {
        matching_native_cmps(url).map(|e| e.name).collect()
    }

    #[test]
    fn match_native_cmp_matches_bbc_hosts() {
        assert_eq!(
            native_names("https://www.bbc.com/news"),
            ["bbc", "consentmanager"]
        );
        assert_eq!(
            native_names("https://www.bbc.co.uk/news"),
            ["bbc", "consentmanager"]
        );
    }

    #[test]
    fn match_native_cmp_is_case_insensitive() {
        assert_eq!(native_names("HTTPS://WWW.BBC.COM/NEWS")[0], "bbc");
    }

    /// Only the any-host entry treats an ambiguous evaluation as a miss; the
    /// host-matched BBC entry keeps the stop-after-ambiguity rule (PR #312
    /// review).
    #[test]
    fn only_any_host_entries_treat_ambiguity_as_miss() {
        for entry in NATIVE_CMP_TABLE {
            assert_eq!(
                entry.ambiguity_is_miss(),
                entry.name == "consentmanager",
                "{}",
                entry.name
            );
        }
    }

    /// dogfooding-session-64 #23: consentmanager.net is multi-tenant, so it is
    /// tried on every host, and its selector targets the in-document box.
    #[test]
    fn consentmanager_matches_any_host_with_cmpbox_selector() {
        assert_eq!(
            native_names("https://www.comparis.ch/hypotheken"),
            ["consentmanager"]
        );
        let entry = matching_native_cmps("https://www.comparis.ch/")
            .next()
            .expect("consentmanager entry");
        assert_eq!(entry.selector, "#cmpbox a.cmpboxbtnyes");
    }

    /// AC: `live_144_bbc_cmp_dismissed` (no-match half) — a non-BBC host
    /// must not match the BBC entry, so the native table can't false-positive
    /// on unrelated sites. Only the any-host consentmanager entry applies, and
    /// its selector misses on a page without `#cmpbox`, falling through to the
    /// iframe table.
    #[test]
    fn match_native_cmp_no_match_for_unrelated_url() {
        assert_eq!(native_names("https://example.com/"), ["consentmanager"]);
    }

    #[test]
    fn native_accept_js_requires_visible_rect_and_carries_selector() {
        let js = native_accept_js("#bbccookies-continue-button");
        assert!(js.contains("#bbccookies-continue-button"));
        assert!(js.contains("return false"));
        assert!(js.contains("getBoundingClientRect"));
        assert!(js.contains("target.click();\n  return true;"));
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
            let err = consent_exit_error(status_of(&json), false, &json);
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

    /// The failing path must be able to carry `cmp`/`action`/`status` on its
    /// own, because `run` returns before printing the success envelope — two
    /// JSON documents on stdout is the iter-153 double-envelope shape.
    #[test]
    fn unit_160_consent_error_envelope_carries_cmp_action_and_status() {
        let result = ConsentResult {
            cmp: Some("sourcepoint"),
            action: None,
        };
        let json = result.to_json();
        let err = consent_exit_error(status_of(&json), false, &json).expect("must fail");
        let envelope = err.to_error_json();
        assert_eq!(envelope["error_type"], json!("consent_not_actioned"));
        assert_eq!(envelope["status"], json!("detected_not_actioned"));
        assert_eq!(envelope["cmp"], json!("sourcepoint"));
        let obj = envelope.as_object().expect("object");
        assert!(obj.contains_key("action"), "action key dropped: {envelope}");
        assert!(obj.contains_key("error"), "{envelope}");
    }

    #[test]
    fn unit_160_allow_no_cmp_only_forgives_the_no_cmp_outcome() {
        assert!(consent_exit_error(ConsentStatus::NoCmpDetected, true, &json!({})).is_none());
        // A CMP that was found and could not be actioned is still a failure:
        // the caller asked for acceptance, which remains unconfirmed.
        let err = consent_exit_error(ConsentStatus::DetectedNotActioned, true, &json!({}))
            .expect("must still fail with --allow-no-cmp");
        assert_eq!(err.error_type(), "consent_not_actioned");
    }

    #[test]
    fn unit_160_consent_error_types_are_stable_discriminants() {
        assert_eq!(
            consent_exit_error(ConsentStatus::NoCmpDetected, false, &json!({}))
                .expect("error")
                .error_type(),
            "consent_no_cmp"
        );
        assert_eq!(
            consent_exit_error(ConsentStatus::DetectedNotActioned, false, &json!({}))
                .expect("error")
                .error_type(),
            "consent_not_actioned"
        );
    }

    /// The `no_cmp_detected` message must name the opt-out, so the caller who
    /// legitimately wants exit 0 learns the flag from the failure itself.
    #[test]
    fn unit_160_no_cmp_error_names_allow_no_cmp() {
        let err =
            consent_exit_error(ConsentStatus::NoCmpDetected, false, &json!({})).expect("error");
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
