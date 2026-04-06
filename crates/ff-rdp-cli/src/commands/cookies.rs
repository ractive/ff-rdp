use ff_rdp_core::{Grip, LongStringActor, WebConsoleActor};
use serde_json::{Value, json};

use crate::cli::args::Cli;
use crate::error::AppError;
use crate::output;
use crate::output_pipeline::OutputPipeline;

use super::connect_tab::{ConnectedTab, connect_and_get_target};

/// JavaScript that serialises `document.cookie` as a JSON string.
///
/// `document.cookie` returns `"key=value; key2=value2"`.  We split on `"; "`,
/// parse each pair at the first `=`, and return the result as a
/// `JSON.stringify`-encoded string so that it survives the Firefox RDP grip
/// encoding unchanged (an Array grip would not carry its element values inline).
const COOKIES_JS: &str = r"(function() {
  var raw = document.cookie;
  if (!raw) return '[]';
  var cookies = raw.split('; ').map(function(c) {
    var idx = c.indexOf('=');
    return {name: c.substring(0, idx), value: c.substring(idx + 1)};
  });
  return JSON.stringify(cookies);
})()";

pub fn run(cli: &Cli, name: Option<&str>) -> Result<(), AppError> {
    let mut ctx = connect_and_get_target(cli)?;
    let console_actor = ctx.target.console_actor.clone();

    let eval_result =
        WebConsoleActor::evaluate_js_async(ctx.transport_mut(), &console_actor, COOKIES_JS)
            .map_err(AppError::from)?;

    if let Some(ref exc) = eval_result.exception {
        let msg = exc.message.as_deref().unwrap_or("cookies query failed");
        eprintln!("error: {msg}");
        return Err(AppError::Exit(1));
    }

    let json_str = resolve_string(&mut ctx, &eval_result.result)?;

    let mut cookies: Value = serde_json::from_str(&json_str)
        .map_err(|e| AppError::from(anyhow::anyhow!("failed to parse cookie JSON: {e}")))?;

    // Client-side filter: keep only cookies whose name matches exactly.
    if let Some(filter_name) = name
        && let Some(arr) = cookies.as_array_mut()
    {
        arr.retain(|c| c.get("name").and_then(Value::as_str) == Some(filter_name));
    }

    let total = cookies.as_array().map_or(0, Vec::len);
    let meta = json!({"host": cli.host, "port": cli.port});
    let envelope = output::envelope(&cookies, total, &meta);

    OutputPipeline::new(cli.jq.clone())
        .finalize(&envelope)
        .map_err(AppError::from)
}

/// Resolve a Grip that is expected to be a plain or long string.
///
/// Returns the raw string content, fetching the full payload from the Firefox
/// RDP server when the grip is a `LongString`.
fn resolve_string(ctx: &mut ConnectedTab, grip: &Grip) -> Result<String, AppError> {
    match grip {
        Grip::Value(Value::String(s)) => Ok(s.clone()),
        Grip::LongString {
            actor,
            length,
            initial: _,
        } => LongStringActor::full_string(ctx.transport_mut(), actor.as_ref(), *length)
            .map_err(AppError::from),
        Grip::Null | Grip::Undefined => Ok("[]".to_owned()),
        other => Err(AppError::from(anyhow::anyhow!(
            "unexpected grip type for cookies result: {:?}",
            other.to_json()
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cookies_js_contains_document_cookie() {
        assert!(COOKIES_JS.contains("document.cookie"));
    }

    #[test]
    fn cookies_js_contains_json_stringify() {
        assert!(COOKIES_JS.contains("JSON.stringify"));
    }

    #[test]
    fn cookies_js_returns_empty_array_for_no_cookies() {
        // Verify the early-return branch for an empty cookie string.
        assert!(COOKIES_JS.contains("return '[]'"));
    }

    #[test]
    fn cookies_js_splits_on_semicolon_space() {
        assert!(COOKIES_JS.contains("split('; ')"));
    }
}
