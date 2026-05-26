//! CSS Selectors Level 4 specificity calculation.
//!
//! Returns the three-tuple `(a, b, c)` where:
//! - `a` — count of ID selectors
//! - `b` — count of class selectors, attribute selectors, and pseudo-classes
//! - `c` — count of type selectors and pseudo-elements
//!
//! The universal selector `*` and combinators (` `, `>`, `+`, `~`, `||`) do
//! not contribute.  Functional pseudo-classes have special rules:
//!
//! - `:not(X)`, `:is(X)`, `:has(X)` — specificity of the *most specific*
//!   complex selector in the argument list, contributing to the result.
//! - `:where(X)` — always zero.
//!
//! Reference: <https://www.w3.org/TR/selectors-4/#specificity-rules>.
//!
//! This module is intentionally small and pure: it parses selector strings
//! Firefox returns over RDP (already validated by the browser) and counts
//! the components.  It is **not** a full CSS selector parser; constructs
//! Firefox does not emit (escape sequences, CDATA, etc.) are not supported.

/// A specificity tuple in CSS Level 4 order: `(ids, classes_attrs_pseudoclasses, types_pseudoelements)`.
pub type Specificity = (u32, u32, u32);

/// Compute the CSS Selectors Level 4 specificity of a selector string.
///
/// Multiple complex selectors separated by `,` are NOT supported here —
/// pass each one individually.  Whitespace, descendant/child/sibling
/// combinators, the universal selector, and `:where(...)` contribute zero.
pub fn compute(selector: &str) -> Specificity {
    let bytes = selector.as_bytes();
    let mut i = 0;
    let mut a: u32 = 0;
    let mut b: u32 = 0;
    let mut c: u32 = 0;

    while i < bytes.len() {
        let ch = bytes[i];
        match ch {
            b' ' | b'\t' | b'\n' | b'\r' | b'>' | b'+' | b'~' | b',' => {
                i += 1;
            }
            b'*' => {
                // Universal selector — contributes nothing.
                i += 1;
            }
            b'#' => {
                a += 1;
                i += 1;
                i += skip_ident(&bytes[i..]);
            }
            b'.' => {
                b += 1;
                i += 1;
                i += skip_ident(&bytes[i..]);
            }
            b'[' => {
                b += 1;
                // Skip the whole attribute selector, accounting for quoted strings.
                i += skip_brackets(&bytes[i..]);
            }
            b':' => {
                // Single colon = pseudo-class (b), double colon = pseudo-element (c).
                let is_pseudo_element = bytes.get(i + 1) == Some(&b':');
                if is_pseudo_element {
                    i += 2;
                } else {
                    i += 1;
                }
                let name_len = skip_ident(&bytes[i..]);
                let name = std::str::from_utf8(&bytes[i..i + name_len]).unwrap_or("");
                i += name_len;

                // Count the pseudo itself.  Pseudo-element → c, pseudo-class → b
                // (the special-case functional pseudo-classes below may override or
                // augment this contribution).
                let has_args = bytes.get(i) == Some(&b'(');
                if is_pseudo_element {
                    c += 1;
                } else if !is_special_functional_pseudo(name) || !has_args {
                    b += 1;
                }

                if has_args {
                    let arg_len = skip_parens(&bytes[i..]);
                    let inside_start = i + 1;
                    let inside_end = i + arg_len - 1;
                    let inside = if inside_end > inside_start {
                        std::str::from_utf8(&bytes[inside_start..inside_end]).unwrap_or("")
                    } else {
                        ""
                    };
                    i += arg_len;
                    if !is_pseudo_element {
                        apply_functional_pseudo(name, inside, &mut a, &mut b, &mut c);
                    }
                }
            }
            _ if is_ident_start(ch) => {
                // Type selector — contributes to c.
                c += 1;
                i += skip_ident(&bytes[i..]);
            }
            _ => {
                // Unknown / unparseable character — advance to avoid infinite loops.
                i += 1;
            }
        }
    }

    (a, b, c)
}

/// True when `:name(...)` has the special "max specificity in argument list"
/// or "always zero" rule — i.e. `:is`, `:not`, `:has`, `:where`.  For these
/// names the pseudo-class itself does NOT contribute its own `b`; the call
/// site handles the count via [`apply_functional_pseudo`].
fn is_special_functional_pseudo(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "is" | "not" | "has" | "where"
    )
}

/// Update `(a, b, c)` for a functional pseudo-class `:name(args)`.
///
/// `:is(...)`, `:not(...)`, `:has(...)` take the specificity of the
/// most-specific selector in their argument list (and the pseudo itself
/// contributes nothing).  `:where(...)` is always zero.  Anything else
/// is already counted as a plain pseudo-class by the caller.
fn apply_functional_pseudo(name: &str, inside: &str, a: &mut u32, b: &mut u32, c: &mut u32) {
    if matches!(name.to_ascii_lowercase().as_str(), "is" | "not" | "has") {
        let max = max_specificity_in_list(inside);
        *a += max.0;
        *b += max.1;
        *c += max.2;
    }
    // `:where(...)` and any other name contribute nothing here;
    // the call site already handled the pseudo-class count.
}

/// Return the maximum specificity across a comma-separated selector list.
fn max_specificity_in_list(list: &str) -> Specificity {
    let mut best: Specificity = (0, 0, 0);
    for piece in split_top_level_commas(list) {
        let s = compute(piece.trim());
        if s > best {
            best = s;
        }
    }
    best
}

/// Split a string on top-level `,` (not inside parens or brackets).
fn split_top_level_commas(s: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut depth_paren = 0i32;
    let mut depth_bracket = 0i32;
    let bytes = s.as_bytes();
    let mut start = 0;
    for (i, &ch) in bytes.iter().enumerate() {
        match ch {
            b'(' => depth_paren += 1,
            b')' => depth_paren -= 1,
            b'[' => depth_bracket += 1,
            b']' => depth_bracket -= 1,
            b',' if depth_paren == 0 && depth_bracket == 0 => {
                out.push(&s[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    out.push(&s[start..]);
    out
}

fn is_ident_start(ch: u8) -> bool {
    ch.is_ascii_alphabetic() || ch == b'_' || ch == b'-'
}

fn is_ident_continue(ch: u8) -> bool {
    ch.is_ascii_alphanumeric() || ch == b'_' || ch == b'-'
}

/// Number of bytes consumed by an identifier at the start of `bytes`.
fn skip_ident(bytes: &[u8]) -> usize {
    let mut n = 0;
    while n < bytes.len() && is_ident_continue(bytes[n]) {
        n += 1;
    }
    n
}

/// Number of bytes consumed by an attribute selector `[...]` at the start.
///
/// Handles single- and double-quoted strings inside the brackets so that
/// `[data-x="]"]` is not split prematurely.
fn skip_brackets(bytes: &[u8]) -> usize {
    debug_assert_eq!(bytes.first(), Some(&b'['));
    let mut i = 1;
    while i < bytes.len() {
        match bytes[i] {
            b']' => return i + 1,
            b'"' | b'\'' => {
                let quote = bytes[i];
                i += 1;
                while i < bytes.len() && bytes[i] != quote {
                    if bytes[i] == b'\\' && i + 1 < bytes.len() {
                        i += 2;
                    } else {
                        i += 1;
                    }
                }
                if i < bytes.len() {
                    i += 1;
                }
            }
            _ => i += 1,
        }
    }
    bytes.len()
}

/// Number of bytes consumed by a parenthesised group `(...)` at the start.
fn skip_parens(bytes: &[u8]) -> usize {
    debug_assert_eq!(bytes.first(), Some(&b'('));
    let mut i = 1;
    let mut depth = 1;
    while i < bytes.len() && depth > 0 {
        match bytes[i] {
            b'(' => depth += 1,
            b')' => depth -= 1,
            b'"' | b'\'' => {
                let quote = bytes[i];
                i += 1;
                while i < bytes.len() && bytes[i] != quote {
                    if bytes[i] == b'\\' && i + 1 < bytes.len() {
                        i += 2;
                    } else {
                        i += 1;
                    }
                }
            }
            _ => {}
        }
        i += 1;
    }
    i
}

#[cfg(test)]
mod tests {
    use super::*;

    // ----- W3C Selectors 4 specificity examples ---------------------------------

    #[test]
    fn type_selector_is_001() {
        assert_eq!(compute("li"), (0, 0, 1));
    }

    #[test]
    fn descendant_of_types_is_002() {
        assert_eq!(compute("ul li"), (0, 0, 2));
    }

    #[test]
    fn multiple_types_and_combinator() {
        assert_eq!(compute("ul ol+li"), (0, 0, 3));
    }

    #[test]
    fn type_with_attribute_is_011() {
        assert_eq!(compute("h1 + *[rel=up]"), (0, 1, 1));
    }

    #[test]
    fn types_with_class_is_013() {
        assert_eq!(compute("ul ol li.red"), (0, 1, 3));
    }

    #[test]
    fn type_with_two_classes_and_pseudo_class() {
        assert_eq!(compute("li.red.level"), (0, 2, 1));
    }

    #[test]
    fn id_is_100() {
        assert_eq!(compute("#x34y"), (1, 0, 0));
    }

    #[test]
    fn id_and_class_and_type() {
        // dialog#lightbox  -> id=1, type=1
        assert_eq!(compute("dialog#lightbox"), (1, 0, 1));
    }

    // ----- Universal & combinators ----------------------------------------------

    #[test]
    fn universal_is_zero() {
        assert_eq!(compute("*"), (0, 0, 0));
    }

    #[test]
    fn combinators_contribute_nothing() {
        // > + ~ only join other simple selectors.
        let a = compute("a > b + c ~ d");
        assert_eq!(a, (0, 0, 4));
    }

    // ----- Pseudo-classes vs pseudo-elements ------------------------------------

    #[test]
    fn pseudo_class_contributes_to_b() {
        assert_eq!(compute("a:hover"), (0, 1, 1));
    }

    #[test]
    fn pseudo_element_contributes_to_c() {
        assert_eq!(compute("p::before"), (0, 0, 2));
    }

    #[test]
    fn double_colon_pseudo_element_with_paren_is_c() {
        // ::part(name) is a functional pseudo-element — c gets +1 for ::part,
        // plus inside-arg contributions (none here).
        assert_eq!(compute("p::part(tab)"), (0, 0, 2));
    }

    // ----- :is / :not / :has / :where -------------------------------------------

    #[test]
    fn is_takes_max_specificity() {
        // :is(#a, .b, c) — max is #a (1,0,0).
        assert_eq!(compute(":is(#a, .b, c)"), (1, 0, 0));
    }

    #[test]
    fn not_takes_max_specificity() {
        assert_eq!(compute(":not(.a, #b)"), (1, 0, 0));
    }

    #[test]
    fn where_is_zero() {
        assert_eq!(compute(":where(#a, .b)"), (0, 0, 0));
    }

    #[test]
    fn nth_child_is_pseudo_class() {
        assert_eq!(compute("li:nth-child(2n+1)"), (0, 1, 1));
    }

    // ----- Attribute selectors --------------------------------------------------

    #[test]
    fn attribute_selector_with_quoted_value() {
        assert_eq!(compute(r#"input[type="text"]"#), (0, 1, 1));
    }

    #[test]
    fn attribute_with_bracket_in_value_does_not_split() {
        // Bracketed string with a `]` inside the quotes must not terminate early.
        assert_eq!(compute(r#"a[href$="]"]"#), (0, 1, 1));
    }

    // ----- Empty / edge cases ---------------------------------------------------

    #[test]
    fn empty_string_is_zero() {
        assert_eq!(compute(""), (0, 0, 0));
    }

    #[test]
    fn whitespace_only_is_zero() {
        assert_eq!(compute("   \t  "), (0, 0, 0));
    }

    // ----- Ordering / comparison ------------------------------------------------

    #[test]
    fn specificity_ordering_uses_tuple_compare() {
        // (1,0,0) > (0,99,99) — IDs beat any number of classes.
        let a = compute("#foo");
        let b = compute(".a.b.c.d.e.f.g.h.i.j");
        assert!(a > b);
    }
}
