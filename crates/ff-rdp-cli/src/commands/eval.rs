use std::io::Read;

use anyhow::Context as _;
use ff_rdp_core::{Grip, ObjectActor, ProtocolError, WebConsoleActor};
use serde_json::json;

use crate::cli::args::Cli;
use crate::error::AppError;
use crate::hints::{HintContext, HintSource};
use crate::output;
use crate::output_pipeline::OutputPipeline;

use super::connect_tab::connect_and_get_target;

/// Load the JavaScript source from exactly one of the three input modes.
///
/// The clap `ArgGroup` constraint guarantees that exactly one of `script`,
/// `file`, or `stdin` is non-empty; this helper defensively errors if that
/// invariant is ever violated.
pub(crate) fn load_script(
    script: Option<&str>,
    file: Option<&str>,
    use_stdin: bool,
) -> Result<String, AppError> {
    let sources =
        usize::from(script.is_some()) + usize::from(file.is_some()) + usize::from(use_stdin);
    if sources == 0 {
        return Err(AppError::User(
            "eval requires a script (positional), --file <PATH>, or --stdin".to_owned(),
        ));
    }
    if sources > 1 {
        return Err(AppError::User(
            "eval accepts only one of: positional <SCRIPT>, --file, --stdin".to_owned(),
        ));
    }

    if let Some(s) = script {
        return Ok(s.to_owned());
    }
    if let Some(path) = file {
        return std::fs::read_to_string(path).map_err(|e| {
            AppError::User(format!("eval: could not read script file '{path}': {e}"))
        });
    }
    // stdin branch.
    let mut buf = String::new();
    std::io::stdin()
        .read_to_string(&mut buf)
        .context("eval: failed to read script from stdin")
        .map_err(AppError::from)?;
    Ok(buf)
}

/// Build the final JS source from the user's script plus the `--stringify` and
/// `--no-isolate` flags.
///
/// # Isolation (default)
///
/// Firefox's console actor shares its global scope across evaluations, so two
/// consecutive `eval 'const x = 1; x'` calls fail with "redeclaration of const
/// x". We wrap the user code in `(function() { "use strict"; return eval(<src>);
/// })()` — a strict-mode IIFE. Direct `eval` inside a strict function uses its
/// own variable environment for `const`/`let`/`var` declarations, so they don't
/// leak across calls. `eval` returns the completion value of its last
/// statement, so single expressions like `1 + 1` still return `2`.
///
/// # Stringify
///
/// `--stringify` wraps the expression in `JSON.stringify(...)` so the user gets
/// real values instead of Firefox grip metadata. When combined with isolation,
/// we evaluate the user code first (so declarations stay scoped), then pass the
/// returned value through `JSON.stringify`.
pub(crate) fn build_script(user_script: &str, stringify: bool, isolate: bool) -> String {
    // The stringify helper: if the value is already a string, return it as-is;
    // otherwise JSON.stringify it. This prevents double-encoding when the JS
    // expression already evaluates to a string (e.g. `document.title`).
    // Circular references throw a TypeError from JSON.stringify; we catch
    // that specific case and return a marker JSON object so the eval still
    // succeeds. All other thrown values (including BigInt's TypeError and
    // Symbol's TypeError) propagate up as eval exceptions.
    const STRINGIFY_HELPER: &str = "(function(v){if(typeof v===\"string\")return v;try{return JSON.stringify(v);}catch(e){if(e instanceof TypeError&&e.message.includes(\"circular\"))return \"{\\\"error\\\":\\\"circular reference detected\\\"}\";throw e;}})";

    // JSON-encode the user source so it survives as a JS string literal.
    let encoded = serde_json::to_string(user_script).unwrap_or_else(|e| {
        // serde_json::to_string is infallible for &str — defensive fallback.
        unreachable!("serde_json::to_string(&str) is infallible: {e}")
    });

    match (isolate, stringify) {
        (false, false) => user_script.to_owned(),
        (false, true) => format!("(function(){{return {STRINGIFY_HELPER}({user_script});}})()"),
        (true, false) => format!("(function() {{ \"use strict\"; return eval({encoded}); }})()"),
        (true, true) => {
            format!("(function(){{\"use strict\";return {STRINGIFY_HELPER}(eval({encoded}));}})()")
        }
    }
}

/// Build the final JavaScript source, exposed for use by the script runner.
pub fn build_eval_js(
    script: Option<&str>,
    file: Option<&str>,
    use_stdin: bool,
    stringify: bool,
    no_isolate: bool,
) -> Result<String, AppError> {
    let user_script = load_script(script, file, use_stdin)?;
    let isolate = !no_isolate;
    Ok(build_script(&user_script, stringify, isolate))
}

/// Returns `true` when an exception message indicates a CSP eval block.
///
/// Firefox surfaces CSP eval rejections as a JS exception with a message
/// containing `"EvalError"` and either `"Content Security Policy"` or
/// `"blocked by CSP"`.  We check for both patterns to handle any wording
/// variation across Firefox versions.
fn is_csp_eval_error(exc_message: &str) -> bool {
    let m = exc_message;
    // "EvalError: call to eval() blocked by CSP"
    // "EvalError: Content Security Policy of your site blocks the use of 'eval'"
    m.contains("Content Security Policy") || m.contains("blocked by CSP") || m.contains("EvalError")
}

/// Evaluate `script` via the console actor, automatically retrying with the
/// chrome context when the first attempt is blocked by page CSP.
///
/// Returns `(result, used_chrome)` where `used_chrome` is `true` when the
/// fallback path was taken.
///
/// # CSP bypass mechanism
///
/// Firefox's `evaluateJSAsync` accepts a `chromeContext: true` flag that runs
/// the script in a privileged browser context, which is not subject to page
/// Content Security Policy.  We first try the normal (content) path.  If we
/// get an exception whose message matches a known CSP eval-block pattern we
/// immediately retry with `chromeContext: true`.
///
/// The chrome context still evaluates against the page's DOM — `document`,
/// `window`, etc. are the page's globals — so `document.title`, DOM queries,
/// and similar expressions work unchanged.  Only `eval()` is unblocked.
fn eval_with_csp_fallback(
    transport: &mut ff_rdp_core::RdpTransport,
    console_actor: &ff_rdp_core::ActorId,
    script: &str,
) -> Result<(ff_rdp_core::EvalResult, bool), AppError> {
    let first = WebConsoleActor::evaluate_js_async(transport, console_actor, script)
        .map_err(AppError::from)?;

    // Check whether the exception looks like a CSP eval block.
    let is_csp = first
        .exception
        .as_ref()
        .and_then(|e| e.message.as_deref())
        .is_some_and(is_csp_eval_error);

    if !is_csp {
        return Ok((first, false));
    }

    // Retry with chrome context.  If the chrome-context eval itself fails with
    // a protocol error (e.g. the actor does not support chromeContext), fall
    // back to surfacing the original CSP exception rather than a confusing
    // internal error.
    match WebConsoleActor::evaluate_js_async_chrome(transport, console_actor, script) {
        Ok(result) => Ok((result, true)),
        Err(ProtocolError::ActorError { .. }) => {
            // Chrome context not available (e.g. content-only build).  Return
            // the original CSP error so the user gets the real message.
            Ok((first, false))
        }
        Err(e) => Err(AppError::from(e)),
    }
}

pub fn run(
    cli: &Cli,
    script: Option<&str>,
    file: Option<&str>,
    use_stdin: bool,
    stringify: bool,
    no_isolate: bool,
) -> Result<(), AppError> {
    let script = load_script(script, file, use_stdin)?;
    let isolate = !no_isolate;
    let final_script = build_script(&script, stringify, isolate);

    let mut ctx = connect_and_get_target(cli)?;
    let console_actor = ctx.target.console_actor.clone();

    let (eval_result, used_chrome_context) =
        eval_with_csp_fallback(ctx.transport_mut(), &console_actor, &final_script)?;

    // If an exception occurred, print it to stderr and exit non-zero.
    if let Some(ref exc) = eval_result.exception {
        let msg = exc
            .message
            .as_deref()
            .unwrap_or("evaluation threw an exception");
        let detail = exc.value.to_json();
        eprintln!("error: {msg}");
        eprintln!(
            "{}",
            serde_json::to_string_pretty(&detail).unwrap_or_default()
        );
        return Err(AppError::Exit(1));
    }

    let mut result_json = eval_result.result.to_json();

    // For object grips, enrich the output with the list of own property names.
    // Best-effort: if the actor is gone or the request fails, we skip silently.
    //
    // Firefox 149 removed the `ownPropertyNames` packet type, so we use
    // `prototypeAndProperties` and extract the keys from the result.
    if let Grip::Object { ref actor, .. } = eval_result.result {
        match ObjectActor::prototype_and_properties(ctx.transport_mut(), actor.as_ref()) {
            Ok(pap) => {
                let names: Vec<&str> = pap.own_properties.keys().map(String::as_str).collect();
                result_json["propertyNames"] = json!(names);
            }
            Err(e) => {
                eprintln!("warning: could not fetch property names: {e}");
            }
        }
    }

    // When --stringify was used, the JS already ran JSON.stringify() so the
    // eval result is a JSON string (e.g. `"{\"a\":1}"`).  Parse it on the
    // ff-rdp side so `results` holds a real JSON object/array rather than a
    // string — agents can then use `--jq '.results.a'` directly without an
    // extra parse step.
    //
    // If parsing fails (e.g. the expression itself returned a plain string, or
    // the caller double-wrapped via another JSON.stringify), keep the raw
    // string value and set `meta.stringify_parsed: false` so callers know the
    // round-trip did not produce a structured value.
    let mut meta = json!({});
    if used_chrome_context {
        // Surface a one-liner so callers know the chrome-context CSP bypass was used.
        if let Some(m) = meta.as_object_mut() {
            m.insert(
                "note".to_owned(),
                json!("eval ran in chrome context (page CSP blocks eval; bypassed automatically)"),
            );
        }
    }
    if stringify && let serde_json::Value::String(ref s) = result_json {
        match serde_json::from_str::<serde_json::Value>(s) {
            Ok(parsed) => {
                result_json = parsed;
                // stringify_parsed defaults to true — omit the flag when parsing
                // succeeds so the output stays minimal.
            }
            Err(_) => {
                // Keep the raw string but signal that parsing did not succeed.
                if let Some(m) = meta.as_object_mut() {
                    m.insert("stringify_parsed".to_owned(), json!(false));
                }
            }
        }
    }
    crate::connection_meta::merge_into_if_verbose(
        &mut meta,
        &cli.host,
        cli.port,
        None,
        cli.is_verbose(),
    );
    let envelope = output::envelope(&result_json, 1, &meta);

    let hint_ctx = HintContext::new(HintSource::Eval);
    let pipeline = OutputPipeline::from_cli(cli)?;
    // When the caller passes `--stringify`, they're extracting a raw value;
    // appending the trailing "-> ff-rdp …" hint line would pollute their
    // captured stdout (dogfood-49 #6).  Suppress hints unconditionally in
    // that mode — symmetric with `--jq` and `--no-hints`.
    let pipeline = if stringify {
        pipeline.without_hints()
    } else {
        pipeline
    };
    pipeline
        .finalize_with_hints(&envelope, Some(&hint_ctx))
        .map_err(AppError::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_script_positional_passthrough() {
        let s = load_script(Some("document.title"), None, false).unwrap();
        assert_eq!(s, "document.title");
    }

    #[test]
    fn load_script_from_file() {
        let tmp = std::env::temp_dir().join(format!(
            "ff_rdp_eval_{}.js",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::write(&tmp, "1 + 2").unwrap();
        let s = load_script(None, Some(tmp.to_str().unwrap()), false).unwrap();
        assert_eq!(s, "1 + 2");
        let _ = std::fs::remove_file(&tmp);
    }

    #[test]
    fn load_script_missing_file_is_user_error() {
        let err = load_script(None, Some("/nonexistent/path/xyz.js"), false).unwrap_err();
        // Any AppError variant is fine as long as the message is helpful.
        let msg = format!("{err:?}");
        assert!(
            msg.contains("could not read script file") || msg.contains("xyz.js"),
            "unexpected error: {msg}"
        );
    }

    #[test]
    fn load_script_no_source_errors() {
        let err = load_script(None, None, false).unwrap_err();
        assert!(matches!(err, AppError::User(_)));
    }

    // ---------------------------------------------------------------------------
    // build_script wrapping tests
    // ---------------------------------------------------------------------------

    #[test]
    fn build_script_no_isolate_no_stringify_passthrough() {
        let s = build_script("document.title", false, false);
        assert_eq!(s, "document.title");
    }

    #[test]
    fn build_script_stringify_only_wraps_in_json_stringify() {
        let s = build_script("document.querySelectorAll('a')", true, false);
        // The stringify helper uses JSON.stringify for non-strings.
        assert!(s.contains("JSON.stringify("));
        assert!(s.contains("document.querySelectorAll('a')"));
        assert!(s.contains("circular"));
        // The stringify helper is inlined as a function — no bare eval().
        assert!(!s.contains("eval("));
        // Strings are passed through without double-encoding.
        assert!(s.contains("typeof v===\"string\""));
    }

    #[test]
    fn build_script_isolate_only_wraps_in_strict_eval_iife() {
        let s = build_script("const x = 1; x", false, true);
        assert!(s.starts_with("(function()"));
        assert!(s.contains("\"use strict\""));
        assert!(s.contains("return eval("));
        // The encoded source should appear as a JSON-encoded string literal.
        assert!(s.contains(r#""const x = 1; x""#));
    }

    #[test]
    fn build_script_isolate_preserves_single_expression() {
        // Single expression must still be returnable through eval().
        let s = build_script("1 + 1", false, true);
        assert!(s.contains("return eval("));
        assert!(s.contains(r#""1 + 1""#));
    }

    #[test]
    fn build_script_isolate_and_stringify_combine() {
        let s = build_script("document.querySelectorAll('a')", true, true);
        assert!(s.contains("\"use strict\""));
        // The stringify helper wraps eval(); JSON.stringify is inside the helper.
        assert!(s.contains("JSON.stringify("));
        assert!(s.contains("eval("));
        assert!(s.contains("circular"));
        // Strings are not double-encoded.
        assert!(s.contains("typeof v===\"string\""));
    }

    #[test]
    fn build_script_stringify_string_passthrough() {
        // When the expression evaluates to a string, the helper must return it
        // without passing through JSON.stringify (no double-encoding).
        let s = build_script("document.title", true, false);
        assert!(s.contains("typeof v===\"string\""));
        // The helper is invoked with the user expression as argument.
        assert!(s.contains("document.title"));
    }

    #[test]
    fn build_script_stringify_number_uses_json_stringify() {
        // For non-string values the helper falls through to JSON.stringify.
        let s = build_script("42", true, false);
        assert!(s.contains("JSON.stringify("));
        assert!(s.contains("42"));
    }

    #[test]
    fn build_script_handles_special_chars() {
        // Quotes, backslashes, newlines must be JSON-encoded safely.
        let s = build_script("'a' + \"b\" + `c\nd`", false, true);
        // The encoded string must not break the surrounding template.
        assert!(s.starts_with("(function()"));
        assert!(s.ends_with(")()"));
        // Encoded string should contain the escaped newline.
        assert!(s.contains(r"\n"));
    }

    // -----------------------------------------------------------------------
    // Theme H: CSP error detection
    // -----------------------------------------------------------------------

    #[test]
    fn is_csp_eval_error_detects_blocked_by_csp() {
        assert!(is_csp_eval_error(
            "EvalError: call to eval() blocked by CSP"
        ));
    }

    #[test]
    fn is_csp_eval_error_detects_content_security_policy() {
        assert!(is_csp_eval_error(
            "Content Security Policy of your site blocks the use of 'eval'"
        ));
    }

    #[test]
    fn is_csp_eval_error_detects_evalerror_prefix() {
        assert!(is_csp_eval_error("EvalError: some message"));
    }

    #[test]
    fn is_csp_eval_error_does_not_match_unrelated_errors() {
        assert!(!is_csp_eval_error("ReferenceError: foo is not defined"));
        assert!(!is_csp_eval_error("TypeError: cannot read property"));
        assert!(!is_csp_eval_error("SyntaxError: unexpected token"));
    }
}
