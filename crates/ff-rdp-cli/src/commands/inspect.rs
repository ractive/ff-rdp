use std::collections::{BTreeMap, HashSet};

use ff_rdp_core::{Grip, ObjectActor, ProtocolError, descriptor_to_json};
use serde_json::{Value, json};

use crate::cli::args::Cli;
use crate::error::AppError;
use crate::hints::{HintContext, HintSource};
use crate::output;
use crate::output_pipeline::OutputPipeline;

use super::connect_tab::connect_and_get_target;
use super::js_helpers::eval_or_bail;

/// Evaluate `expression` in the page and inspect the resulting object on the
/// same connection.
///
/// Object grips are actors scoped to the connection that created them, so the
/// evaluation and the walk must share one connection: an actor ID printed by
/// an earlier `eval` is meaningless to this command's fresh connection.
pub fn run(cli: &Cli, expression: &str, depth: u32) -> Result<(), AppError> {
    let mut ctx = connect_and_get_target(cli)?;
    let console_actor = ctx.target().console_actor.clone();
    let evaluated = eval_or_bail(
        &mut ctx,
        &console_actor,
        expression,
        "inspect: evaluation failed",
    )?;

    let result = match &evaluated.result {
        Grip::Object { actor, .. } => {
            inspect_object(
                ctx.transport_mut(),
                actor.as_ref(),
                depth,
                &mut HashSet::new(),
            )
            .map_err(|e| match e {
                // The grip was released (or its document replaced) between
                // the evaluation and the walk.
                ProtocolError::ActorError { ref error, .. }
                    if error == "noSuchActor" || error == "unknownActor" =>
                {
                    AppError::User(format!(
                        "the object {expression:?} evaluated to went away before it could be \
                             inspected — re-run the command"
                    ))
                }
                other => AppError::from(other),
            })?
        }
        // A primitive has nothing to walk: report the value itself.
        other => other.to_json(),
    };

    let mut meta = json!({"expression": expression});
    crate::connection_meta::merge_into_if_verbose(
        &mut meta,
        &cli.host,
        cli.port,
        None,
        cli.is_verbose(),
    );
    let envelope = output::envelope(&result, 1, &meta);
    let hint_ctx = HintContext::new(HintSource::Inspect);
    OutputPipeline::from_cli(cli)?.finalize_with_hints(&envelope, Some(&hint_ctx))
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
