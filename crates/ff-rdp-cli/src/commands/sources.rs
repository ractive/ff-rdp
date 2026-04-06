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
        .as_ref()
        .ok_or_else(|| AppError::User("target does not expose a thread actor".into()))?
        .as_ref()
        .to_owned();

    let sources =
        ThreadActor::list_sources(ctx.transport_mut(), &thread_actor).map_err(AppError::from)?;

    // Apply filters.
    let regex = pattern
        .map(|p| {
            regex::Regex::new(p)
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

    OutputPipeline::new(cli.jq.clone())
        .finalize(&envelope)
        .map_err(AppError::from)
}
