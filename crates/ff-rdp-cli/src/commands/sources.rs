use ff_rdp_core::ThreadActor;
use serde_json::{Value, json};

use crate::cli::args::Cli;
use crate::error::AppError;
use crate::output;
use crate::output_pipeline::OutputPipeline;

use super::connect_tab::connect_and_get_target;

pub fn run(cli: &Cli, filter: Option<&str>, pattern: Option<&str>) -> Result<(), AppError> {
    let mut ctx = connect_and_get_target(cli)?;

    let thread_actor = ctx
        .target
        .thread_actor
        .clone()
        .ok_or_else(|| AppError::User("target does not expose a thread actor".into()))?;

    // Firefox error messages are matched by substring — fragile but necessary
    // since RDP actor errors lack structured error codes.
    let sources = ThreadActor::list_sources(ctx.transport_mut(), thread_actor.as_ref()).map_err(
        |e| match &e {
            ff_rdp_core::ProtocolError::ActorError { error, .. }
                if error == "unrecognizedPacketType" =>
            {
                AppError::User(
                    "sources: Firefox does not recognise the 'sources' method on the thread \
                     actor — this indicates a protocol change in Firefox 125+. \
                     The thread-based source listing API may have been removed or moved. \
                     Try the `eval` command to inspect `performance.getEntries()` instead."
                        .to_owned(),
                )
            }
            ff_rdp_core::ProtocolError::ActorError { message, .. }
                if message.contains("undefined") || message.contains("not available") =>
            {
                AppError::User(format!(
                    "sources: Firefox returned an error ({message}) — \
                     the page may not have loaded scripts yet, or this Firefox version \
                     (125+) may use a different protocol for source listing"
                ))
            }
            _ => AppError::from(e),
        },
    )?;

    // Apply filters.
    let regex = pattern
        .map(|p| {
            regex::RegexBuilder::new(p)
                .size_limit(1_000_000)
                .build()
                .map_err(|e| AppError::User(format!("invalid --pattern regex: {e}")))
        })
        .transpose()?;

    let filtered: Vec<_> = sources
        .iter()
        .filter(|s| {
            if let Some(f) = filter
                && !s.url.contains(f)
            {
                return false;
            }
            if let Some(ref re) = regex
                && !re.is_match(&s.url)
            {
                return false;
            }
            true
        })
        .collect();

    let results: Vec<Value> = filtered
        .iter()
        .map(|s| {
            json!({
                "url": s.url,
                "actor": s.actor,
                "isBlackBoxed": s.is_black_boxed,
            })
        })
        .collect();

    let total = results.len();
    let result_json = json!(results);
    let meta = json!({"host": cli.host, "port": cli.port});
    let envelope = output::envelope(&result_json, total, &meta);

    OutputPipeline::from_cli(cli)?
        .finalize(&envelope)
        .map_err(AppError::from)
}

#[cfg(test)]
mod tests {
    /// Verify that a normal pattern compiles successfully under the size limit.
    #[test]
    fn accepts_reasonable_regex() {
        let result = regex::RegexBuilder::new(r"\.js$|\.ts$")
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
