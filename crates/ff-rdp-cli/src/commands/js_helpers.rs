use std::time::{Duration, Instant};

use ff_rdp_core::{ActorId, EvalResult, Grip, LongStringActor, WebConsoleActor};
use serde_json::Value;

use super::connect_tab::ConnectedTab;
use crate::error::AppError;

/// Evaluate JavaScript on a tab and bail with an error if the result is an exception.
///
/// This is the standard "eval and check" helper used by most commands.
/// The `error_context` string is used as the fallback message when the
/// exception has no message field.
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
        eprintln!("error: {msg}");
        return Err(AppError::Exit(1));
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
  var isEditable = tag === 'input' || tag === 'textarea' || el.isContentEditable;
  if (!isEditable) throw new Error('element exists but is not an input, textarea, or contenteditable');
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
            return Err(AppError::Timeout(format!(
                "selector '{selector}' not found or not ready after {timeout_ms}ms — element exists but not visible"
            )));
        }

        let eval =
            WebConsoleActor::evaluate_js_async(ctx.transport_mut(), console_actor, &readiness_js)
                .map_err(AppError::from)?;

        if let Some(ref exc) = eval.exception {
            let msg = exc
                .message
                .as_deref()
                .unwrap_or("element readiness check failed");
            return Err(AppError::Timeout(format!(
                "selector '{selector}' not ready after {timeout_ms}ms: {msg}"
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
        if started.elapsed() >= timeout || started.elapsed() >= stability_timeout {
            // Stable-rect check timed out — treat as ready anyway (element existed).
            break;
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
pub(crate) fn build_click_js(
    escaped_selector: &str,
    mode: DispatchMode,
    keyboard_fallback: bool,
) -> String {
    let event_dispatch: &str = match mode {
        DispatchMode::Pointer => {
            r"
  // Pointer event sequence (Radix / Headless-UI compatible).
  var opts = {bubbles: true, cancelable: true, view: window, pointerType: 'mouse', isPrimary: true};
  var mopts = {bubbles: true, cancelable: true, view: window};
  el.dispatchEvent(new PointerEvent('pointerover', opts));
  el.dispatchEvent(new PointerEvent('pointerenter', {...opts, bubbles: false}));
  el.dispatchEvent(new MouseEvent('mouseover', mopts));
  el.dispatchEvent(new MouseEvent('mouseenter', {...mopts, bubbles: false}));
  el.dispatchEvent(new PointerEvent('pointerdown', opts));
  el.dispatchEvent(new MouseEvent('mousedown', mopts));
  el.dispatchEvent(new PointerEvent('pointerup', opts));
  el.dispatchEvent(new MouseEvent('mouseup', mopts));
  el.dispatchEvent(new MouseEvent('click', mopts));"
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

    // Keyboard-activation fallback: if `aria-haspopup` or `role=button` and
    // no state change was observed within 200 ms, fire Enter key events.
    let kb_fallback = if keyboard_fallback {
        r"
  // Keyboard activation fallback (B2).
  var needsKbCheck = el.hasAttribute('aria-haspopup') || el.getAttribute('role') === 'button';
  if (needsKbCheck) {
    var stateBefore = el.getAttribute('data-state') + '|' + el.getAttribute('aria-expanded');
    var observed = false;
    var obs = new MutationObserver(function() { observed = true; });
    obs.observe(el, {attributes: true, subtree: true, childList: true});
    // We rely on the 200ms sleep happening outside JS (poll loop).
    // Store a marker for the outer poll to check.
    el._ffrdpKbFallback = {stateBefore: stateBefore, obs: obs};
  }"
    } else {
        ""
    };

    format!(
        r"(function() {{
  var entered = false;
  var el = document.querySelector('{escaped_selector}');
  if (!el) throw new Error('Element not found: {escaped_selector} — use ff-rdp dom SELECTOR --count to verify the selector matches');
  entered = true;
  {event_dispatch}
  {kb_fallback}
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
        for js in &js_exprs {
            let eval = WebConsoleActor::evaluate_js_async(ctx.transport_mut(), console_actor, js)
                .map_err(AppError::from)?;
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
    var origOpen = XMLHttpRequest.prototype.open;
    var origSend = XMLHttpRequest.prototype.send;
    XMLHttpRequest.prototype.send = function() {
      window.__ffrdpInflight++;
      var self = this;
      this.addEventListener('loadend', function() { window.__ffrdpInflight = Math.max(0, window.__ffrdpInflight - 1); });
      origSend.apply(this, arguments);
    };
    var origFetch = window.fetch;
    window.fetch = function() {
      window.__ffrdpInflight++;
      return origFetch.apply(this, arguments).finally(function() {
        window.__ffrdpInflight = Math.max(0, window.__ffrdpInflight - 1);
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

    // Poll for idle state: inflightCount == 0 for 500ms AND no mutation for 200ms.
    let idle_check_js = r"(function() {
  var inflightOk = (window.__ffrdpInflight || 0) === 0;
  var domOk = (Date.now() - (window.__ffrdpLastMutation || 0)) >= 200;
  return inflightOk && domOk;
})()";

    let _ = poll_js_condition(
        ctx,
        console_actor,
        idle_check_js,
        timeout_ms,
        "settle check threw",
        &format!("page did not settle within {timeout_ms}ms"),
    );

    Ok(SettleMethod::NetworkIdle)
}

/// How the settle completed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SettleMethod {
    /// Network idle + DOM idle (normal path).
    NetworkIdle,
    /// CSP blocked injection — fell back to a 1 s sleep.
    Sleep,
}

/// Poll a JS expression until it returns a truthy value or the timeout expires.
///
/// Returns the elapsed time in milliseconds on success.  Returns
/// `Err(AppError::Exit(1))` if a JS exception is thrown, or
/// `Err(AppError::Timeout(timeout_context))` if the timeout expires.
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
        let eval_result =
            WebConsoleActor::evaluate_js_async(ctx.transport_mut(), console_actor, js)
                .map_err(AppError::from)?;

        if let Some(ref exc) = eval_result.exception {
            if let Some(msg) = exc.message.as_deref() {
                eprintln!("error: {error_context}: {msg}");
            } else {
                eprintln!("error: {error_context}");
            }
            return Err(AppError::Exit(1));
        }

        if is_truthy(&eval_result.result) {
            return Ok(u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX));
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
}
