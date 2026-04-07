use ff_rdp_core::{Grip, LongStringActor};
use serde_json::Value;

use crate::error::AppError;

use super::connect_tab::ConnectedTab;

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

#[cfg(test)]
mod tests {
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
}
