use ff_rdp_core::{Grip, LongStringActor, WebConsoleActor};
use serde_json::{Value, json};

use crate::cli::args::Cli;
use crate::error::AppError;
use crate::output;
use crate::output_controls::{OutputControls, SortDir};
use crate::output_pipeline::OutputPipeline;

use super::connect_tab::connect_and_get_target;
use super::js_helpers::{JSON_SENTINEL, escape_selector, resolve_result};

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
    let meta = json!({"host": cli.host, "port": cli.port, "selector": selector});

    // Apply output controls when results is an array (multi-element queries).
    // DOM results are in document order — no default sort applied.
    if let Value::Array(arr) = results {
        let controls = OutputControls::from_cli(cli, SortDir::Asc);
        let mut items = arr;
        controls.apply_sort(&mut items);
        let (limited, total, truncated) = controls.apply_limit(items, Some(20));
        let shown = limited.len();
        let limited = controls.apply_fields(limited);
        let envelope =
            output::envelope_with_truncation(&json!(limited), shown, total, truncated, &meta);
        return OutputPipeline::new(cli.jq.clone())
            .finalize(&envelope)
            .map_err(AppError::from);
    }

    let total = match &results {
        Value::Null => 0,
        _ => 1,
    };

    let envelope = output::envelope(&results, total, &meta);

    OutputPipeline::new(cli.jq.clone())
        .finalize(&envelope)
        .map_err(AppError::from)
}

pub fn run_count(cli: &Cli, selector: &str) -> Result<(), AppError> {
    let mut ctx = connect_and_get_target(cli)?;
    let console_actor = ctx.target.console_actor.clone();

    let escaped = escape_selector(selector);
    let js = format!("document.querySelectorAll('{escaped}').length");

    let eval_result = WebConsoleActor::evaluate_js_async(ctx.transport_mut(), &console_actor, &js)
        .map_err(AppError::from)?;

    if let Some(ref exc) = eval_result.exception {
        let msg = exc.message.as_deref().unwrap_or("DOM count query failed");
        eprintln!("error: {msg}");
        return Err(AppError::Exit(1));
    }

    let count = match &eval_result.result {
        Grip::Value(v) => v.as_u64().unwrap_or(0),
        _ => 0,
    };

    let results = json!({"selector": selector, "count": count});
    let meta = json!({"host": cli.host, "port": cli.port, "selector": selector});
    let envelope = output::envelope(&results, usize::try_from(count).unwrap_or(0), &meta);

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

/// JavaScript IIFE that collects DOM statistics in a single evaluation.
const STATS_JS: &str = r"(function() {
  var nodeCount = document.getElementsByTagName('*').length;
  var docSize = document.documentElement.outerHTML.length;
  var scripts = document.getElementsByTagName('script');
  var inlineScriptCount = 0;
  for (var i = 0; i < scripts.length; i++) {
    if (!scripts[i].getAttribute('src')) inlineScriptCount++;
  }
  var head = document.head || document.getElementsByTagName('head')[0];
  var renderBlockingCount = 0;
  if (head) {
    var headLinks = head.getElementsByTagName('link');
    for (var j = 0; j < headLinks.length; j++) {
      if (headLinks[j].getAttribute('rel') === 'stylesheet') renderBlockingCount++;
    }
    var headScripts = head.getElementsByTagName('script');
    for (var k = 0; k < headScripts.length; k++) {
      var hs = headScripts[k];
      if (!hs.hasAttribute('async') && !hs.hasAttribute('defer')) renderBlockingCount++;
    }
  }
  var imgs = document.getElementsByTagName('img');
  var imagesWithoutLazy = 0;
  for (var m = 0; m < imgs.length; m++) {
    var img = imgs[m];
    var rect = img.getBoundingClientRect();
    var inViewport = rect.top < window.innerHeight && rect.bottom >= 0;
    if (!inViewport && img.getAttribute('loading') !== 'lazy') imagesWithoutLazy++;
  }
  return JSON.stringify({
    node_count: nodeCount,
    document_size: docSize,
    inline_script_count: inlineScriptCount,
    render_blocking_count: renderBlockingCount,
    images_without_lazy: imagesWithoutLazy
  });
})()";

pub fn run_stats(cli: &Cli) -> Result<(), AppError> {
    let mut ctx = connect_and_get_target(cli)?;
    let console_actor = ctx.target.console_actor.clone();

    let eval_result =
        WebConsoleActor::evaluate_js_async(ctx.transport_mut(), &console_actor, STATS_JS)
            .map_err(AppError::from)?;

    if let Some(ref exc) = eval_result.exception {
        let msg = exc.message.as_deref().unwrap_or("DOM stats query failed");
        eprintln!("error: {msg}");
        return Err(AppError::Exit(1));
    }

    let json_str = match &eval_result.result {
        Grip::Value(Value::String(s)) => s.clone(),
        Grip::LongString {
            actor,
            length,
            initial: _,
        } => LongStringActor::full_string(ctx.transport_mut(), actor.as_ref(), *length)
            .map_err(AppError::from)?,
        Grip::Null | Grip::Undefined => {
            return Err(AppError::User("DOM stats returned no result".to_string()));
        }
        other => {
            return Err(AppError::User(format!(
                "unexpected DOM stats result type: {:?}",
                other.to_json()
            )));
        }
    };

    let stats: Value = serde_json::from_str(&json_str)
        .map_err(|e| AppError::from(anyhow::anyhow!("failed to parse DOM stats JSON: {e}")))?;

    let meta = json!({"host": cli.host, "port": cli.port});
    let envelope = output::envelope(&stats, 1, &meta);

    OutputPipeline::new(cli.jq.clone())
        .finalize(&envelope)
        .map_err(AppError::from)
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

    #[test]
    fn build_count_js() {
        let escaped = escape_selector("script");
        let js = format!("document.querySelectorAll('{escaped}').length");
        assert!(js.contains("querySelectorAll('script')"));
        assert!(js.contains(".length"));
    }
}
