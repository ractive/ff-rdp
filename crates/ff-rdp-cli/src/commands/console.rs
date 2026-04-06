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

    // Start listeners — best-effort; some Firefox builds reject certain listener types.
    if let Err(e) = WebConsoleActor::start_listeners(
        ctx.transport_mut(),
        &console_actor,
        &["PageError", "ConsoleAPI"],
    ) {
        eprintln!("warning: startListeners failed: {e}");
    }

    // Retrieve all cached console messages.
    // If the combined request fails (Firefox may reject PageError serialization),
    // fall back to ConsoleAPI-only to recover partial results.
    let messages = match WebConsoleActor::get_cached_messages(
        ctx.transport_mut(),
        &console_actor,
        &["PageError", "ConsoleAPI"],
    ) {
        Ok(msgs) => msgs,
        Err(_) => WebConsoleActor::get_cached_messages(
            ctx.transport_mut(),
            &console_actor,
            &["ConsoleAPI"],
        )
        .map_err(AppError::from)?,
    };

    // Apply filters.
    let regex = pattern
        .map(|p| {
            regex::RegexBuilder::new(p)
                .size_limit(1_000_000)
                .build()
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

#[cfg(test)]
mod tests {
    /// Verify that a normal pattern compiles successfully under the size limit.
    #[test]
    fn accepts_reasonable_regex() {
        let result = regex::RegexBuilder::new(r"(?i)error|warn")
            .size_limit(1_000_000)
            .build();
        assert!(result.is_ok());
    }

    /// Verify that a pattern exceeding a small compiled-regex size limit is rejected.
    #[test]
    fn rejects_oversized_regex() {
        let oversized = (0..100)
            .map(|i| format!("literal_{i}"))
            .collect::<Vec<_>>()
            .join("|");
        let result = regex::RegexBuilder::new(&oversized).size_limit(64).build();
        assert!(result.is_err(), "expected oversized pattern to be rejected");
    }
}
