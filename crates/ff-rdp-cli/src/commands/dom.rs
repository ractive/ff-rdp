use ff_rdp_core::{Grip, LongStringActor, WebConsoleActor};
use serde_json::{Value, json};

use crate::cli::args::Cli;
use crate::error::AppError;
use crate::output;
use crate::output_pipeline::OutputPipeline;

use super::connect_tab::{ConnectedTab, connect_and_get_target};
use super::js_helpers::escape_selector;

/// Sentinel prefix prepended to JSON.stringify results in the generated JS.
/// Used to distinguish structured multi-element results from plain strings
/// that happen to start with `[` or `{`.
const JSON_SENTINEL: &str = "__FF_RDP_JSON__";

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
    let escaped = escape_selector(selector);

    // Multi-element results and attrs are JSON.stringify'd with a sentinel
    // prefix so resolve_result can distinguish them from plain text that
    // happens to look like JSON.
    match mode {
        OutputMode::OuterHtml => format!(
            r"(function() {{
  var els = document.querySelectorAll('{escaped}');
  if (els.length === 0) return null;
  if (els.length === 1) return els[0].outerHTML;
  return '{JSON_SENTINEL}' + JSON.stringify(Array.from(els, function(e) {{ return e.outerHTML; }}));
}})()"
        ),
        OutputMode::InnerHtml => format!(
            r"(function() {{
  var els = document.querySelectorAll('{escaped}');
  if (els.length === 0) return null;
  if (els.length === 1) return els[0].innerHTML;
  return '{JSON_SENTINEL}' + JSON.stringify(Array.from(els, function(e) {{ return e.innerHTML; }}));
}})()"
        ),
        OutputMode::Text => format!(
            r"(function() {{
  var els = document.querySelectorAll('{escaped}');
  if (els.length === 0) return null;
  if (els.length === 1) return els[0].textContent;
  return '{JSON_SENTINEL}' + JSON.stringify(Array.from(els, function(e) {{ return e.textContent; }}));
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
  if (els.length === 1) return '{JSON_SENTINEL}' + JSON.stringify(attrs(els[0]));
  return '{JSON_SENTINEL}' + JSON.stringify(Array.from(els, attrs));
}})()"
        ),
    }
}

/// Resolve the eval result to a JSON value, fetching LongStrings.
///
/// Multi-element results and attrs are prefixed with [`JSON_SENTINEL`] to
/// distinguish them from plain text.  Only strings with that sentinel are
/// parsed back into structured JSON.
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

    // Strip the sentinel and parse the JSON payload.
    if let Some(s) = raw.as_str()
        && let Some(json_str) = s.strip_prefix(JSON_SENTINEL)
    {
        return serde_json::from_str::<Value>(json_str)
            .map_err(|e| AppError::from(anyhow::anyhow!("failed to parse DOM result JSON: {e}")));
    }
    Ok(raw)
}

#[cfg(test)]
mod tests {
    use super::super::js_helpers::escape_selector;
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
        // Single quotes are now escaped for safe embedding in '…' JS literals.
        assert!(js.contains(r"div[data-name=\'test\']"));
    }

    #[test]
    fn escape_selector_handles_special_chars() {
        // Newlines and backslashes should be escaped
        assert_eq!(escape_selector("a\nb"), r"a\nb");
        assert_eq!(escape_selector(r"a\b"), r"a\\b");
        // Double quotes are escaped (embedded in single-quoted JS literal)
        assert_eq!(escape_selector(r#"a"b"#), r#"a\"b"#);
    }

    #[test]
    fn build_js_multi_uses_sentinel() {
        let js = build_js("li", OutputMode::Text);
        assert!(js.contains(JSON_SENTINEL));
    }
}
