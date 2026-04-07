use ff_rdp_core::{LongStringActor, WebConsoleActor};
use serde_json::json;

use crate::cli::args::Cli;
use crate::error::AppError;
use crate::output;
use crate::output_pipeline::OutputPipeline;

use super::connect_tab::connect_and_get_target;

pub fn run(cli: &Cli) -> Result<(), AppError> {
    let mut ctx = connect_and_get_target(cli)?;
    let console_actor = ctx.target.console_actor.clone();

    let eval_result = WebConsoleActor::evaluate_js_async(
        ctx.transport_mut(),
        &console_actor,
        "document.body.innerText",
    )
    .map_err(AppError::from)?;

    if let Some(ref exc) = eval_result.exception {
        let msg = exc
            .message
            .as_deref()
            .unwrap_or("failed to extract page text");
        eprintln!("error: {msg}");
        return Err(AppError::Exit(1));
    }

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
