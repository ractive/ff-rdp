use ff_rdp_core::StorageActor;
use serde_json::{Value, json};

use crate::cli::args::Cli;
use crate::error::AppError;
use crate::output;
use crate::output_pipeline::OutputPipeline;

use super::connect_tab::connect_and_get_target;

pub fn run(cli: &Cli, name: Option<&str>) -> Result<(), AppError> {
    let mut ctx = connect_and_get_target(cli)?;
    let tab_actor = ctx.target_tab_actor().clone();

    let cookies =
        StorageActor::list_cookies(ctx.transport_mut(), &tab_actor).map_err(AppError::from)?;

    let mut results: Vec<Value> = cookies
        .iter()
        .map(|c| {
            let mut obj = serde_json::to_value(c).unwrap_or_default();
            // Replace numeric expires=0 with human-readable "Session".
            if c.expires == 0 {
                obj["expires"] = json!("Session");
            }
            // Drop internal-only fields that aren't useful for CLI output.
            if let Some(o) = obj.as_object_mut() {
                o.remove("lastAccessed");
                o.remove("creationTime");
            }
            obj
        })
        .collect();

    // Filter by cookie name if requested.
    if let Some(filter_name) = name {
        results.retain(|c| c.get("name").and_then(Value::as_str) == Some(filter_name));
    }

    let total = results.len();
    let result_json = json!(results);
    let meta = json!({"host": cli.host, "port": cli.port});
    let envelope = output::envelope(&result_json, total, &meta);

    OutputPipeline::new(cli.jq.clone())
        .finalize(&envelope)
        .map_err(AppError::from)
}
