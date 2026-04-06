/// Escape a CSS selector for safe embedding in a JS string literal.
///
/// Uses `serde_json::to_string` which handles all JS-problematic characters
/// (backslashes, quotes, newlines, U+2028, U+2029, etc.) then strips the
/// outer double-quotes since we embed into a single-quoted JS literal.
pub fn escape_selector(selector: &str) -> String {
    let json_str = serde_json::to_string(selector).unwrap_or_default();
    // serde_json wraps in double quotes: "value" — strip them
    json_str[1..json_str.len() - 1].to_owned()
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
    fn escape_selector_plain() {
        assert_eq!(escape_selector("button.submit"), "button.submit");
        assert_eq!(escape_selector("input[name=email]"), "input[name=email]");
    }
}
