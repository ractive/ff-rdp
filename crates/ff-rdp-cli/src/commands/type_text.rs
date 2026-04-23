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
    // Escape the text value using JSON encoding so special chars are safe in JS strings.
    let escaped_text_json = serde_json::to_string(text)
        .map_err(|e| AppError::from(anyhow::anyhow!("failed to encode text argument: {e}")))?;
    // escaped_text_json is already a JSON string literal (with quotes), use it directly.
    let clear_stmt = if clear { "el.value = '';" } else { "" };

    let js = format!(
        r"(function() {{
  var el = document.querySelector('{escaped_sel}');
  if (!el) throw new Error('Element not found: {escaped_sel} — use ff-rdp dom SELECTOR --count to verify the selector matches');
  {clear_stmt}
  el.value = {escaped_text_json};
  el.dispatchEvent(new Event('input', {{bubbles: true}}));
  el.dispatchEvent(new Event('change', {{bubbles: true}}));
  return '{JSON_SENTINEL}' + JSON.stringify({{typed: true, tag: el.tagName, value: el.value}});
}})()"
    );

    let eval_result = eval_or_bail(&mut ctx, &console_actor, &js, "type failed")?;

    let result_json = resolve_result(&mut ctx, &eval_result.result)?;
    let meta = json!({"host": cli.host, "port": cli.port, "selector": selector});
    let envelope = output::envelope(&result_json, 1, &meta);

    let hint_ctx = HintContext::new(HintSource::TypeText).with_selector(selector);
    OutputPipeline::from_cli(cli)?
        .finalize_with_hints(&envelope, Some(&hint_ctx))
        .map_err(AppError::from)
}
