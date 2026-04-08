use ff_rdp_core::{Grip, ObjectActor, WebConsoleActor};
use serde_json::json;

use crate::cli::args::Cli;
use crate::error::AppError;
use crate::output;
use crate::output_pipeline::OutputPipeline;

use super::connect_tab::connect_and_get_target;

pub fn run(cli: &Cli, script: &str) -> Result<(), AppError> {
    let mut ctx = connect_and_get_target(cli)?;
    let console_actor = ctx.target.console_actor.clone();

    let eval_result =
        WebConsoleActor::evaluate_js_async(ctx.transport_mut(), &console_actor, script)
            .map_err(AppError::from)?;

    // If an exception occurred, print it to stderr and exit non-zero.
    // Also print the exception value as pretty JSON for richer debugging output —
    // this extra detail is why eval.rs keeps the explicit check rather than
    // delegating to eval_or_bail.
    if let Some(ref exc) = eval_result.exception {
        let msg = exc
            .message
            .as_deref()
            .unwrap_or("evaluation threw an exception");
        let detail = exc.value.to_json();
        eprintln!("error: {msg}");
        eprintln!(
            "{}",
            serde_json::to_string_pretty(&detail).unwrap_or_default()
        );
        return Err(AppError::Exit(1));
    }

    let mut result_json = eval_result.result.to_json();

    // For object grips, enrich the output with the list of own property names.
    // Best-effort: if the actor is gone or the request fails, we skip silently.
    //
    // Firefox 149 removed the `ownPropertyNames` packet type, so we use
    // `prototypeAndProperties` and extract the keys from the result.
    if let Grip::Object { ref actor, .. } = eval_result.result {
        match ObjectActor::prototype_and_properties(ctx.transport_mut(), actor.as_ref()) {
            Ok(pap) => {
                let names: Vec<&str> = pap.own_properties.keys().map(String::as_str).collect();
                result_json["propertyNames"] = json!(names);
            }
            Err(e) => {
                eprintln!("warning: could not fetch property names: {e}");
            }
        }
    }

    let meta = json!({"host": cli.host, "port": cli.port});
    let envelope = output::envelope(&result_json, 1, &meta);

    OutputPipeline::from_cli(cli)?
        .finalize(&envelope)
        .map_err(AppError::from)
}
