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
pub fn escape_selector(selector: &str) -> String {
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

/// Poll a JS expression until it returns a truthy value or the timeout expires.
///
/// Returns the elapsed time in milliseconds on success.  Returns
/// `Err(AppError::Exit(1))` if a JS exception is thrown or the timeout expires.
///
/// - `error_context`: used as a fallback message when a JS exception has no message.
/// - `timeout_context`: printed to stderr when the timeout expires.
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
            let msg = exc.message.as_deref().unwrap_or(error_context);
            eprintln!("error: {msg}");
            return Err(AppError::Exit(1));
        }

        if is_truthy(&eval_result.result) {
            return Ok(u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX));
        }

        if started.elapsed() >= timeout {
            eprintln!("error: {timeout_context}");
            return Err(AppError::Exit(1));
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
