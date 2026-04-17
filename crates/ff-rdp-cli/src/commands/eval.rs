use std::io::Read;

use anyhow::Context as _;
use ff_rdp_core::{Grip, ObjectActor, WebConsoleActor};
use serde_json::json;

use crate::cli::args::Cli;
use crate::error::AppError;
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

pub fn run(
    cli: &Cli,
    script: Option<&str>,
    file: Option<&str>,
    use_stdin: bool,
    stringify: bool,
) -> Result<(), AppError> {
    let script = load_script(script, file, use_stdin)?;

    // When --stringify is set, wrap the expression so we get actual data instead of actor grips.
    let script_owned;
    let script = if stringify {
        script_owned = format!(
            "(function() {{ try {{ return JSON.stringify({script}); }} catch(e) {{ if (e instanceof TypeError && e.message.includes('circular')) return '{{\"error\":\"circular reference detected\"}}'; throw e; }} }})()"
        );
        script_owned.as_str()
    } else {
        script.as_str()
    };

    let mut ctx = connect_and_get_target(cli)?;
    let console_actor = ctx.target.console_actor.clone();

    let eval_result =
        WebConsoleActor::evaluate_js_async(ctx.transport_mut(), &console_actor, script)
            .map_err(AppError::from)?;

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

    let meta = json!({"host": cli.host, "port": cli.port});
    let envelope = output::envelope(&result_json, 1, &meta);

    OutputPipeline::from_cli(cli)?
        .finalize(&envelope)
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
    // --stringify wrapping tests
    // ---------------------------------------------------------------------------

    /// Helper: simulate what run() does with the stringify flag, without connecting
    /// to Firefox — just test the script transformation logic.
    fn apply_stringify(expr: &str, stringify: bool) -> String {
        let script = expr.to_owned();
        if stringify {
            format!(
                "(function() {{ try {{ return JSON.stringify({script}); }} catch(e) {{ if (e instanceof TypeError && e.message.includes('circular')) return '{{\"error\":\"circular reference detected\"}}'; throw e; }} }})()"
            )
        } else {
            script
        }
    }

    #[test]
    fn stringify_false_leaves_script_unchanged() {
        let expr = "document.querySelectorAll('a')";
        let result = apply_stringify(expr, false);
        assert_eq!(result, expr);
    }

    #[test]
    fn stringify_true_wraps_in_json_stringify() {
        let expr = "document.querySelectorAll('a')";
        let result = apply_stringify(expr, true);
        assert!(
            result.contains("JSON.stringify("),
            "should wrap in JSON.stringify: {result}"
        );
        assert!(
            result.contains(expr),
            "original expression should be present: {result}"
        );
    }

    #[test]
    fn stringify_wraps_in_iife_with_circular_guard() {
        let expr = "window.location";
        let result = apply_stringify(expr, true);
        assert!(
            result.starts_with("(function()"),
            "should be an IIFE: {result}"
        );
        assert!(
            result.contains("circular"),
            "should guard against circular refs: {result}"
        );
    }
}
