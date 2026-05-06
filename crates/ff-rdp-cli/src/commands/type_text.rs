use serde_json::json;

use crate::cli::args::Cli;
use crate::error::AppError;
use crate::hints::{HintContext, HintSource};
use crate::output;
use crate::output_pipeline::OutputPipeline;

use super::connect_tab::connect_and_get_target;
use super::js_helpers::{JSON_SENTINEL, escape_selector, eval_or_bail, resolve_result};

pub fn run(cli: &Cli, selector: &str, text: &str, clear: bool) -> Result<(), AppError> {
    let mut ctx = connect_and_get_target(cli)?;
    let console_actor = ctx.target.console_actor.clone();

    let escaped_sel = escape_selector(selector);
    let escaped_text_json = serde_json::to_string(text)
        .map_err(|e| AppError::from(anyhow::anyhow!("failed to encode text argument: {e}")))?;
    let clear_flag_js = if clear { "true" } else { "false" };

    // React/Vue/Svelte track input values via a hidden tracker on the element
    // (see React's input-value-tracking module).  Setting `el.value = ...`
    // directly bypasses the framework setter, so the change is silently
    // discarded.  We invoke the native prototype setter (cached at
    // module-eval time below) to invalidate the tracker, then dispatch the
    // synthetic `input`/`change` events the framework listeners expect.
    let js = format!(
        r#"(function() {{
  "use strict";
  var el = document.querySelector('{escaped_sel}');
  if (!el) throw new Error('Element not found: {escaped_sel} — use ff-rdp dom SELECTOR --count to verify the selector matches');
  var setter = null;
  if (window.HTMLInputElement && el instanceof window.HTMLInputElement) {{
    setter = Object.getOwnPropertyDescriptor(window.HTMLInputElement.prototype, 'value').set;
  }} else if (window.HTMLTextAreaElement && el instanceof window.HTMLTextAreaElement) {{
    setter = Object.getOwnPropertyDescriptor(window.HTMLTextAreaElement.prototype, 'value').set;
  }} else if (window.HTMLSelectElement && el instanceof window.HTMLSelectElement) {{
    setter = Object.getOwnPropertyDescriptor(window.HTMLSelectElement.prototype, 'value').set;
  }}
  function applyValue(v) {{
    if (setter) {{ setter.call(el, v); }} else {{ el.value = v; }}
  }}
  if ({clear_flag_js}) {{ applyValue(''); }}
  applyValue({escaped_text_json});
  el.dispatchEvent(new Event('input', {{bubbles: true}}));
  el.dispatchEvent(new Event('change', {{bubbles: true}}));
  return '{JSON_SENTINEL}' + JSON.stringify({{typed: true, tag: el.tagName, value: el.value}});
}})()"#
    );

    let eval_result = eval_or_bail(&mut ctx, &console_actor, &js, "type failed")?;

    let result_json = resolve_result(&mut ctx, &eval_result.result)?;
    let mut meta = json!({"host": cli.host, "port": cli.port, "selector": selector});
    crate::connection_meta::merge_into(&mut meta, &cli.host, cli.port, None);
    let envelope = output::envelope(&result_json, 1, &meta);

    let hint_ctx = HintContext::new(HintSource::TypeText).with_selector(selector);
    OutputPipeline::from_cli(cli)?
        .finalize_with_hints(&envelope, Some(&hint_ctx))
        .map_err(AppError::from)
}
