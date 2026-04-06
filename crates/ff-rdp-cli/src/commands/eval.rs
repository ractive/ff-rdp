use ff_rdp_core::WebConsoleActor;
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

    let result_json = eval_result.result.to_json();
    let meta = json!({"host": cli.host, "port": cli.port});
    let envelope = output::envelope(&result_json, 1, &meta);

    OutputPipeline::new(cli.jq.clone())
        .finalize(&envelope)
        .map_err(AppError::from)
}
