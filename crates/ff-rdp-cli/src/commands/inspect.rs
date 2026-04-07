use std::collections::{BTreeMap, HashSet};

use ff_rdp_core::{ObjectActor, ProtocolError, descriptor_to_json};
use serde_json::{Value, json};

use crate::cli::args::Cli;
use crate::error::AppError;
use crate::output;
use crate::output_pipeline::OutputPipeline;

use super::connect_tab::connect_and_get_target;

pub fn run(cli: &Cli, actor_id: &str, depth: u32) -> Result<(), AppError> {
    let mut ctx = connect_and_get_target(cli)?;
    let result =
        inspect_object(ctx.transport_mut(), actor_id, depth, &mut HashSet::new()).map_err(
            |e| match e {
                // A noSuchActor error means the grip actor has expired.  This
                // commonly happens when using --no-daemon: object grips are
                // scoped to the Firefox connection that created them, so they
                // disappear when the `eval` command's connection closes.
                ProtocolError::ActorError { ref error, .. }
                    if error == "noSuchActor" || error == "unknownActor" =>
                {
                    let hint = if cli.no_daemon {
                        " — re-run `eval` in the same session, or remove --no-daemon so the daemon keeps the connection alive"
                    } else {
                        " — re-run `eval` to get a fresh grip actor"
                    };
                    AppError::User(format!(
                        "grip actor '{actor_id}' is no longer valid{hint}"
                    ))
                }
                other => AppError::from(other),
            },
        )?;

    let meta = json!({"host": cli.host, "port": cli.port, "actor": actor_id});
    let envelope = output::envelope(&result, 1, &meta);
    OutputPipeline::from_cli(cli)?
        .finalize(&envelope)
        .map_err(AppError::from)
}

/// Recursively inspect a remote JS object by its grip actor ID.
///
/// - `depth` controls how many levels of nested Object grips are followed.
/// - `seen` tracks actor IDs already visited to prevent infinite cycles.
fn inspect_object(
    transport: &mut ff_rdp_core::RdpTransport,
    actor_id: &str,
    depth: u32,
    seen: &mut HashSet<String>,
) -> Result<Value, ff_rdp_core::ProtocolError> {
    if !seen.insert(actor_id.to_owned()) {
        // Already visited — emit a back-reference to avoid cycles.
        return Ok(json!({"type": "alreadyVisited", "actor": actor_id}));
    }

    let pap = ObjectActor::prototype_and_properties(transport, actor_id)?;

    // Build the properties map.
    let mut props: BTreeMap<String, Value> = BTreeMap::new();
    for (name, desc) in &pap.own_properties {
        let mut desc_json = descriptor_to_json(desc);

        // If depth allows, recurse into nested objects.
        if depth > 1
            && let Some(value_json) = desc_json.get("value")
            && let Some(nested_actor) = nested_object_actor(value_json)
        {
            let nested = inspect_object(transport, &nested_actor, depth - 1, seen)?;
            desc_json["value"] = nested;
        }

        props.insert(name.clone(), desc_json);
    }

    Ok(json!({
        "actor": actor_id,
        "prototype": pap.prototype.to_json(),
        "ownProperties": props,
    }))
}

/// Extract the actor ID from a JSON value that represents an object grip,
/// returning `None` for any other value.
fn nested_object_actor(value: &Value) -> Option<String> {
    if value.get("type")?.as_str()? == "object" {
        value.get("actor")?.as_str().map(String::from)
    } else {
        None
    }
}
