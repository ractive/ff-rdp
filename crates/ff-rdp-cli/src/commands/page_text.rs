use ff_rdp_core::LongStringActor;
use serde_json::json;

use crate::cli::args::Cli;
use crate::error::AppError;
use crate::output;
use crate::output_pipeline::OutputPipeline;

use super::connect_tab::connect_and_get_target;
use super::js_helpers::eval_or_bail;

pub fn run(cli: &Cli) -> Result<(), AppError> {
    let mut ctx = connect_and_get_target(cli)?;
    let console_actor = ctx.target.console_actor.clone();

    let eval_result = eval_or_bail(
        &mut ctx,
        &console_actor,
        "document.body.innerText",
        "failed to extract page text",
    )?;

    let text = resolve_string_result(&mut ctx, &eval_result.result)?;

    let meta = json!({"host": cli.host, "port": cli.port});
    let envelope = output::envelope(&json!(text), 1, &meta);

    OutputPipeline::from_cli(cli)?
        .finalize(&envelope)
        .map_err(AppError::from)
}

/// Resolve a Grip to a string, fetching the full content if it's a LongString.
fn resolve_string_result(
    ctx: &mut super::connect_tab::ConnectedTab,
    grip: &ff_rdp_core::Grip,
) -> Result<String, AppError> {
    match grip {
        ff_rdp_core::Grip::Value(serde_json::Value::String(s)) => Ok(s.clone()),
        ff_rdp_core::Grip::LongString {
            actor,
            length,
            initial: _,
        } => LongStringActor::full_string(ctx.transport_mut(), actor.as_ref(), *length)
            .map_err(AppError::from),
        ff_rdp_core::Grip::Null | ff_rdp_core::Grip::Undefined => Ok(String::new()),
        other => Ok(other.to_json().to_string()),
    }
}
