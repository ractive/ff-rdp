use serde_json::json;

use crate::cli::args::Cli;
use crate::error::AppError;
use crate::output;
use crate::output_pipeline::OutputPipeline;

use super::connect_tab::connect_and_get_target;
use super::js_helpers::{JSON_SENTINEL, escape_selector, eval_or_bail, resolve_result};

pub fn run(cli: &Cli, selector: &str) -> Result<(), AppError> {
    let mut ctx = connect_and_get_target(cli)?;
    let console_actor = ctx.target.console_actor.clone();

    let escaped = escape_selector(selector);
    let js = format!(
        r"(function() {{
  var el = document.querySelector('{escaped}');
  if (!el) throw new Error('Element not found: {escaped} — use ff-rdp dom SELECTOR --count to verify the selector matches');
  el.click();
  return '{JSON_SENTINEL}' + JSON.stringify({{clicked: true, tag: el.tagName, text: (el.textContent || '').trim().substring(0, 100)}});
}})()"
    );

    let eval_result = eval_or_bail(&mut ctx, &console_actor, &js, "click failed")?;

    let result_json = resolve_result(&mut ctx, &eval_result.result)?;
    let meta = json!({"host": cli.host, "port": cli.port, "selector": selector});
    let envelope = output::envelope(&result_json, 1, &meta);

    OutputPipeline::from_cli(cli)?
        .finalize(&envelope)
        .map_err(AppError::from)
}
