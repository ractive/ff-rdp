use ff_rdp_core::{Grip, LongStringActor, WebConsoleActor};
use serde_json::{Value, json};

use crate::cli::args::Cli;
use crate::error::AppError;
use crate::output;
use crate::output_pipeline::OutputPipeline;

use super::connect_tab::{ConnectedTab, connect_and_get_target};

#[derive(Debug, Clone, Copy)]
pub enum OutputMode {
    OuterHtml,
    InnerHtml,
    Text,
    Attrs,
}

pub fn run(cli: &Cli, selector: &str, mode: OutputMode) -> Result<(), AppError> {
    let mut ctx = connect_and_get_target(cli)?;
    let console_actor = ctx.target.console_actor.clone();

    let js = build_js(selector, mode);

    let eval_result = WebConsoleActor::evaluate_js_async(ctx.transport_mut(), &console_actor, &js)
        .map_err(AppError::from)?;

    if let Some(ref exc) = eval_result.exception {
        let msg = exc.message.as_deref().unwrap_or("DOM query failed");
        eprintln!("error: {msg}");
        return Err(AppError::Exit(1));
    }

    let results = resolve_result(&mut ctx, &eval_result.result)?;

    let total = match &results {
        Value::Array(arr) => arr.len(),
        Value::Null => 0,
        _ => 1,
    };

    let meta = json!({"host": cli.host, "port": cli.port, "selector": selector});
    let envelope = output::envelope(&results, total, &meta);

    OutputPipeline::new(cli.jq.clone())
        .finalize(&envelope)
        .map_err(AppError::from)
}

fn build_js(selector: &str, mode: OutputMode) -> String {
    // Escape the selector for embedding in a JS string literal.
    let escaped = selector.replace('\\', "\\\\").replace('\'', "\\'");

    // For multi-element results we JSON.stringify the array so Firefox returns
    // a plain string (or longString) rather than an object grip.  Single-element
    // results are returned as-is so short values stay as plain JSON strings.
    match mode {
        OutputMode::OuterHtml => format!(
            r"(function() {{
  var els = document.querySelectorAll('{escaped}');
  if (els.length === 0) return null;
  if (els.length === 1) return els[0].outerHTML;
  return JSON.stringify(Array.from(els, function(e) {{ return e.outerHTML; }}));
}})()"
        ),
        OutputMode::InnerHtml => format!(
            r"(function() {{
  var els = document.querySelectorAll('{escaped}');
  if (els.length === 0) return null;
  if (els.length === 1) return els[0].innerHTML;
  return JSON.stringify(Array.from(els, function(e) {{ return e.innerHTML; }}));
}})()"
        ),
        OutputMode::Text => format!(
            r"(function() {{
  var els = document.querySelectorAll('{escaped}');
  if (els.length === 0) return null;
  if (els.length === 1) return els[0].textContent;
  return JSON.stringify(Array.from(els, function(e) {{ return e.textContent; }}));
}})()"
        ),
        OutputMode::Attrs => format!(
            r"(function() {{
  function attrs(e) {{
    var o = {{}};
    for (var i = 0; i < e.attributes.length; i++) {{
      o[e.attributes[i].name] = e.attributes[i].value;
    }}
    return o;
  }}
  var els = document.querySelectorAll('{escaped}');
  if (els.length === 0) return null;
  if (els.length === 1) return JSON.stringify(attrs(els[0]));
  return JSON.stringify(Array.from(els, attrs));
}})()"
        ),
    }
}

/// Resolve the eval result to a JSON value, fetching LongStrings.
///
/// The JS we emit uses `JSON.stringify` for multi-element results and for attrs,
/// so the raw result is a JSON-encoded string.  We detect this and parse it back
/// into structured JSON.
fn resolve_result(ctx: &mut ConnectedTab, grip: &Grip) -> Result<Value, AppError> {
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

    // If the value is a string that looks like JSON array/object, parse it.
    // This handles the JSON.stringify'd multi-element results.
    if let Some(s) = raw.as_str()
        && (s.starts_with('[') || s.starts_with('{'))
        && let Ok(parsed) = serde_json::from_str::<Value>(s)
    {
        return Ok(parsed);
    }
    Ok(raw)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_js_outer_html() {
        let js = build_js("h1", OutputMode::OuterHtml);
        assert!(js.contains("querySelectorAll('h1')"));
        assert!(js.contains("outerHTML"));
    }

    #[test]
    fn build_js_text() {
        let js = build_js(".content", OutputMode::Text);
        assert!(js.contains("textContent"));
    }

    #[test]
    fn build_js_attrs() {
        let js = build_js("a", OutputMode::Attrs);
        assert!(js.contains("attributes"));
    }

    #[test]
    fn build_js_inner_html() {
        let js = build_js("div", OutputMode::InnerHtml);
        assert!(js.contains("innerHTML"));
    }

    #[test]
    fn build_js_escapes_selector() {
        let js = build_js("div[data-name='test']", OutputMode::Text);
        assert!(js.contains(r"div[data-name=\'test\']"));
    }
}
