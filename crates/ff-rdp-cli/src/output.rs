use anyhow::Context as _;
use jaq_core::load::{Arena, File, Loader};
use jaq_core::{Compiler, Ctx, Native, Vars, data};
use jaq_json::Val;
use serde_json::Value;

use crate::hints::Hint;

/// Type alias matching jaq-core 3.x idiom: a filter with no imported
/// modules beyond the built-in function lookup table.
type D = data::JustLut<Val>;

/// Build the standard JSON output envelope.
///
/// When `meta` is an empty object, the `"meta"` key is omitted from the
/// envelope to keep default responses compact.
pub fn envelope(results: &Value, total: usize, meta: &Value) -> Value {
    envelope_with_truncation(results, total, total, false, meta)
}

/// Build the standard JSON output envelope with optional truncation info.
///
/// When `truncated` is true, a `"truncated": true` field and a human-readable
/// `"hint"` field are included so callers know results were capped.
///
/// When `meta` is an empty object (`{}`), the `"meta"` key is omitted to keep
/// the default compact output shape free of boilerplate.
pub fn envelope_with_truncation(
    results: &Value,
    shown: usize,
    total: usize,
    truncated: bool,
    meta: &Value,
) -> Value {
    let meta_empty = crate::connection_meta::is_meta_empty(meta);
    let mut env = if meta_empty {
        serde_json::json!({
            "results": results,
            "total": total,
        })
    } else {
        serde_json::json!({
            "results": results,
            "total": total,
            "meta": meta,
        })
    };
    if truncated && let Some(obj) = env.as_object_mut() {
        obj.insert("truncated".to_string(), Value::Bool(true));
        obj.insert(
            "hint".to_string(),
            Value::String(format!(
                "showing {shown} of {total}, use --all for complete list"
            )),
        );
    }
    env
}

/// Middle-ellipsize `s` to fit within `max_width` *characters* (not bytes —
/// multibyte-safe; splits on `char` boundaries so it never panics on UTF-8
/// input) for `--format text` table cells (iter-128 Theme C).
///
/// Strings already at or under `max_width` characters are returned
/// unchanged — a no-op below the cap, so short URLs and other short values
/// are never touched.
///
/// When `s` looks like a URL (`scheme://host…`), the `scheme://host` prefix
/// is preserved intact whenever it fits the budget, and the remaining budget
/// is spent on the tail of the path/query — so a truncated cell still shows
/// *where* the request went and how it ended, e.g.
/// `https://ads.example.com/…&clickid=9f8e7d6c5b4a`. Non-URL strings (no
/// `://`) fall back to an even head/tail split around the ellipsis.
pub fn middle_ellipsis(s: &str, max_width: usize) -> String {
    let char_count = s.chars().count();
    if char_count <= max_width {
        return s.to_owned();
    }
    if max_width == 0 {
        return String::new();
    }
    // Reserve 1 character for the '…' marker itself.
    let budget = max_width - 1;

    // Try to find a `scheme://host` prefix (up to the first '/' after the
    // "://") that fits within the budget, leaving at least 1 char for the
    // tail.
    let url_prefix_end = s.find("://").and_then(|scheme_end| {
        let after_scheme = scheme_end + 3;
        let host_end = s[after_scheme..]
            .find('/')
            .map_or(s.len(), |i| after_scheme + i);
        let prefix_chars = s[..host_end].chars().count();
        (prefix_chars > 0 && prefix_chars < budget).then_some(prefix_chars)
    });

    let (head_chars, tail_chars) = match url_prefix_end {
        Some(prefix_chars) => (prefix_chars, budget - prefix_chars),
        None => (budget / 2, budget - budget / 2),
    };

    let head: String = s.chars().take(head_chars).collect();
    let tail: String = {
        let mut rev: Vec<char> = s.chars().rev().take(tail_chars).collect();
        rev.reverse();
        rev.into_iter().collect()
    };
    format!("{head}…{tail}")
}

/// Inject contextual hints into a pre-built envelope.
///
/// Adds `"hints": [...]` as a top-level key. Returns an error if any hint
/// fails to serialize.
pub fn inject_hints(envelope: &mut Value, hints: &[Hint]) -> anyhow::Result<()> {
    if let Some(obj) = envelope.as_object_mut() {
        let hints_json: Vec<Value> = hints
            .iter()
            .map(|h| serde_json::to_value(h).context("failed to serialize hint"))
            .collect::<anyhow::Result<_>>()?;
        obj.insert("hints".to_string(), Value::Array(hints_json));
    }
    Ok(())
}

/// Compile and execute a jq filter on a JSON value.
///
/// Returns the filtered results as a `Vec<Value>`. Each output item from the
/// filter becomes one element. If the filter produces no outputs the vec is
/// empty. Parse and runtime errors are surfaced as `anyhow::Error`.
pub fn apply_jq_filter(input: &Value, filter: &str) -> anyhow::Result<Vec<Value>> {
    let compiled = compile_jq_filter(filter)?;
    execute_jq_filter(&compiled, input)
}

/// Compile a jq filter string into an owned, reusable `Filter`.
///
/// The `Arena` used by the `Loader` is a temporary scratch pad that is dropped
/// at the end of this function — the returned `Filter` owns all its data.
fn compile_jq_filter(filter_code: &str) -> anyhow::Result<jaq_core::compile::Filter<Native<D>>> {
    let program = File {
        code: filter_code,
        path: (),
    };

    let defs = jaq_core::defs()
        .chain(jaq_std::defs())
        .chain(jaq_json::defs());
    let loader = Loader::new(defs);
    let arena = Arena::default();

    let modules = loader.load(&arena, program).map_err(|errs| {
        let msg = errs
            .iter()
            .map(|(_file, e)| format_load_error(filter_code, e))
            .collect::<Vec<_>>()
            .join("; ");
        anyhow::anyhow!("jq parse error: {msg}")
    })?;

    let funs = jaq_core::funs::<D>()
        .chain(jaq_std::funs::<D>())
        .chain(jaq_json::funs::<D>());

    Compiler::default()
        .with_funs(funs)
        .compile(modules)
        .map_err(|errs| {
            let msg = errs
                .iter()
                .map(|(_file, e)| format_compile_errors(e))
                .collect::<Vec<_>>()
                .join("; ");
            anyhow::anyhow!("jq compile error: {msg}")
        })
}

/// Byte offset of a subslice within its parent string.
///
/// jaq's lexer and parser errors carry `&str` slices that are always
/// subslices of the original filter source (a suffix for lexer errors, an
/// interior token for parser errors) — never a copy. Comparing pointer
/// addresses (as integers, so no unsafe dereference is involved) recovers
/// the byte position without re-scanning the string. Falls back to `0` if
/// `needle` is somehow not a subslice of `haystack` (defensive only — this
/// should never happen given how jaq constructs these errors, but a wrong
/// position is far better than a panicking CLI on malformed `--jq` input).
fn subslice_offset(haystack: &str, needle: &str) -> usize {
    let h = haystack.as_ptr() as usize;
    let n = needle.as_ptr() as usize;
    if n < h || n > h + haystack.len() {
        return 0;
    }
    n - h
}

/// Short " near \"...\"" context suffix for an error message, anchored at
/// byte offset `pos` in `code`. Empty string once `pos` reaches the end of
/// input (nothing left to show).
fn error_snippet(code: &str, pos: usize) -> String {
    if pos >= code.len() {
        return String::new();
    }
    let snippet: String = code[pos..].chars().take(20).collect();
    format!(" near \"{snippet}\"")
}

/// Human-readable description of a lexer `Expect` value.
///
/// Deliberately does not call the upstream `Expect::as_str()` — that method
/// panics on an unrecognized `Delim` payload, and this is a user-facing
/// error path for arbitrary (possibly malformed) `--jq` input, where a
/// slightly-generic description beats a crashed CLI.
fn describe_lex_expect(expect: &jaq_core::load::lex::Expect<&str>) -> &'static str {
    use jaq_core::load::lex::Expect;
    match expect {
        Expect::Digit => "digit",
        Expect::Ident => "identifier",
        Expect::Delim("(") => "closing parenthesis",
        Expect::Delim("[") => "closing bracket",
        Expect::Delim("{") => "closing brace",
        Expect::Delim("\"") => "closing quote",
        Expect::Delim(_) => "closing delimiter",
        Expect::Escape => "string escape sequence",
        Expect::Unicode => "4-digit hexadecimal UTF-8 code point",
        Expect::Token => "token",
        _ => "valid token",
    }
}

/// Render one lexer error as `at position N[ (end of input)]: expected
/// <what>[ near "<snippet>"]` — no `Expect`/`Delim` Debug fragments.
fn format_lex_error(code: &str, err: &jaq_core::load::lex::Error<&str>) -> String {
    let (expect, rest) = err;
    let pos = subslice_offset(code, rest);
    let expected = describe_lex_expect(expect);
    let snippet = error_snippet(code, pos);
    if rest.is_empty() {
        format!("at position {pos} (end of input): expected {expected}")
    } else {
        format!("at position {pos}: expected {expected}{snippet}")
    }
}

/// Render one parser error as `at position N: expected <what>[, found
/// "<token>"]` — no `Expect`/`Token` Debug fragments.
///
/// The `found` slice has already been resolved from `Option<&Token>` down
/// to a plain `&str` by jaq's own loader (empty slice at end-of-input when
/// no token was found) — see `load::mod::conv_err`.
fn format_parse_error(code: &str, err: &jaq_core::load::parse::Error<&str>) -> String {
    let (expect, found_str) = err;
    let pos = subslice_offset(code, found_str);
    let expected = expect.as_str();
    let snippet = error_snippet(code, pos);
    if found_str.is_empty() {
        format!("at position {pos} (end of input): expected {expected}")
    } else {
        format!("at position {pos}: expected {expected}, found \"{found_str}\"{snippet}")
    }
}

/// Render a single-module load error (`Io`/`Lex`/`Parse`) without leaking
/// any Rust `Debug` formatting of jaq's internal `Expect`/`Token` types.
fn format_load_error(code: &str, err: &jaq_core::load::Error<&str>) -> String {
    use jaq_core::load::Error;
    match err {
        Error::Io(errs) => errs
            .iter()
            .map(|(path, msg)| format!("module '{path}': {msg}"))
            .collect::<Vec<_>>()
            .join("; "),
        Error::Lex(errs) => errs
            .iter()
            .map(|e| format_lex_error(code, e))
            .collect::<Vec<_>>()
            .join("; "),
        Error::Parse(errs) => errs
            .iter()
            .map(|e| format_parse_error(code, e))
            .collect::<Vec<_>>()
            .join("; "),
    }
}

/// Render compilation errors (undefined variables/filters/modules/labels)
/// for one module without leaking `Undefined` Debug formatting.
fn format_compile_errors(errs: &[jaq_core::compile::Error<&str>]) -> String {
    errs.iter()
        .map(|(name, kind)| format!("undefined {}: '{name}'", kind.as_str()))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Execute a pre-compiled jq filter against a JSON value.
fn execute_jq_filter(
    filter: &jaq_core::compile::Filter<Native<D>>,
    value: &Value,
) -> anyhow::Result<Vec<Value>> {
    let input: Val = serde_json::from_value(value.clone())
        .map_err(|e| anyhow::anyhow!("jq input conversion error: {e}"))?;

    let ctx = Ctx::<D>::new(&filter.lut, Vars::new([]));

    let mut results = Vec::new();
    for result in filter.id.run((ctx, input)).map(jaq_core::unwrap_valr) {
        let val = result.map_err(|e| anyhow::anyhow!("jq runtime error: {e}"))?;
        let json = val_to_value(&val)?;
        results.push(json);
    }

    Ok(results)
}

/// Convert a jaq `Val` to a `serde_json::Value`.
///
/// `Val` does not implement `Serialize`, but it does implement `Display`
/// which outputs JSON. We format to string and re-parse.
fn val_to_value(val: &Val) -> anyhow::Result<Value> {
    let json_str = val.to_string();
    serde_json::from_str(&json_str)
        .map_err(|e| anyhow::anyhow!("jq output is not valid JSON: {e}: {json_str}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn envelope_structure() {
        let results = json!(["a", "b"]);
        let meta = json!({"tab": "test"});
        let env = envelope(&results, 2, &meta);
        assert_eq!(env["total"], 2);
        assert_eq!(env["results"], results);
        assert_eq!(env["meta"], meta);
        assert!(env.get("truncated").is_none());
    }

    #[test]
    fn envelope_omits_meta_when_empty() {
        // iter-60 Part A: empty meta is omitted to keep responses compact.
        let results = json!(["a", "b"]);
        let meta = json!({});
        let env = envelope(&results, 2, &meta);
        assert!(
            env.get("meta").is_none(),
            "meta must be omitted when empty; got: {env}"
        );
    }

    #[test]
    fn envelope_includes_meta_when_non_empty() {
        let results = json!(["a", "b"]);
        let meta = json!({"selector": "h1"});
        let env = envelope(&results, 2, &meta);
        assert!(env["meta"].is_object(), "non-empty meta must be included");
    }

    #[test]
    fn envelope_with_truncation_not_truncated() {
        let results = json!(["a", "b"]);
        let meta = json!({});
        let env = envelope_with_truncation(&results, 2, 2, false, &meta);
        assert_eq!(env["total"], 2);
        assert!(env.get("truncated").is_none());
        assert!(env.get("hint").is_none());
    }

    #[test]
    fn envelope_with_truncation_truncated() {
        let results = json!(["a"]);
        let meta = json!({});
        let env = envelope_with_truncation(&results, 1, 5, true, &meta);
        assert_eq!(env["total"], 5);
        assert_eq!(env["truncated"], true);
        let hint = env["hint"].as_str().expect("hint should be a string");
        assert!(hint.contains("1 of 5"));
        assert!(hint.contains("--all"));
    }

    // -----------------------------------------------------------------------
    // iter-128 Theme C: middle_ellipsis
    // -----------------------------------------------------------------------

    /// AC: `unit_middle_ellipsis` — no-op below the width cap.
    #[test]
    fn middle_ellipsis_short_string_untouched() {
        let s = "https://example.com/";
        assert_eq!(middle_ellipsis(s, 80), s);
    }

    #[test]
    fn middle_ellipsis_exactly_at_cap_untouched() {
        let s = "a".repeat(80);
        assert_eq!(middle_ellipsis(&s, 80), s);
    }

    /// AC: `unit_middle_ellipsis` — preserves the `scheme://host` prefix and
    /// a tail of the path around the ellipsis for long URLs.
    #[test]
    fn middle_ellipsis_preserves_url_prefix_and_tail() {
        let long_url = format!(
            "https://ads.sourcepoint.example.com/{}?clickid=deadbeef1234",
            "x".repeat(200)
        );
        let out = middle_ellipsis(&long_url, 80);
        assert!(out.chars().count() <= 80, "result exceeds cap: {out:?}");
        assert!(
            out.starts_with("https://ads.sourcepoint.example.com"),
            "scheme+host prefix must be preserved: {out:?}"
        );
        assert!(
            out.ends_with("clickid=deadbeef1234"),
            "path tail must be preserved: {out:?}"
        );
        assert!(
            out.contains('…'),
            "must contain the ellipsis marker: {out:?}"
        );
    }

    /// Non-URL strings (no `://`) fall back to an even head/tail split.
    #[test]
    fn middle_ellipsis_non_url_even_split() {
        let long = "a".repeat(50) + &"b".repeat(50);
        let out = middle_ellipsis(&long, 21);
        assert!(out.chars().count() <= 21, "result exceeds cap: {out:?}");
        assert!(out.starts_with('a'), "head must be preserved: {out:?}");
        assert!(out.ends_with('b'), "tail must be preserved: {out:?}");
        assert!(out.contains('…'));
    }

    /// AC: `unit_middle_ellipsis` — multibyte safety: truncating on `char`
    /// boundaries must never panic or produce invalid UTF-8, even when the
    /// cut point would otherwise land mid-codepoint under a byte-oriented
    /// truncation.
    #[test]
    fn middle_ellipsis_multibyte_safe() {
        // Multi-byte (3-byte UTF-8) characters throughout — a byte-index
        // slice at an arbitrary offset would panic; a char-based one must not.
        let s = "€".repeat(100);
        let out = middle_ellipsis(&s, 21);
        // No panic reaching here is the primary assertion; also sanity-check
        // the char-count budget and that it's still valid UTF-8 (guaranteed
        // by `String`, but the char() collection round-trip is the real proof).
        assert!(out.chars().count() <= 21, "result exceeds cap: {out:?}");
        assert!(out.contains('…'));
    }

    #[test]
    fn middle_ellipsis_zero_width() {
        assert_eq!(middle_ellipsis("hello", 0), "");
    }

    #[test]
    fn jq_identity_filter() {
        let val = json!({"name": "test"});
        let results = apply_jq_filter(&val, ".").unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0], val);
    }

    #[test]
    fn jq_field_access() {
        let val = json!({"name": "hello", "count": 3});
        let results = apply_jq_filter(&val, ".name").unwrap();
        assert_eq!(results, vec![json!("hello")]);
    }

    #[test]
    fn jq_array_iteration() {
        let val = json!([1, 2, 3]);
        let results = apply_jq_filter(&val, ".[]").unwrap();
        assert_eq!(results, vec![json!(1), json!(2), json!(3)]);
    }

    #[test]
    fn jq_invalid_filter() {
        let val = json!({"x": 1});
        let result = apply_jq_filter(&val, "this is not valid %%%");
        assert!(result.is_err());
    }

    // ── unit_jq_parse_error_clean (iter-132 Theme A) ─────────────────────────

    /// An unclosed `[` must yield a human-readable error carrying the
    /// failing position, and must NOT contain any Rust `Debug` fragments
    /// (`Lex(`, `Delim(`, `Expect::`) from jaq's internal error types.
    #[test]
    fn jq_parse_error_unclosed_bracket_is_clean() {
        let val = json!({"x": 1});
        let err = apply_jq_filter(&val, "[").unwrap_err();
        let msg = err.to_string();

        assert!(
            !msg.contains("Lex("),
            "must not leak jaq's Debug-formatted Lex(...) variant, got: {msg}"
        );
        assert!(
            !msg.contains("Delim("),
            "must not leak jaq's Debug-formatted Delim(...) payload, got: {msg}"
        );
        assert!(
            !msg.contains("Expect::") && !msg.contains("Expect {"),
            "must not leak the Expect enum's Debug repr, got: {msg}"
        );
        assert!(
            msg.contains("position"),
            "must name the failing position, got: {msg}"
        );
    }

    /// A malformed filter with garbage tokens must also render cleanly and
    /// report a position, not just an empty/opaque message.
    #[test]
    fn jq_parse_error_garbage_tokens_reports_position() {
        let val = json!({"x": 1});
        let err = apply_jq_filter(&val, "this is not valid %%%").unwrap_err();
        let msg = err.to_string();
        assert!(!msg.contains("Lex("), "got: {msg}");
        assert!(!msg.contains("Delim("), "got: {msg}");
        assert!(msg.contains("position"), "got: {msg}");
    }

    /// An undefined filter/function name (a *compile* error, not a lex/parse
    /// error) must also avoid leaking `Undefined` Debug formatting.
    #[test]
    fn jq_compile_error_undefined_filter_is_clean() {
        let val = json!({"x": 1});
        let err = apply_jq_filter(&val, "this_filter_does_not_exist").unwrap_err();
        let msg = err.to_string();
        assert!(
            !msg.contains("Undefined") && !msg.contains("Filter("),
            "must not leak the Undefined enum's Debug repr, got: {msg}"
        );
        assert!(
            msg.contains("undefined"),
            "must name what was undefined, got: {msg}"
        );
    }

    #[test]
    fn subslice_offset_finds_position() {
        let code = "hello world";
        let sub = &code[6..];
        assert_eq!(subslice_offset(code, sub), 6);
        assert_eq!(subslice_offset(code, code), 0);
        // A completely unrelated string is not a subslice — must not panic,
        // falls back to 0.
        let unrelated = String::from("unrelated");
        assert_eq!(subslice_offset(code, &unrelated), 0);
    }
}
