use std::time::{Duration, Instant};

use ff_rdp_core::{ActorId, Grip, RdpTransport, WebConsoleActor};

use crate::error::AppError;

/// Default polling interval in milliseconds.
pub(crate) const POLL_INTERVAL_MS: u64 = 100;

/// Result returned by [`poll_js_condition`].
pub(crate) struct PollResult {
    /// `true` if the JS condition became truthy within the timeout.
    pub matched: bool,
    /// Wall-clock time elapsed during polling, saturated at [`u64::MAX`].
    pub elapsed_ms: u64,
}

/// Poll a JS expression until it evaluates to a truthy value or the timeout
/// expires.
///
/// # Behavior
///
/// - On a JS exception: prints an error to stderr and returns
///   [`AppError::Exit(1)`] immediately (the condition will never resolve).
/// - On truthy result: returns [`PollResult { matched: true, .. }`].
/// - On timeout: returns [`PollResult { matched: false, .. }`] — the caller
///   decides whether that is an error.
///
/// # Parameters
///
/// - `transport` – active RDP transport.
/// - `console_actor` – actor ID string for the `WebConsoleActor`.
/// - `js` – JavaScript expression to evaluate each iteration.
/// - `timeout_ms` – maximum total wait time.
/// - `interval_ms` – sleep duration between polls; pass [`POLL_INTERVAL_MS`]
///   for the default.
pub(crate) fn poll_js_condition(
    transport: &mut RdpTransport,
    console_actor: &ActorId,
    js: &str,
    timeout_ms: u64,
    interval_ms: u64,
) -> Result<PollResult, AppError> {
    let timeout = Duration::from_millis(timeout_ms);
    let poll = Duration::from_millis(interval_ms);
    let started = Instant::now();

    loop {
        let eval_result = WebConsoleActor::evaluate_js_async(transport, console_actor, js)
            .map_err(AppError::from)?;

        // A JS exception (e.g. SyntaxError from an invalid CSS selector) will
        // never resolve to truthy — return an error immediately.
        if let Some(exc) = &eval_result.exception {
            let msg = exc
                .message
                .as_deref()
                .unwrap_or("JS exception during poll condition");
            eprintln!("error: poll condition aborted due to JS exception: {msg}");
            return Err(AppError::Exit(1));
        }

        let elapsed_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);

        if is_truthy(&eval_result.result) {
            return Ok(PollResult {
                matched: true,
                elapsed_ms,
            });
        }

        if started.elapsed() >= timeout {
            return Ok(PollResult {
                matched: false,
                elapsed_ms,
            });
        }

        std::thread::sleep(poll);
    }
}

/// Returns `true` if a [`Grip`] is truthy by JavaScript semantics.
///
/// - `null`, `undefined`, `NaN`, `-0`, `false`, `0`, and `""` are falsy.
/// - Everything else (including objects, arrays, non-empty strings, non-zero
///   numbers, `Infinity`, `-Infinity`, and `LongString`) is truthy.
pub(crate) fn is_truthy(grip: &Grip) -> bool {
    match grip {
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
        Grip::Inf | Grip::NegInf | Grip::LongString { .. } | Grip::Object { .. } => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn is_truthy_true_values() {
        assert!(is_truthy(&Grip::Value(json!(true))));
        assert!(is_truthy(&Grip::Value(json!(1))));
        assert!(is_truthy(&Grip::Value(json!("hello"))));
        assert!(is_truthy(&Grip::Inf));
        assert!(is_truthy(&Grip::NegInf));
        assert!(is_truthy(&Grip::LongString {
            actor: "conn0/longString1".into(),
            length: 100,
            initial: "abc".into(),
        }));
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
