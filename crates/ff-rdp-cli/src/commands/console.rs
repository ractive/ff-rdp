use ff_rdp_core::WebConsoleActor;
use serde_json::json;

use crate::cli::args::Cli;
use crate::error::AppError;
use crate::output;
use crate::output_pipeline::OutputPipeline;

use super::connect_tab::connect_and_get_target;

pub fn run(cli: &Cli, level: Option<&str>, pattern: Option<&str>) -> Result<(), AppError> {
    let mut ctx = connect_and_get_target(cli)?;
    let console_actor = ctx.target.console_actor.clone();

    // Start listeners so Firefox tracks console events for this connection.
    WebConsoleActor::start_listeners(
        ctx.transport_mut(),
        &console_actor,
        &["PageError", "ConsoleAPI"],
    )
    .map_err(AppError::from)?;

    // Retrieve all cached console messages.
    let messages = WebConsoleActor::get_cached_messages(
        ctx.transport_mut(),
        &console_actor,
        &["PageError", "ConsoleAPI"],
    )
    .map_err(AppError::from)?;

    // Apply filters.
    let regex = pattern
        .map(|p| {
            regex::Regex::new(p)
                .map_err(|e| AppError::User(format!("invalid --pattern regex: {e}")))
        })
        .transpose()?;

    let filtered: Vec<_> = messages
        .into_iter()
        .filter(|msg| {
            if let Some(l) = level
                && !msg.level.eq_ignore_ascii_case(l)
            {
                return false;
            }
            if let Some(ref re) = regex
                && !re.is_match(&msg.message)
            {
                return false;
            }
            true
        })
        .collect();

    // Convert to JSON output.
    let results: Vec<serde_json::Value> = filtered
        .iter()
        .map(|msg| {
            json!({
                "level": msg.level,
                "message": msg.message,
                "source": msg.source,
                "line": msg.line,
                "timestamp": msg.timestamp,
            })
        })
        .collect();

    let total = results.len();
    let results_json = json!(results);
    let meta = json!({"host": cli.host, "port": cli.port});
    let envelope = output::envelope(&results_json, total, &meta);

    OutputPipeline::new(cli.jq.clone())
        .finalize(&envelope)
        .map_err(AppError::from)
}
