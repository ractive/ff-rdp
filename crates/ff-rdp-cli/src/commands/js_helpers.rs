use std::time::{Duration, Instant};

use ff_rdp_core::{
    ActorId, EvalResult, Grip, LongStringActor, ProtocolError, WebConsoleActor,
    sanitize_for_terminal,
};
use serde_json::Value;

use super::connect_tab::{ConnectedTab, connect_and_get_target};
use crate::cli::args::Cli;
use crate::error::AppError;

/// Evaluate JavaScript on a tab and bail with an error if the result is an exception.
///
/// This is the standard "eval and check" helper used by most commands.
/// The `error_context` string is used as the fallback message when the
/// exception has no message field.
///
/// A JS exception is surfaced as `Err(AppError::User(..))` — routed through
/// the standard `{"error":…,"error_type":"User"}` JSON envelope, same as
/// every other command failure (iter-141 Theme E). Previously this printed
/// `error: <msg>` directly to stderr and returned `AppError::Exit(1)`, which
/// bypasses `main`'s JSON-envelope emission entirely: `ff-rdp dom 'div[[['`
/// printed the bare text `error: Document.querySelectorAll: 'div[[[' is not
/// a valid selector` with no JSON at all, while every other error path
/// (connection failures, protocol errors, ...) emits the envelope. A CSS
/// syntax error is exactly the kind of well-formed-but-invalid-input case
/// `AppError::User` exists for.
pub(crate) fn eval_or_bail(
    ctx: &mut ConnectedTab,
    console_actor: &ActorId,
    js: &str,
    error_context: &str,
) -> Result<EvalResult, AppError> {
    let eval_result = WebConsoleActor::evaluate_js_async(ctx.transport_mut(), console_actor, js)
        .map_err(AppError::from)?;

    if let Some(ref exc) = eval_result.exception {
        let msg = exc.message.as_deref().unwrap_or(error_context);
        return Err(AppError::User(sanitize_for_terminal(msg).into_owned()));
    }

    Ok(eval_result)
}

/// Sentinel prefix prepended to JSON.stringify results in the generated JS.
///
/// Used in geometry, snapshot, and DOM commands to distinguish structured JSON
/// output from plain strings that happen to start with `[` or `{`.
pub(crate) const JSON_SENTINEL: &str = "__FF_RDP_JSON__";

/// Resolve an eval result [`Grip`] to a [`Value`], fetching LongStrings as needed.
///
/// Commands that always prefix their JS output with [`JSON_SENTINEL`] use this
/// to strip the sentinel and parse the JSON payload.  Grips that are
/// `Null`/`Undefined` return [`Value::Null`] immediately.
pub(crate) fn resolve_result(ctx: &mut ConnectedTab, grip: &Grip) -> Result<Value, AppError> {
    let raw = match grip {
        Grip::Value(v) => v.clone(),
        Grip::LongString {
            actor,
            length,
            initial: _,
        } => {
            let full = LongStringActor::full_string(ctx.transport_mut(), actor.as_ref(), *length)
                .map_err(AppError::from)?;
            Value::String(full)
        }
        Grip::Null | Grip::Undefined => return Ok(Value::Null),
        other => other.to_json(),
    };

    // Strip the sentinel and parse the JSON payload.
    if let Some(s) = raw.as_str()
        && let Some(json_str) = s.strip_prefix(JSON_SENTINEL)
    {
        return serde_json::from_str::<Value>(json_str)
            .map_err(|e| AppError::from(anyhow::anyhow!("failed to parse JS result JSON: {e}")));
    }

    Ok(raw)
}

/// Escape a CSS selector for safe embedding in a **single-quoted** JS string literal.
///
/// Uses `serde_json::to_string` which handles backslashes, double quotes, newlines,
/// U+2028, U+2029, etc.  After stripping the outer double-quotes we additionally
/// escape single quotes (`'` → `\'`) since JSON encoding does not escape them but
/// they would terminate our single-quoted JS literal.
pub(crate) fn escape_selector(selector: &str) -> String {
    // serde_json::to_string is infallible for &str — the error branch is unreachable.
    let json_str = serde_json::to_string(selector)
        .unwrap_or_else(|e| unreachable!("serde_json::to_string(&str) is infallible: {e}"));
    // serde_json always wraps in double quotes: "value" — strip them.
    // The result is guaranteed to be at least 2 bytes (`""`), so slicing is safe.
    let inner = &json_str[1..json_str.len() - 1];
    // Escape single quotes for embedding in '…' JS literals.
    inner.replace('\'', "\\'")
}

// ---------------------------------------------------------------------------
// Unique-selector generation (iter-140 Theme A/F)
// ---------------------------------------------------------------------------

/// Source of a JS function `__ffrdpUniqueSelector(el)` that computes a
/// genuinely-unique CSS selector for a live DOM element: an `#id` shortcut
/// when available, otherwise a `tag:nth-child(N)` structural path walked up
/// to (but not including) `document.documentElement`.
///
/// This is the single source of truth for "turn a DOM node into a selector
/// safe to hand back to `document.querySelector` / `DomWalkerActor::query_selector`
/// unchanged" — used by:
/// - `dom.rs`'s ARIA-tree ref registration (`--ref e<N>` resolvers), so a ref
///   round-trips into a real CSS selector instead of a bare JS expression
///   (iter-140 Theme A bug #1).
/// - `resolve_disambiguated_target` below, so `--visible`/`--index` on
///   `click`/`type`/`styles` resolve to the exact chosen element.
/// - `page_map`'s landmark/form-submit extraction, so generated page-maps
///   hand back selectors that resolve to exactly one element (iter-140 Theme F).
///
/// Callers embed this once per IIFE and then call `__ffrdpUniqueSelector(el)`.
/// The function assumes `el` lives in the top-level document (no shadow-DOM
/// traversal) — consistent with every call site's existing scope.
pub(crate) const UNIQUE_SELECTOR_JS_FN: &str = r"
  function __ffrdpUniqueSelector(el) {
    if (!el || el.nodeType !== 1) return null;
    if (el === document.documentElement) return 'html';
    var path = [];
    var node = el;
    while (node && node.nodeType === 1 && node !== document.documentElement) {
      var part;
      if (node.id) {
        part = '#' + CSS.escape(node.id);
        path.unshift(part);
        break;
      }
      var sib = node;
      var nth = 1;
      while ((sib = sib.previousElementSibling)) { nth++; }
      part = node.nodeName.toLowerCase() + ':nth-child(' + nth + ')';
      path.unshift(part);
      node = node.parentElement;
    }
    return path.join(' > ');
  }
";

const POLL_INTERVAL_MS: u64 = 100;

// ---------------------------------------------------------------------------
// Auto-wait helpers
// ---------------------------------------------------------------------------

/// Generate a JavaScript polling expression that resolves to a structured
/// readiness result for the element matched by `escaped_selector`.
///
/// The expression evaluates to `null` when the element is not yet ready
/// (caller should retry) or a JSON string (prefixed with `JSON_SENTINEL`)
/// when ready or definitively failed.
///
/// Result shape (on success):
/// ```json
/// {"ready": true, "tag": "BUTTON", "text": "..."}
/// ```
/// Result shape (on transient-not-ready):
/// Returns JS `null` so the caller retries.
///
/// Result shape (on JS exception / stable-rect check):
/// Throws a JS `Error` whose `message` describes which sub-condition failed.
pub(crate) fn build_autowait_js(escaped_selector: &str, for_input: bool) -> String {
    let input_check = if for_input {
        r"
  if (el.disabled) throw new Error('element exists but is disabled');
  var tag = el.tagName.toLowerCase();
  var isEditable = tag === 'input' || tag === 'textarea' || tag === 'select' || el.isContentEditable;
  if (!isEditable) throw new Error('element exists but is not an input, textarea, select, or contenteditable');
  el.focus();"
    } else {
        ""
    };

    format!(
        r"(function() {{
  var el = document.querySelector('{escaped_selector}');
  if (!el) return null;

  // Visibility check (display:none / visibility:hidden)
  var style = window.getComputedStyle(el);
  if (style.display === 'none') throw new Error('element exists but has display:none');
  if (style.visibility === 'hidden') throw new Error('element exists but has visibility:hidden');
  if (style.opacity === '0') return null; // transitioning in, retry

  // Non-zero bounding rect
  var r1 = el.getBoundingClientRect();
  if (r1.width === 0 && r1.height === 0) return null; // not yet laid out, retry
  {input_check}

  return '{JSON_SENTINEL}' + JSON.stringify({{ready: true, tag: el.tagName, text: (el.textContent || '').trim().substring(0, 100)}});
}})()"
    )
}

/// Build a JS snippet that polls for rect stability (two consecutive reads within 50 ms
/// must be identical). Returns the sentinel-prefixed JSON if stable, or `null` to retry.
pub(crate) fn build_stability_check_js(escaped_selector: &str) -> String {
    format!(
        r"(function() {{
  var el = document.querySelector('{escaped_selector}');
  if (!el) return null;
  var r = el.getBoundingClientRect();
  return JSON.stringify([r.top, r.left, r.width, r.height]);
}})()"
    )
}

/// Auto-wait for an element to be ready (exist + visible + stable rect).
///
/// Default timeout: 5000 ms. Returns the sentinel-resolved JSON on success,
/// or an error describing which sub-condition failed.
///
/// When `for_input` is `true`, also checks `disabled === false` and that the
/// element is an input/textarea/contenteditable, and calls `.focus()`.
pub(crate) fn autowait_element(
    ctx: &mut ConnectedTab,
    console_actor: &ActorId,
    selector: &str,
    timeout_ms: u64,
    for_input: bool,
) -> Result<Value, AppError> {
    use std::time::{Duration, Instant};

    let escaped = escape_selector(selector);
    let readiness_js = build_autowait_js(&escaped, for_input);
    let stability_js = build_stability_check_js(&escaped);

    let timeout = Duration::from_millis(timeout_ms);
    let poll = Duration::from_millis(POLL_INTERVAL_MS);
    let started = Instant::now();

    // Phase 1: wait for element to exist + be visible + have non-zero rect.
    loop {
        if started.elapsed() >= timeout {
            // iter-140 Theme B: run one extra (cheap — only at the moment of
            // failure, never per-poll) diagnostic eval so the error names how
            // many elements matched and distinguishes "hidden" from
            // "not found" instead of the old undifferentiated
            // "not found / hidden / unstable" for every cause.
            let diag = diagnose_selector_failure(ctx, console_actor, selector, &escaped);
            return Err(AppError::Timeout(format!("{diag} after {timeout_ms}ms")));
        }

        let eval =
            WebConsoleActor::evaluate_js_async(ctx.transport_mut(), console_actor, &readiness_js)
                .map_err(AppError::from)?;

        if eval.exception.is_some() {
            // iter-140 Theme B (on-the-wire correction): `display:none` /
            // `visibility:hidden` on the DOM-order-0 match throws
            // immediately here — this branch returns on the *first* eval,
            // before the timeout loop above ever gets a chance to run
            // `diagnose_selector_failure`. A selector matching one hidden
            // element and one visible one (both legitimate real-page shapes:
            // an a11y-hidden duplicate, an inactive tab panel) used to report
            // only the bare JS message with no match count — exactly the
            // "distinguishes hidden from not-found" gap Theme B exists to
            // close, just reached from a different code path than the
            // timeout branch. Route through the same diagnostic so both
            // paths report match count / chosen index identically.
            let diag = diagnose_selector_failure(ctx, console_actor, selector, &escaped);
            let elapsed_ms = started.elapsed().as_millis();
            return Err(AppError::Timeout(format!(
                "{diag} (after {elapsed_ms}ms, timeout {timeout_ms}ms)"
            )));
        }

        if is_truthy(&eval.result) {
            break; // visible + non-zero rect
        }

        std::thread::sleep(poll);
    }

    // Phase 2: wait for stable rect (two consecutive reads must match).
    let stability_timeout = started.elapsed() + Duration::from_millis(500);
    let mut last_rect: Option<String> = None;

    loop {
        if started.elapsed() >= stability_timeout {
            // Stability check timed out — the element rect never stopped changing.
            return Err(AppError::Timeout(format!(
                "selector '{selector}' rect did not stabilise after {timeout_ms}ms"
            )));
        }
        if started.elapsed() >= timeout {
            let diag = diagnose_selector_failure(ctx, console_actor, selector, &escaped);
            return Err(AppError::Timeout(format!("{diag} after {timeout_ms}ms")));
        }

        let eval =
            WebConsoleActor::evaluate_js_async(ctx.transport_mut(), console_actor, &stability_js)
                .map_err(AppError::from)?;

        let current = match &eval.result {
            Grip::Value(v) => v.as_str().map(std::borrow::ToOwned::to_owned),
            _ => None,
        };

        if let Some(ref cur) = current {
            if let Some(ref prev) = last_rect
                && prev == cur
            {
                break; // stable
            }
            last_rect = current;
        }

        std::thread::sleep(Duration::from_millis(50));
    }

    Ok(Value::Null) // caller will proceed with the action
}

/// Diagnose *why* a selector never became ready, for a richer timeout error
/// than the old undifferentiated "not found / hidden / unstable" (iter-140
/// Theme B: gov.uk's `input[name=keywords]` matches two elements — `type`
/// silently took the hidden one and reported nothing about the other match).
///
/// Runs a single extra JS eval — cheap, since it only happens once, at the
/// moment `autowait_element` gives up — that reports the match count and
/// whether the DOM-order-0 match (the one autowait actually polled) is
/// hidden. Distinguishes:
/// - 0 matches → not found
/// - 1 match, hidden → a single permanently-hidden element (not an ambiguity
///   problem — just genuinely hidden)
/// - 2+ matches, chosen (index 0) hidden → the exact repro from the plan:
///   names the count and points at `--visible`/`--index` to recover
/// - 2+ matches, chosen (index 0) visible but unstable → same match-count
///   context, without wrongly implying the element can't be found at all
///
/// Best-effort: if the diagnostic eval itself throws or the transport drops,
/// falls back to the original undifferentiated message rather than masking
/// the real timeout with a second error.
fn diagnose_selector_failure(
    ctx: &mut ConnectedTab,
    console_actor: &ActorId,
    selector: &str,
    escaped_selector: &str,
) -> String {
    let js = format!(
        r"(function() {{
  var matches = document.querySelectorAll('{escaped_selector}');
  var n = matches.length;
  if (n === 0) return JSON.stringify({{matchCount: 0}});
  var el = matches[0];
  var r = el.getBoundingClientRect();
  var cs = window.getComputedStyle(el);
  var hidden = cs.display === 'none' || cs.visibility === 'hidden' || (r.width === 0 && r.height === 0);
  return JSON.stringify({{matchCount: n, hidden: hidden}});
}})()"
    );

    let diag = WebConsoleActor::evaluate_js_async(ctx.transport_mut(), console_actor, &js)
        .ok()
        .filter(|r| r.exception.is_none())
        .and_then(|r| match r.result {
            Grip::Value(v) => v
                .as_str()
                .and_then(|s| serde_json::from_str::<Value>(s).ok()),
            _ => None,
        });

    let Some(diag) = diag else {
        return format!("selector '{selector}' not ready (not found / hidden / unstable)");
    };

    let match_count = diag.get("matchCount").and_then(Value::as_u64).unwrap_or(0);
    if match_count == 0 {
        return format!("selector '{selector}' not ready — 0 elements matched (not found)");
    }
    let hidden = diag.get("hidden").and_then(Value::as_bool).unwrap_or(false);
    if match_count == 1 {
        return if hidden {
            format!("selector '{selector}' not ready — the 1 matching element is hidden")
        } else {
            format!(
                "selector '{selector}' not ready — matched 1 element (layout did not stabilise)"
            )
        };
    }
    let last_index = match_count - 1;
    if hidden {
        format!(
            "selector '{selector}' not ready — matched {match_count} elements, chose index 0 \
             which is hidden; pass --visible or --index 0..{last_index} to target a different match"
        )
    } else {
        format!(
            "selector '{selector}' not ready — matched {match_count} elements, chose index 0 \
             (layout did not stabilise); pass --index 0..{last_index} to target a different match"
        )
    }
}

// ---------------------------------------------------------------------------
// Match-policy disambiguation (iter-140 Theme B/C)
// ---------------------------------------------------------------------------

/// How to choose among multiple elements matched by a CSS selector, when the
/// caller explicitly asked to disambiguate via `--visible` / `--index N`.
///
/// The flag-less default path (`autowait_element` above) is entirely
/// unaffected by this enum — it keeps taking DOM-order index 0 with unchanged
/// timing, per [`crate::commands::click::ClickOptions::match_policy`]'s doc
/// comment. This only applies when a flag is passed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MatchPolicy {
    /// `--visible`: the first match that is not hidden (display:none,
    /// visibility:hidden, or a zero-size rect). Reports "no visible match"
    /// rather than silently falling back to a hidden one.
    Visible,
    /// `--index N`: the Nth match (0-based), regardless of visibility.
    Index(usize),
}

impl MatchPolicy {
    /// Build a `MatchPolicy` from the two mutually-exclusive CLI flags.
    ///
    /// clap's `conflicts_with` already prevents both being set on the command
    /// line; this is a defensive second check for callers that construct the
    /// combination programmatically (script runner steps, tests).
    pub(crate) fn from_flags(
        visible: bool,
        index: Option<usize>,
    ) -> Result<Option<Self>, AppError> {
        match (visible, index) {
            (true, Some(_)) => Err(AppError::User(
                "--visible and --index are mutually exclusive".to_string(),
            )),
            (true, None) => Ok(Some(Self::Visible)),
            (false, Some(n)) => Ok(Some(Self::Index(n))),
            (false, None) => Ok(None),
        }
    }
}

/// The result of resolving an ambiguous selector to one specific element.
pub(crate) struct ResolvedTarget {
    /// A genuinely-unique CSS selector for the chosen element (an `#id`
    /// shortcut or a `tag:nth-child(N)` structural path — see
    /// [`UNIQUE_SELECTOR_JS_FN`]), safe to feed into `document.querySelector`
    /// or `DomWalkerActor::query_selector` unchanged.
    pub(crate) selector: String,
    /// How many elements the original selector matched.
    pub(crate) match_count: usize,
    /// Which 0-based index was chosen among those matches.
    pub(crate) chosen_index: usize,
}

/// Build the JS that evaluates `escaped_selector`, applies `policy` to pick
/// one match, and returns that match's genuinely-unique selector alongside
/// the match count — or `{ok: false, matchCount}` when `policy` can't be
/// satisfied (no visible match / index out of range).
fn build_disambiguation_js(escaped_selector: &str, policy: MatchPolicy) -> String {
    let choose = match policy {
        MatchPolicy::Index(n) => format!("var chosenIndex = ({n} < matches.length) ? {n} : -1;"),
        MatchPolicy::Visible => r"
  var chosenIndex = -1;
  for (var i = 0; i < matches.length; i++) {
    var r = matches[i].getBoundingClientRect();
    var cs = window.getComputedStyle(matches[i]);
    var visible = r.width > 0 && r.height > 0 && cs.display !== 'none' && cs.visibility !== 'hidden';
    if (visible) { chosenIndex = i; break; }
  }"
        .to_string(),
    };

    format!(
        r"(function() {{
  {UNIQUE_SELECTOR_JS_FN}
  var matches = document.querySelectorAll('{escaped_selector}');
  var matchCount = matches.length;
  {choose}
  if (chosenIndex === -1) {{
    return '{JSON_SENTINEL}' + JSON.stringify({{ok: false, matchCount: matchCount}});
  }}
  var chosen = matches[chosenIndex];
  return '{JSON_SENTINEL}' + JSON.stringify({{
    ok: true,
    matchCount: matchCount,
    chosenIndex: chosenIndex,
    selector: __ffrdpUniqueSelector(chosen)
  }});
}})()"
    )
}

/// Resolve a possibly-ambiguous selector to the unique selector of a single
/// chosen element, per `policy` (iter-140 Theme B/C — `--visible`/`--index`
/// on `click`/`type`/`styles`).
///
/// Polls until `timeout_ms` elapses so a `--visible` match that appears after
/// the initial call (e.g. a hydrating SPA) is still caught, matching
/// `autowait_element`'s existing patience on the flag-less path.
pub(crate) fn resolve_disambiguated_target(
    ctx: &mut ConnectedTab,
    console_actor: &ActorId,
    selector: &str,
    policy: MatchPolicy,
    timeout_ms: u64,
) -> Result<ResolvedTarget, AppError> {
    use std::time::{Duration, Instant};

    let escaped = escape_selector(selector);
    let js = build_disambiguation_js(&escaped, policy);
    let timeout = Duration::from_millis(timeout_ms);
    let poll = Duration::from_millis(POLL_INTERVAL_MS);
    let started = Instant::now();

    let last_match_count: u64 = loop {
        let eval = WebConsoleActor::evaluate_js_async(ctx.transport_mut(), console_actor, &js)
            .map_err(AppError::from)?;
        if let Some(exc) = &eval.exception {
            let msg = exc
                .message
                .as_deref()
                .unwrap_or("selector evaluation failed");
            return Err(AppError::User(format!(
                "selector '{selector}' is invalid: {msg}"
            )));
        }
        let value = resolve_result(ctx, &eval.result)?;
        let match_count = value.get("matchCount").and_then(Value::as_u64).unwrap_or(0);
        if value.get("ok").and_then(Value::as_bool) == Some(true) {
            let resolved = value
                .get("selector")
                .and_then(Value::as_str)
                .map(str::to_owned)
                .ok_or_else(|| {
                    AppError::Internal(anyhow::anyhow!("disambiguation JS missing 'selector'"))
                })?;
            let chosen_index = value
                .get("chosenIndex")
                .and_then(Value::as_u64)
                .unwrap_or(0);
            return Ok(ResolvedTarget {
                selector: resolved,
                match_count: usize::try_from(match_count).unwrap_or(usize::MAX),
                chosen_index: usize::try_from(chosen_index).unwrap_or(usize::MAX),
            });
        }
        if started.elapsed() >= timeout {
            break match_count;
        }
        std::thread::sleep(poll);
    };

    Err(AppError::Timeout(match policy {
        MatchPolicy::Index(n) => format!(
            "selector '{selector}' matched {last_match_count} element(s) after {timeout_ms}ms — \
             index {n} is out of range (0..{})",
            last_match_count.saturating_sub(1)
        ),
        MatchPolicy::Visible if last_match_count == 0 => {
            format!("selector '{selector}' matched 0 elements (not found) after {timeout_ms}ms")
        }
        MatchPolicy::Visible => format!(
            "selector '{selector}' matched {last_match_count} element(s) after {timeout_ms}ms but \
             none are visible — pass --index 0..{} to target a hidden one",
            last_match_count.saturating_sub(1)
        ),
    }))
}

/// Connect, resolve `selector` per `policy`, and return the resolved unique
/// selector's `String` alone. For one-shot commands (`styles`/`cascade`/
/// `computed`) that don't otherwise need a held-open [`ConnectedTab`] before
/// dispatching to their own `run` function — those open their own connection
/// internally, so this makes (and drops) a short-lived one just for the
/// resolution step.
pub(crate) fn resolve_disambiguated_selector_standalone(
    cli: &Cli,
    selector: &str,
    policy: MatchPolicy,
    timeout_ms: u64,
) -> Result<String, AppError> {
    let mut ctx = connect_and_get_target(cli)?;
    let console_actor = ctx.target.console_actor.clone();
    let target =
        resolve_disambiguated_target(&mut ctx, &console_actor, selector, policy, timeout_ms)?;
    Ok(target.selector)
}

// ---------------------------------------------------------------------------
// Pointer-event dispatch
// ---------------------------------------------------------------------------

/// Dispatch mode for `click`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DispatchMode {
    /// Full pointer-event sequence: pointerover, pointerenter, pointerdown,
    /// pointerup, click (plus matching mouse events). This is the default.
    Pointer,
    /// Legacy mouse-event sequence: mouseover, mouseenter, mousedown, mouseup, click.
    Legacy,
    /// Only dispatch a synthetic `click` event (pre-iter-59 behaviour).
    ClickOnly,
}

/// Build a JS expression that dispatches the appropriate event sequence on the
/// element matched by `escaped_selector`, then returns a sentinel-prefixed JSON.
///
/// The `entered` sentinel is set before the action so D2 can detect partial success.
pub(crate) fn build_click_js(escaped_selector: &str, mode: DispatchMode) -> String {
    let event_dispatch: &str = match mode {
        DispatchMode::Pointer => {
            r"
  // Pointer event sequence (Radix / Headless-UI compatible).
  var opts = {bubbles: true, cancelable: true, view: window, pointerType: 'mouse', isPrimary: true, button: 0, buttons: 1};
  var mopts = {bubbles: true, cancelable: true, view: window, button: 0, buttons: 1};
  var upOpts = {bubbles: true, cancelable: true, view: window, pointerType: 'mouse', isPrimary: true, button: 0, buttons: 0};
  var upMopts = {bubbles: true, cancelable: true, view: window, button: 0, buttons: 0};
  el.dispatchEvent(new PointerEvent('pointerover', opts));
  el.dispatchEvent(new PointerEvent('pointerenter', {...opts, bubbles: false}));
  el.dispatchEvent(new MouseEvent('mouseover', mopts));
  el.dispatchEvent(new MouseEvent('mouseenter', {...mopts, bubbles: false}));
  el.dispatchEvent(new PointerEvent('pointerdown', opts));
  el.dispatchEvent(new MouseEvent('mousedown', mopts));
  el.dispatchEvent(new PointerEvent('pointerup', upOpts));
  el.dispatchEvent(new MouseEvent('mouseup', upMopts));
  el.dispatchEvent(new MouseEvent('click', upMopts));"
        }
        DispatchMode::Legacy => {
            r"
  var mopts = {bubbles: true, cancelable: true, view: window};
  el.dispatchEvent(new MouseEvent('mouseover', mopts));
  el.dispatchEvent(new MouseEvent('mouseenter', {...mopts, bubbles: false}));
  el.dispatchEvent(new MouseEvent('mousedown', mopts));
  el.dispatchEvent(new MouseEvent('mouseup', mopts));
  el.dispatchEvent(new MouseEvent('click', mopts));"
        }
        DispatchMode::ClickOnly => "  el.click();",
    };

    format!(
        r"(function() {{
  var entered = false;
  var el = document.querySelector('{escaped_selector}');
  if (!el) throw new Error('Element not found: {escaped_selector} — use ff-rdp dom SELECTOR --count to verify the selector matches');
  entered = true;
  {event_dispatch}
  return '{JSON_SENTINEL}' + JSON.stringify({{clicked: true, entered: entered, tag: el.tagName, text: (el.textContent || '').trim().substring(0, 100)}});
}})()"
    )
}

// ---------------------------------------------------------------------------
// Wait-for predicate helpers
// ---------------------------------------------------------------------------

/// A single post-action wait predicate.
#[derive(Debug, Clone)]
pub(crate) enum WaitForPredicate<'a> {
    /// `selector:<css>` — element must exist in the DOM.
    Selector(&'a str),
    /// `text:<substr>` — substring must appear in `document.body.innerText`.
    Text(&'a str),
    /// `url:<regex>` — current URL must match the regex.
    Url(&'a str),
    /// `gone:<css>` — element must NOT exist in the DOM.
    Gone(&'a str),
}

impl<'a> WaitForPredicate<'a> {
    /// Parse a `--wait-for` argument string into a [`WaitForPredicate`].
    pub(crate) fn parse(s: &'a str) -> Result<Self, AppError> {
        if let Some(rest) = s.strip_prefix("selector:") {
            Ok(Self::Selector(rest))
        } else if let Some(rest) = s.strip_prefix("text:") {
            Ok(Self::Text(rest))
        } else if let Some(rest) = s.strip_prefix("url:") {
            Ok(Self::Url(rest))
        } else if let Some(rest) = s.strip_prefix("gone:") {
            Ok(Self::Gone(rest))
        } else {
            Err(AppError::User(format!(
                "--wait-for predicate must start with 'selector:', 'text:', 'url:', or 'gone:' — got: {s:?}"
            )))
        }
    }

    /// Build a JavaScript expression that returns truthy when the predicate is satisfied.
    pub(crate) fn to_js(&self) -> Result<String, AppError> {
        Ok(match self {
            Self::Selector(sel) => {
                let esc = escape_selector(sel);
                format!("document.querySelector('{esc}') !== null")
            }
            Self::Text(text) => {
                let esc = serde_json::to_string(text).map_err(|e| {
                    AppError::from(anyhow::anyhow!("failed to encode wait-for text: {e}"))
                })?;
                format!("(document.body && document.body.innerText.includes({esc}))")
            }
            Self::Url(pattern) => {
                let esc = serde_json::to_string(pattern).map_err(|e| {
                    AppError::from(anyhow::anyhow!("failed to encode wait-for url: {e}"))
                })?;
                format!("(new RegExp({esc}).test(window.location.href))")
            }
            Self::Gone(sel) => {
                let esc = escape_selector(sel);
                format!("document.querySelector('{esc}') === null")
            }
        })
    }

    fn describe(&self) -> String {
        match self {
            Self::Selector(s) => format!("selector:{s}"),
            Self::Text(t) => format!("text:{t}"),
            Self::Url(u) => format!("url:{u}"),
            Self::Gone(s) => format!("gone:{s}"),
        }
    }
}

/// Poll all `predicates` until all are satisfied or `timeout_ms` elapses.
pub(crate) fn wait_for_predicates(
    ctx: &mut ConnectedTab,
    console_actor: &ActorId,
    predicates: &[WaitForPredicate<'_>],
    timeout_ms: u64,
) -> Result<(), AppError> {
    use std::time::{Duration, Instant};

    if predicates.is_empty() {
        return Ok(());
    }

    let timeout = Duration::from_millis(timeout_ms);
    let poll = Duration::from_millis(POLL_INTERVAL_MS);
    let started = Instant::now();

    // Build JS expressions once up front.
    let js_exprs: Vec<String> = predicates
        .iter()
        .map(WaitForPredicate::to_js)
        .collect::<Result<_, _>>()?;

    loop {
        if started.elapsed() >= timeout {
            let unmet: Vec<String> = predicates.iter().map(WaitForPredicate::describe).collect();
            return Err(AppError::Timeout(format!(
                "wait-for predicates not satisfied after {timeout_ms}ms: {}",
                unmet.join(", ")
            )));
        }

        let mut all_met = true;
        for (js, predicate) in js_exprs.iter().zip(predicates.iter()) {
            let eval = WebConsoleActor::evaluate_js_async(ctx.transport_mut(), console_actor, js)
                .map_err(AppError::from)?;
            if let Some(ref exc) = eval.exception {
                let msg = exc
                    .message
                    .as_deref()
                    .unwrap_or("wait-for predicate threw an exception");
                return Err(AppError::User(format!(
                    "wait-for predicate '{}' threw: {msg}",
                    predicate.describe()
                )));
            }
            if !is_truthy(&eval.result) {
                all_met = false;
                break;
            }
        }

        if all_met {
            return Ok(());
        }

        std::thread::sleep(poll);
    }
}

// ---------------------------------------------------------------------------
// Settle helper (network + DOM idle)
// ---------------------------------------------------------------------------

/// Inject and wait for network+DOM settle: no XHR/fetch in flight for 500 ms AND
/// no DOM mutations for 200 ms.
///
/// On CSP injection failure, falls back to a 1 s sleep and emits
/// `meta.settle_method = "sleep"` via the returned string.
pub(crate) fn settle_page(
    ctx: &mut ConnectedTab,
    console_actor: &ActorId,
    timeout_ms: u64,
) -> Result<SettleMethod, AppError> {
    // Attempt to inject network monitoring + MutationObserver.
    let inject_js = r"(function() {
  try {
    if (window.__ffrdpSettleInit) return '__ok__';
    window.__ffrdpInflight = 0;
    window.__ffrdpLastInflightZero = Date.now();
    var origSend = XMLHttpRequest.prototype.send;
    XMLHttpRequest.prototype.send = function() {
      window.__ffrdpInflight++;
      this.addEventListener('loadend', function() {
        window.__ffrdpInflight = Math.max(0, window.__ffrdpInflight - 1);
        if (window.__ffrdpInflight === 0) { window.__ffrdpLastInflightZero = Date.now(); }
      });
      origSend.apply(this, arguments);
    };
    var origFetch = window.fetch;
    window.fetch = function() {
      window.__ffrdpInflight++;
      return origFetch.apply(this, arguments).finally(function() {
        window.__ffrdpInflight = Math.max(0, window.__ffrdpInflight - 1);
        if (window.__ffrdpInflight === 0) { window.__ffrdpLastInflightZero = Date.now(); }
      });
    };
    window.__ffrdpLastMutation = Date.now();
    window.__ffrdpMutObs = new MutationObserver(function() { window.__ffrdpLastMutation = Date.now(); });
    window.__ffrdpMutObs.observe(document.documentElement, {childList: true, subtree: true, attributes: true});
    window.__ffrdpSettleInit = true;
    return '__ok__';
  } catch(e) { return '__csp__'; }
})()";

    let eval = WebConsoleActor::evaluate_js_async(ctx.transport_mut(), console_actor, inject_js)
        .map_err(AppError::from)?;

    let inject_ok = match &eval.result {
        Grip::Value(v) => v.as_str() == Some("__ok__"),
        _ => false,
    };

    if !inject_ok {
        // CSP blocked injection — fall back to 1 s sleep.
        std::thread::sleep(std::time::Duration::from_secs(1));
        return Ok(SettleMethod::Sleep);
    }

    // Poll for idle state: inflight == 0 for 500ms sustained AND no mutation for 200ms.
    let idle_check_js = r"(function() {
  var now = Date.now();
  var inflight = (window.__ffrdpInflight || 0);
  var networkIdle = inflight === 0 && (now - (window.__ffrdpLastInflightZero || 0)) >= 500;
  var domOk = (now - (window.__ffrdpLastMutation || 0)) >= 200;
  return networkIdle && domOk;
})()";

    match poll_js_condition(
        ctx,
        console_actor,
        idle_check_js,
        timeout_ms,
        "settle check threw",
        &format!("page did not settle within {timeout_ms}ms"),
    ) {
        Ok(_) => Ok(SettleMethod::NetworkIdle),
        Err(AppError::Timeout(_)) => Ok(SettleMethod::NetworkIdleTimeout),
        Err(e) => Err(e),
    }
}

/// How the settle completed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SettleMethod {
    /// Network idle + DOM idle (normal path).
    NetworkIdle,
    /// Network/DOM idle injection succeeded but idle condition was not met within the timeout.
    NetworkIdleTimeout,
    /// CSP blocked injection — fell back to a 1 s sleep.
    Sleep,
}

impl SettleMethod {
    /// Serialised string used in JSON meta output.
    pub(crate) fn as_meta_str(self) -> &'static str {
        match self {
            Self::NetworkIdle => "network_idle",
            Self::NetworkIdleTimeout => "network_idle_timeout",
            Self::Sleep => "sleep_fallback",
        }
    }
}

/// Poll a JS expression until it returns a truthy value or the timeout expires.
///
/// Returns the elapsed time in milliseconds on success.  Returns
/// `Err(AppError::User(..))` (routed through the JSON error envelope,
/// iter-141 Theme E — see [`eval_or_bail`]'s doc comment) if a JS exception
/// is thrown, or `Err(AppError::Timeout(timeout_context))` if the timeout
/// expires.
///
/// A timeout of 0 means the condition is evaluated once; if falsy, a timeout
/// error is returned immediately.
///
/// - `error_context`: used as a fallback message when a JS exception has no message.
/// - `timeout_context`: carried inside the returned `AppError::Timeout` when the timeout expires.
pub(crate) fn poll_js_condition(
    ctx: &mut ConnectedTab,
    console_actor: &ActorId,
    js: &str,
    timeout_ms: u64,
    error_context: &str,
    timeout_context: &str,
) -> Result<u64, AppError> {
    let timeout = Duration::from_millis(timeout_ms);
    let poll = Duration::from_millis(POLL_INTERVAL_MS);
    let started = Instant::now();

    loop {
        // A transport-level recv timeout (Firefox didn't answer within the
        // socket's read-timeout window) is treated the same as "condition not
        // yet observed" rather than a hard error: it falls through to the
        // deadline check below, which raises the descriptive
        // `timeout_context` message. Without this, `AppError::from` would
        // convert `ProtocolError::Timeout` into a generic, contextless
        // `RdpTimeout { phase: "recv", after_ms: 0 }` ("timed out after 0ms
        // (phase: recv)") that tells the user nothing about which condition
        // failed to become true.
        let eval_result =
            match WebConsoleActor::evaluate_js_async(ctx.transport_mut(), console_actor, js) {
                Ok(result) => Some(result),
                Err(ProtocolError::Timeout) => None,
                Err(e) => return Err(AppError::from(e)),
            };

        if let Some(eval_result) = eval_result {
            if let Some(ref exc) = eval_result.exception {
                let msg = match exc.message.as_deref() {
                    Some(m) => format!("{error_context}: {m}"),
                    None => error_context.to_owned(),
                };
                return Err(AppError::User(sanitize_for_terminal(&msg).into_owned()));
            }

            if is_truthy(&eval_result.result) {
                return Ok(u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX));
            }
        }

        // Check timeout before sleeping to avoid an unnecessary extra poll interval.
        if started.elapsed() >= timeout {
            return Err(AppError::Timeout(timeout_context.to_owned()));
        }

        std::thread::sleep(poll);
    }
}

/// Check whether a JavaScript [`Grip`] value is truthy.
///
/// Follows JavaScript truthiness rules: `null`, `undefined`, `NaN`, `-0`,
/// `false`, `0`, and empty string are falsy; everything else is truthy.
pub(crate) fn is_truthy(grip: &Grip) -> bool {
    match grip {
        // Null, Undefined, NaN, and -0 are all falsy in JavaScript.
        Grip::Null | Grip::Undefined | Grip::NaN | Grip::NegZero => false,
        Grip::Value(v) => {
            if let Some(b) = v.as_bool() {
                return b;
            }
            if let Some(n) = v.as_f64() {
                return n != 0.0;
            }
            if let Some(s) = v.as_str() {
                return !s.is_empty();
            }
            // Objects and arrays are truthy.
            !v.is_null()
        }
        // Infinity, -Infinity, LongString, Object are all truthy.
        Grip::Inf | Grip::NegInf | Grip::LongString { .. } | Grip::Object { .. } => true,
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn escape_selector_handles_special_chars() {
        assert_eq!(escape_selector("a\nb"), r"a\nb");
        assert_eq!(escape_selector(r"a\b"), r"a\\b");
        assert_eq!(escape_selector(r#"a"b"#), r#"a\"b"#);
    }

    #[test]
    fn escape_selector_escapes_single_quotes() {
        assert_eq!(
            escape_selector("div[data-name='test']"),
            r"div[data-name=\'test\']"
        );
    }

    #[test]
    fn escape_selector_plain() {
        assert_eq!(escape_selector("button.submit"), "button.submit");
        assert_eq!(escape_selector("input[name=email]"), "input[name=email]");
    }

    #[test]
    fn match_policy_from_flags_visible_only() {
        assert_eq!(
            MatchPolicy::from_flags(true, None).unwrap(),
            Some(MatchPolicy::Visible)
        );
    }

    #[test]
    fn match_policy_from_flags_index_only() {
        assert_eq!(
            MatchPolicy::from_flags(false, Some(2)).unwrap(),
            Some(MatchPolicy::Index(2))
        );
    }

    #[test]
    fn match_policy_from_flags_neither_is_none() {
        assert_eq!(MatchPolicy::from_flags(false, None).unwrap(), None);
    }

    #[test]
    fn match_policy_from_flags_both_is_error() {
        // Defensive check for programmatic callers (script runner steps,
        // tests) that construct the combination without going through
        // clap's `conflicts_with`, which already prevents this on the CLI.
        let err = MatchPolicy::from_flags(true, Some(1)).unwrap_err();
        let AppError::User(msg) = &err else {
            panic!("expected AppError::User, got: {err:?}");
        };
        assert!(
            msg.contains("mutually exclusive"),
            "expected a mutually-exclusive User error, got: {msg:?}"
        );
    }

    #[test]
    fn is_truthy_true_values() {
        assert!(is_truthy(&Grip::Value(json!(true))));
        assert!(is_truthy(&Grip::Value(json!(1))));
        assert!(is_truthy(&Grip::Value(json!("hello"))));
        assert!(is_truthy(&Grip::Inf));
        assert!(is_truthy(&Grip::NegInf));
    }

    #[test]
    fn is_truthy_false_values() {
        assert!(!is_truthy(&Grip::Null));
        assert!(!is_truthy(&Grip::Undefined));
        assert!(!is_truthy(&Grip::Value(json!(false))));
        assert!(!is_truthy(&Grip::Value(json!(0))));
        assert!(!is_truthy(&Grip::Value(json!(""))));
        assert!(!is_truthy(&Grip::NaN));
        assert!(!is_truthy(&Grip::NegZero));
    }

    /// Regression test: a transport-level recv timeout inside
    /// `poll_js_condition` (Firefox never answers `evaluateJSAsync`) must
    /// surface as `AppError::Timeout` with the caller's descriptive
    /// `timeout_context` — not the generic, contextless
    /// `AppError::RdpTimeout { phase: "recv", after_ms: 0 }` that
    /// `AppError::from(ProtocolError::Timeout)` would otherwise produce
    /// ("timed out after 0ms (phase: recv)").
    #[test]
    fn poll_js_condition_recv_timeout_surfaces_descriptive_message() {
        use std::io::Write as _;
        use std::net::TcpListener;

        use ff_rdp_core::transport::{RdpTransport, encode_frame, recv_from};

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();

        // Mock Firefox: send the greeting, then read (and drop) the
        // `evaluateJSAsync` request but never reply — the transport's read
        // timeout is what must fire, not a clean protocol response.
        let server_handle = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut writer = stream.try_clone().unwrap();
            let mut reader = std::io::BufReader::new(stream);

            let greeting = json!({
                "from": "root",
                "applicationType": "browser",
                "traits": {}
            });
            writer
                .write_all(encode_frame(&serde_json::to_string(&greeting).unwrap()).as_bytes())
                .unwrap();

            let _ = recv_from(&mut reader);
            // No reply sent — the client's read timeout must fire.
            std::thread::sleep(Duration::from_secs(1));
        });

        // Short read timeout so the test completes quickly; this is the same
        // timeout that governs every `recv()` call after connect, including
        // the one inside `evaluate_js_async`.
        let transport =
            RdpTransport::connect("127.0.0.1", port, Duration::from_millis(150)).unwrap();

        let console_actor = ActorId::from("conn0/console1");
        let mut ctx = ConnectedTab::for_test(transport, console_actor.clone());

        // A tiny loop budget: by the time the first `evaluate_js_async` call
        // returns (after the ~150ms socket read timeout), the loop's own
        // deadline has already passed, so it returns immediately instead of
        // looping again.
        let result = poll_js_condition(
            &mut ctx,
            &console_actor,
            "true",
            10,
            "condition threw",
            "condition-not-met descriptive message",
        );

        server_handle.join().unwrap();

        match result {
            Err(AppError::Timeout(msg)) => {
                assert!(
                    msg.contains("condition-not-met descriptive message"),
                    "expected the descriptive timeout_context, got: {msg:?}"
                );
            }
            other => panic!("expected AppError::Timeout, got: {other:?}"),
        }
    }
}
