use std::borrow::Cow;

/// Sanitize a string for safe output to a terminal.
///
/// Replaces ASCII control characters — except `\n` (0x0A) and `\t` (0x09) —
/// with `?`.  This prevents terminal escape injection attacks where an attacker
/// could embed ANSI escape sequences (e.g. cursor movement, color codes) or
/// other control characters in server-supplied strings that get printed to
/// stderr.
///
/// Returns a [`Cow::Borrowed`] when no replacement is needed (the common
/// case), avoiding any allocation.
pub fn sanitize_for_terminal(s: &str) -> Cow<'_, str> {
    /// Returns `true` for ASCII control characters that are unsafe to emit
    /// in a terminal context.  Allows `\t` (0x09) and `\n` (0x0A).
    fn is_unsafe_ctrl(b: u8) -> bool {
        matches!(b, 0x00..=0x08 | 0x0B..=0x1F | 0x7F)
    }

    // Fast path: check whether any byte needs replacing before allocating.
    let needs_sanitize = s.bytes().any(is_unsafe_ctrl);
    if !needs_sanitize {
        return Cow::Borrowed(s);
    }

    // Slow path: replace each unsafe control byte with '?'.
    // We operate character-by-character to preserve valid multi-byte UTF-8
    // sequences — a multi-byte sequence can never contain a lone ASCII byte,
    // so replacing ASCII control bytes is safe without examining UTF-8 structure.
    let sanitized: String = s
        .chars()
        .map(|c| {
            // `is_ascii_control()` returns true for 0x00–0x1F and 0x7F.
            // We want to allow LF (0x0A) and TAB (0x09), so exclude those.
            if c.is_ascii() && c != '\n' && c != '\t' && (c.is_ascii_control() || c == '\x7f') {
                '?'
            } else {
                c
            }
        })
        .collect();
    Cow::Owned(sanitized)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_string_is_borrowed() {
        let s = "hello world\nwith newline\tand tab";
        let result = sanitize_for_terminal(s);
        assert!(matches!(result, Cow::Borrowed(_)));
        assert_eq!(result, s);
    }

    #[test]
    fn escape_sequences_are_replaced() {
        // ESC (0x1B) is a common ANSI escape prefix.
        let s = "\x1b[31mred\x1b[0m";
        let result = sanitize_for_terminal(s);
        assert_eq!(result, "?[31mred?[0m");
    }

    #[test]
    fn null_byte_is_replaced() {
        let s = "before\x00after";
        assert_eq!(sanitize_for_terminal(s), "before?after");
    }

    #[test]
    fn cr_is_replaced() {
        // Carriage return can be used to overwrite terminal output.
        let s = "legit\rmalicious";
        assert_eq!(sanitize_for_terminal(s), "legit?malicious");
    }

    #[test]
    fn del_is_replaced() {
        let s = "a\x7fb";
        assert_eq!(sanitize_for_terminal(s), "a?b");
    }

    #[test]
    fn newline_and_tab_are_preserved() {
        let s = "line1\nline2\ttabbed";
        let result = sanitize_for_terminal(s);
        assert!(matches!(result, Cow::Borrowed(_)));
        assert_eq!(result, s);
    }

    #[test]
    fn unicode_is_preserved() {
        let s = "Krankenkasse – pricé: 42€";
        let result = sanitize_for_terminal(s);
        assert!(matches!(result, Cow::Borrowed(_)));
        assert_eq!(result, s);
    }

    #[test]
    fn empty_string_is_borrowed() {
        let s = "";
        let result = sanitize_for_terminal(s);
        assert!(matches!(result, Cow::Borrowed(_)));
    }
}
