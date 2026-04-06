use ff_rdp_core::WebConsoleActor;
use serde_json::json;

use crate::cli::args::Cli;
use crate::error::AppError;
use crate::output;
use crate::output_pipeline::OutputPipeline;

use super::connect_tab::connect_and_get_target;
use super::js_helpers::escape_selector;

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
  if (!el) throw new Error('Element not found: {escaped_sel}');
  {clear_stmt}
  el.value = {escaped_text_json};
  el.dispatchEvent(new Event('input', {{bubbles: true}}));
  el.dispatchEvent(new Event('change', {{bubbles: true}}));
  return {{typed: true, value: el.value}};
}})()"
    );

    let eval_result = WebConsoleActor::evaluate_js_async(ctx.transport_mut(), &console_actor, &js)
        .map_err(AppError::from)?;

    if let Some(ref exc) = eval_result.exception {
        let msg = exc.message.as_deref().unwrap_or("type failed");
        eprintln!("error: {msg}");
        return Err(AppError::Exit(1));
    }

    let result_json = eval_result.result.to_json();
    let meta = json!({"host": cli.host, "port": cli.port, "selector": selector});
    let envelope = output::envelope(&result_json, 1, &meta);

    OutputPipeline::new(cli.jq.clone())
        .finalize(&envelope)
        .map_err(AppError::from)
}
