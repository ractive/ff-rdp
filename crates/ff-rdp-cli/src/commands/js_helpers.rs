/// Escape a CSS selector for safe embedding in a **single-quoted** JS string literal.
///
/// Uses `serde_json::to_string` which handles backslashes, double quotes, newlines,
/// U+2028, U+2029, etc.  After stripping the outer double-quotes we additionally
/// escape single quotes (`'` → `\'`) since JSON encoding does not escape them but
/// they would terminate our single-quoted JS literal.
pub fn escape_selector(selector: &str) -> String {
    let json_str =
        serde_json::to_string(selector).expect("serde_json::to_string cannot fail for &str");
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
