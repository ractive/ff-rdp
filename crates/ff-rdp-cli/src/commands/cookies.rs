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
            let mut obj = json!({
                "name": c.name,
                "value": c.value,
                "host": c.host,
                "path": c.path,
                "size": c.size,
                "isHttpOnly": c.is_http_only,
                "isSecure": c.is_secure,
                "sameSite": c.same_site,
                "hostOnly": c.host_only,
            });
            if c.expires > 0 {
                obj["expires"] = json!(c.expires);
            } else {
                obj["expires"] = json!("Session");
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
