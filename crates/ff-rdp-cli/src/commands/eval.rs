use std::io::Read;

use anyhow::Context as _;
use ff_rdp_core::{
    ActorId, EvaluateScope, Grip, ObjectActor, ScopedGrip, TabActor, WebConsoleActor,
    sanitize_for_terminal,
};
use serde_json::json;

use crate::cli::args::Cli;
use crate::error::AppError;
use crate::hints::{HintContext, HintSource};
use crate::output;
use crate::output_pipeline::OutputPipeline;

use super::connect_tab::{connect_and_get_target, register_target_fronts};

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
/// # CSP safety — no page `eval()` call (iter-93)
///
/// Firefox's `evaluateJSAsync` routes through `Debugger.evalInGlobal` in
/// `devtools/server/actors/webconsole/eval-with-debugger.js:119-247`, which is
/// **not** subject to page Content Security Policy.  Page CSP restricts `eval()`
/// when called *from within a page script*, but the DevTools evaluator operates
/// at the Debugger API level, outside the page's scripting environment.
///
/// The previous isolation strategy wrapped the user code as
/// `(function() { "use strict"; return eval(<encoded>); })()`.  The outer IIFE
/// is fine — the Debugger evaluates it — but the inner `eval()` *is* a call to
/// the page's `eval` function, which IS blocked by the page's CSP.  That is
/// exactly what produced `EvalError: call to eval() blocked by CSP` on MDN and
/// other strict-CSP sites (dogfooding session 59).
///
/// The fix: drop the `eval()` isolation wrapper entirely.  The user script is
/// sent raw — Firefox evaluates it via the DevTools mechanism, which bypasses
/// page CSP by design.
///
/// # `--no-isolate` flag
///
/// `--no-isolate` is now effectively a no-op because isolation no longer relies
/// on `eval()`.  The flag is retained for CLI backward compatibility but does
/// not change the generated script.
///
/// # Stringify
///
/// `--stringify` wraps the expression in `JSON.stringify(...)` so the user gets
/// real values instead of Firefox grip metadata.  The stringify helper does NOT
/// use `eval()` and is therefore unaffected by page CSP.
pub(crate) fn build_script(user_script: &str, stringify: bool, _isolate: bool) -> String {
    // The stringify helper: if the value is already a string, return it as-is;
    // otherwise JSON.stringify it. This prevents double-encoding when the JS
    // expression already evaluates to a string (e.g. `document.title`).
    // Circular references throw a TypeError from JSON.stringify; we catch
    // that specific case and return a marker JSON object so the eval still
    // succeeds. All other thrown values (including BigInt's TypeError and
    // Symbol's TypeError) propagate up as eval exceptions.
    const STRINGIFY_HELPER: &str = "(function(v){if(typeof v===\"string\")return v;try{return JSON.stringify(v);}catch(e){if(e instanceof TypeError&&e.message.includes(\"circular\"))return \"{\\\"error\\\":\\\"circular reference detected\\\"}\";throw e;}})";

    if stringify {
        format!("(function(){{return {STRINGIFY_HELPER}({user_script});}})()")
    } else {
        user_script.to_owned()
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

/// CLI-side companion to [`EvaluateScope`] — owns `&str` slices borrowed
/// from clap so the dispatch site does not have to construct an
/// [`ActorId`] before deciding which connection path to take.
#[derive(Debug, Default, Clone, Copy)]
pub struct CliEvalScope<'a> {
    pub frame_actor: Option<&'a str>,
    pub selected_node_actor: Option<&'a str>,
    pub inner_window_id: Option<u64>,
}

impl CliEvalScope<'_> {
    /// Convert into an owned [`EvaluateScope`] for the core API, returning
    /// `None` when every field is unset (so callers can pass `None` to the
    /// scoped evaluator and stay on the legacy code path).
    pub fn to_scope(self) -> Option<EvaluateScope> {
        if self.frame_actor.is_none()
            && self.selected_node_actor.is_none()
            && self.inner_window_id.is_none()
        {
            return None;
        }
        Some(EvaluateScope {
            frame_actor: self.frame_actor.map(ActorId::from),
            selected_node_actor: self.selected_node_actor.map(ActorId::from),
            inner_window_id: self.inner_window_id,
        })
    }
}

#[allow(clippy::too_many_arguments, clippy::fn_params_excessive_bools)]
pub fn run(
    cli: &Cli,
    script: Option<&str>,
    file: Option<&str>,
    use_stdin: bool,
    stringify: bool,
    no_isolate: bool,
    unwrap: bool,
    cli_scope: CliEvalScope<'_>,
) -> Result<(), AppError> {
    let script = load_script(script, file, use_stdin)?;
    let scope = cli_scope.to_scope();
    // --no-isolate is a no-op since iter-93: the eval() isolation wrapper was
    // dropped because it triggered page CSP.  The flag is retained for CLI
    // backward compatibility.
    let final_script = build_script(&script, stringify, !no_isolate);

    let mut ctx = connect_and_get_target(cli)?;

    // The console actor ID is taken directly from the target descriptor
    // returned by `get_target`.  The retry path below re-fetches the target
    // if the actor turns out to be stale (noSuchActor / unknownActor).
    let console_actor = ctx.target.console_actor.clone();

    // Evaluate via the DevTools console actor.  Firefox routes this through
    // Debugger.evalInGlobal (eval-with-debugger.js:119-247), which bypasses
    // page CSP — no fallback to a chrome context is needed.
    let eval_result = match WebConsoleActor::evaluate_js_async_scoped(
        ctx.transport_mut(),
        &console_actor,
        &final_script,
        scope.as_ref(),
    ) {
        Ok(result) => result,
        Err(ff_rdp_core::ProtocolError::ActorError {
            kind: ff_rdp_core::ActorErrorKind::UnknownActor,
            ..
        }) => {
            // Actor is stale — re-resolve and retry once.
            let tab_actor = ctx.target_tab_actor().clone();
            let fresh_target =
                TabActor::get_target(ctx.transport_mut(), &tab_actor).map_err(AppError::from)?;
            register_target_fronts(ctx.registry(), &fresh_target);
            let fresh_console = fresh_target.console_actor.clone();
            ctx.target = fresh_target;
            WebConsoleActor::evaluate_js_async_scoped(
                ctx.transport_mut(),
                &fresh_console,
                &final_script,
                scope.as_ref(),
            )
            .map_err(AppError::from)?
        }
        Err(e) => return Err(AppError::from(e)),
    };

    // If an exception occurred, print it to stderr and exit non-zero.
    if let Some(ref exc) = eval_result.exception {
        let msg = exc
            .message
            .as_deref()
            .unwrap_or("evaluation threw an exception");
        let detail = exc.value.to_json();
        eprintln!("error: {}", sanitize_for_terminal(msg));
        eprintln!(
            "{}",
            serde_json::to_string_pretty(&detail).unwrap_or_default()
        );
        return Err(AppError::Exit(1));
    }

    // Compute the JSON representation before we potentially move the grip into
    // a ScopedGrip.  `to_json()` borrows `result`, so this must come first.
    let mut result_json = eval_result.result.to_json();

    // Wrap object/long-string grips in ScopedGrip so we can release them
    // before the process exits.  Firefox allocates a server-side actor for
    // each such grip returned by evaluateJSAsync; on long-lived daemon
    // connections these accumulate without bound.  We send `release` after
    // printing output so Firefox can free the actor immediately.
    //
    // Release applies equally in direct-connect and daemon-proxy modes: the
    // daemon transparently forwards all RDP frames, so the `release` packet
    // reaches Firefox through the same channel.
    let scoped_grip: Option<ScopedGrip> = match eval_result.result {
        g @ (Grip::Object { .. } | Grip::LongString { .. }) => Some(ScopedGrip::new(g)),
        _ => None,
    };

    // For object grips, enrich the output with the list of own property names.
    // Best-effort: if the actor is gone or the request fails, we skip silently.
    //
    // Firefox 149 removed the `ownPropertyNames` packet type, so we use
    // `prototypeAndProperties` and extract the keys from the result.
    if let Some(ref sg) = scoped_grip
        && let Grip::Object { ref actor, .. } = *sg.grip()
    {
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
    // Surface which evaluation path was taken (iter-61r Theme C / iter-93).
    // "page-await" = evaluateJSAsync routed through Debugger.evalInGlobal,
    //   which bypasses page CSP (devtools/server/actors/webconsole/
    //   eval-with-debugger.js:119-247).  This is always the path taken.
    if let Some(m) = meta.as_object_mut() {
        m.insert("eval_path".to_owned(), json!("page-await"));
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
    if unwrap
        && try_unwrap_json_string(&mut result_json)
        && let Some(m) = meta.as_object_mut()
    {
        m.insert("unwrapped".to_owned(), json!(true));
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
    let pipeline_result = pipeline
        .finalize_with_hints(&envelope, Some(&hint_ctx))
        .map_err(AppError::from);

    // Release the server-side object actor after output is flushed.
    //
    // We intentionally release *after* printing so the caller sees the full
    // output even if release fails.  Release failures are logged at WARN and
    // never propagate — a failed release means the actor leaks until the
    // connection closes, which is acceptable for one-shot CLI invocations.
    if let Some(sg) = scoped_grip
        && let Err(e) = sg.release(ctx.transport_mut())
    {
        tracing::warn!("eval: failed to release object actor: {e}");
    }

    pipeline_result
}

/// `--unwrap` helper: if `value` is a string whose contents parse as a JSON
/// object or array, replace `value` with the parsed structure and return
/// `true`.  Returns `false` and leaves `value` untouched otherwise (including
/// for valid JSON that parses to a primitive — numbers, booleans, null, or
/// plain strings).
fn try_unwrap_json_string(value: &mut serde_json::Value) -> bool {
    let serde_json::Value::String(s) = &*value else {
        return false;
    };
    let Ok(parsed) = serde_json::from_str::<serde_json::Value>(s) else {
        return false;
    };
    if matches!(
        parsed,
        serde_json::Value::Object(_) | serde_json::Value::Array(_)
    ) {
        *value = parsed;
        true
    } else {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_script_positional_passthrough() {
        let s = load_script(Some("document.title"), None, false).unwrap();
        assert_eq!(s, "document.title");
    }

    /// AC iter-80 Theme C: `eval_unwrap_parses_json_string`.
    /// A JSON-encoded object string is replaced with the parsed object.
    #[test]
    fn eval_unwrap_parses_json_string() {
        let mut v = serde_json::Value::String(r#"{"a":1}"#.to_owned());
        let unwrapped = try_unwrap_json_string(&mut v);
        assert!(
            unwrapped,
            "expected unwrap to succeed on JSON object string"
        );
        assert_eq!(v, serde_json::json!({"a": 1}));
    }

    /// Negative: a plain string is left unchanged.
    #[test]
    fn eval_unwrap_leaves_plain_string_unchanged() {
        let mut v = serde_json::Value::String("hello".to_owned());
        let unwrapped = try_unwrap_json_string(&mut v);
        assert!(
            !unwrapped,
            "plain non-JSON string must not be unwrapped: {v:?}"
        );
        assert_eq!(v, serde_json::Value::String("hello".to_owned()));
    }

    /// JSON-encoded primitive (e.g. `"42"`) must stay a string — only
    /// objects and arrays unwrap.
    #[test]
    fn eval_unwrap_leaves_primitive_string_unchanged() {
        let mut v = serde_json::Value::String("42".to_owned());
        let unwrapped = try_unwrap_json_string(&mut v);
        assert!(
            !unwrapped,
            "primitive JSON value must not trigger unwrap: {v:?}"
        );
        assert_eq!(v, serde_json::Value::String("42".to_owned()));
    }

    /// Arrays are also valid unwrap targets.
    #[test]
    fn eval_unwrap_parses_json_array_string() {
        let mut v = serde_json::Value::String("[1,2,3]".to_owned());
        let unwrapped = try_unwrap_json_string(&mut v);
        assert!(unwrapped);
        assert_eq!(v, serde_json::json!([1, 2, 3]));
    }

    /// Non-string values are never touched.
    #[test]
    fn eval_unwrap_skips_non_string_values() {
        let mut v = serde_json::json!({"already": "object"});
        let unwrapped = try_unwrap_json_string(&mut v);
        assert!(!unwrapped);
        assert_eq!(v, serde_json::json!({"already": "object"}));
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

    // ---------------------------------------------------------------------------
    // build_script: iter-93 — no eval() in any code path (CSP safety).
    //
    // Firefox routes evaluateJSAsync through Debugger.evalInGlobal, which
    // bypasses page CSP.  However, any eval() call inside the script IS subject
    // to page CSP.  So build_script must NEVER emit a bare `eval(` substring.
    // ---------------------------------------------------------------------------

    #[test]
    fn build_script_no_isolate_no_stringify_passthrough() {
        let s = build_script("document.title", false, false);
        assert_eq!(s, "document.title");
        // Must not contain a bare eval() call.
        assert!(!s.contains("eval("), "must not contain eval(): {s}");
    }

    #[test]
    fn build_script_isolate_flag_is_noop_passthrough() {
        // With isolate=true and stringify=false, result is still the raw script.
        // --isolate is a no-op since iter-93 (no eval() wrapper).
        let s = build_script("document.title", false, true);
        assert_eq!(s, "document.title");
        assert!(!s.contains("eval("), "must not contain eval(): {s}");
    }

    #[test]
    fn build_script_stringify_only_wraps_in_json_stringify() {
        let s = build_script("document.querySelectorAll('a')", true, false);
        // The stringify helper uses JSON.stringify for non-strings.
        assert!(s.contains("JSON.stringify("));
        assert!(s.contains("document.querySelectorAll('a')"));
        assert!(s.contains("circular"));
        // No bare eval() in any stringify path.
        assert!(!s.contains("eval("), "must not contain eval(): {s}");
        // Strings are passed through without double-encoding.
        assert!(s.contains("typeof v===\"string\""));
    }

    #[test]
    fn build_script_isolate_and_stringify_combine() {
        // isolate=true + stringify=true: result is stringify-wrapped, no eval().
        let s = build_script("document.querySelectorAll('a')", true, true);
        assert!(s.contains("JSON.stringify("));
        assert!(!s.contains("eval("), "must not contain eval(): {s}");
        assert!(s.contains("circular"));
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
        assert!(!s.contains("eval("), "must not contain eval(): {s}");
    }

    #[test]
    fn build_script_stringify_number_uses_json_stringify() {
        // For non-string values the helper falls through to JSON.stringify.
        let s = build_script("42", true, false);
        assert!(s.contains("JSON.stringify("));
        assert!(s.contains("42"));
        assert!(!s.contains("eval("), "must not contain eval(): {s}");
    }

    #[test]
    fn build_script_handles_special_chars() {
        // Quotes, backslashes, newlines: no-stringify path passes through raw.
        let input = "'a' + \"b\" + `c\nd`";
        let s = build_script(input, false, false);
        assert_eq!(s, input);
        assert!(!s.contains("eval("), "must not contain eval(): {s}");
    }

    /// Invariant: build_script MUST NOT emit a bare `eval(` for any
    /// combination of flags and user input.  This is the CSP-safety invariant
    /// introduced in iter-93.
    #[test]
    fn build_script_never_emits_eval_for_any_combination() {
        let scripts = [
            "document.title",
            "1 + 1",
            "const x = 1; x",
            "throw new Error('boom')",
        ];
        for &script in &scripts {
            for stringify in [false, true] {
                for isolate in [false, true] {
                    let s = build_script(script, stringify, isolate);
                    assert!(
                        !s.contains("eval("),
                        "eval() found in build_script({script:?}, stringify={stringify}, isolate={isolate}): {s}"
                    );
                }
            }
        }
    }
}
