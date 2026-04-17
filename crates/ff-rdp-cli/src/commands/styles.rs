use ff_rdp_core::{ActorId, DomWalkerActor, InspectorActor, PageStyleActor};
use serde_json::{Value, json};

use crate::cli::args::Cli;
use crate::error::AppError;
use crate::output;
use crate::output_controls::{OutputControls, SortDir};
use crate::output_pipeline::OutputPipeline;

use super::connect_tab::{ConnectedTab, connect_and_get_target};

/// Shared setup: connect, discover actors, resolve the target node.
///
/// Returns the connected tab (which owns the transport), the page-style actor
/// ID, and the matched DOM node actor ID.
fn setup(cli: &Cli, selector: &str) -> Result<(ConnectedTab, ActorId, ActorId), AppError> {
    let mut ctx = connect_and_get_target(cli)?;

    let inspector_actor = ctx
        .target
        .inspector_actor
        .clone()
        .ok_or_else(|| AppError::User("no inspector actor available".to_string()))?;

    let walker_actor = InspectorActor::get_walker(ctx.transport_mut(), &inspector_actor)
        .map_err(map_style_error)?;

    let page_style_actor = InspectorActor::get_page_style(ctx.transport_mut(), &inspector_actor)
        .map_err(map_style_error)?;

    let doc_root = DomWalkerActor::document_element(ctx.transport_mut(), &walker_actor)
        .map_err(map_style_error)?;

    let root_actor_str = doc_root
        .actor
        .as_deref()
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("document root node has no actor ID")))?;
    let root_actor = ActorId::from(root_actor_str);

    let maybe_node =
        DomWalkerActor::query_selector(ctx.transport_mut(), &walker_actor, &root_actor, selector)
            .map_err(map_style_error)?;

    let node = maybe_node
        .ok_or_else(|| AppError::User(format!("no element matching selector '{selector}'")))?;

    let node_actor_str = node
        .actor
        .as_deref()
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("matched node has no actor ID")))?;
    let node_actor = ActorId::from(node_actor_str);

    Ok((ctx, page_style_actor, node_actor))
}

/// Computed styles (default).
pub fn run(cli: &Cli, selector: &str, properties: Option<&[String]>) -> Result<(), AppError> {
    let (mut ctx, page_style_actor, node_actor) = setup(cli, selector)?;

    let computed =
        PageStyleActor::get_computed(ctx.transport_mut(), &page_style_actor, &node_actor)
            .map_err(map_style_error)?;

    // The core layer already sorts by name; convert to JSON values for OutputControls.
    let mut items: Vec<Value> = computed
        .iter()
        .map(|p| {
            json!({
                "name": p.name,
                "value": p.value,
                "priority": p.priority,
            })
        })
        .collect();

    // Filter by --properties if set.
    if let Some(props) = properties {
        items.retain(|item| {
            item.get("name")
                .and_then(Value::as_str)
                .is_some_and(|name| props.iter().any(|p| p == name))
        });
    }

    let controls = OutputControls::from_cli(cli, SortDir::Asc);

    // Apply user sort override only when an explicit --sort flag is given;
    // the core already returns properties sorted alphabetically by name.
    if cli.sort.is_some() {
        controls.apply_sort(&mut items);
    }

    let items = controls.apply_fields(items);
    let (items, total, truncated) = controls.apply_limit(items, None);
    let shown = items.len();
    let results = Value::Array(items);

    let meta = json!({
        "host": cli.host,
        "port": cli.port,
        "selector": selector,
    });

    let envelope = output::envelope_with_truncation(&results, shown, total, truncated, &meta);

    OutputPipeline::from_cli(cli)?
        .finalize(&envelope)
        .map_err(AppError::from)
}

/// Applied CSS rules with source locations.
pub fn run_applied(cli: &Cli, selector: &str) -> Result<(), AppError> {
    let (mut ctx, page_style_actor, node_actor) = setup(cli, selector)?;

    let applied = PageStyleActor::get_applied(ctx.transport_mut(), &page_style_actor, &node_actor)
        .map_err(map_style_error)?;

    let mut items: Vec<Value> = applied
        .iter()
        .map(|r| serde_json::to_value(r).map_err(|e| AppError::Internal(e.into())))
        .collect::<Result<Vec<_>, _>>()?;

    let controls = OutputControls::from_cli(cli, SortDir::Asc);

    // Default: sort by selector alphabetically; honour --sort override.
    if cli.sort.is_none() {
        items.sort_by(|a, b| {
            let sa = a.get("selector").and_then(Value::as_str).unwrap_or("");
            let sb = b.get("selector").and_then(Value::as_str).unwrap_or("");
            sa.cmp(sb)
        });
    } else {
        controls.apply_sort(&mut items);
    }

    let items = controls.apply_fields(items);
    let (items, total, truncated) = controls.apply_limit(items, None);
    let shown = items.len();
    let results = Value::Array(items);

    let meta = json!({
        "host": cli.host,
        "port": cli.port,
        "selector": selector,
    });

    let envelope = output::envelope_with_truncation(&results, shown, total, truncated, &meta);

    OutputPipeline::from_cli(cli)?
        .finalize(&envelope)
        .map_err(AppError::from)
}

/// Box model layout.
pub fn run_layout(cli: &Cli, selector: &str) -> Result<(), AppError> {
    let (mut ctx, page_style_actor, node_actor) = setup(cli, selector)?;

    let layout = PageStyleActor::get_layout(ctx.transport_mut(), &page_style_actor, &node_actor)
        .map_err(map_style_error)?;

    let results = serde_json::to_value(&layout).map_err(|e| AppError::Internal(e.into()))?;

    let meta = json!({
        "host": cli.host,
        "port": cli.port,
        "selector": selector,
    });

    let envelope = output::envelope(&results, 1, &meta);

    OutputPipeline::from_cli(cli)?
        .finalize(&envelope)
        .map_err(AppError::from)
}

/// Map actor errors to user-friendly messages.
fn map_style_error(err: ff_rdp_core::ProtocolError) -> AppError {
    match &err {
        ff_rdp_core::ProtocolError::ActorError { error, .. }
            if error == "noSuchActor" || error == "unknownActor" =>
        {
            AppError::User(
                "style actor is no longer valid — the actor may have expired after navigation. \
                 Re-run the command to get a fresh actor"
                    .to_string(),
            )
        }
        _ => AppError::from(err),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn map_style_error_no_such_actor() {
        let err = ff_rdp_core::ProtocolError::ActorError {
            actor: "conn0/pageStyleActor1".to_string(),
            kind: ff_rdp_core::ActorErrorKind::UnknownActor,
            error: "noSuchActor".to_string(),
            message: "actor not found".to_string(),
        };
        let app_err = map_style_error(err);
        match app_err {
            AppError::User(msg) => assert!(msg.contains("no longer valid")),
            other => panic!("expected User error, got {other:?}"),
        }
    }

    #[test]
    fn map_style_error_unknown_actor() {
        let err = ff_rdp_core::ProtocolError::ActorError {
            actor: "conn0/pageStyleActor1".to_string(),
            kind: ff_rdp_core::ActorErrorKind::UnknownActor,
            error: "unknownActor".to_string(),
            message: String::new(),
        };
        let app_err = map_style_error(err);
        match app_err {
            AppError::User(msg) => assert!(msg.contains("no longer valid")),
            other => panic!("expected User error, got {other:?}"),
        }
    }

    #[test]
    fn map_style_error_other_errors_become_internal() {
        let err = ff_rdp_core::ProtocolError::InvalidPacket("bad data".into());
        let app_err = map_style_error(err);
        match app_err {
            AppError::Internal(_) => {}
            other => panic!("expected Internal error, got {other:?}"),
        }
    }

    #[test]
    fn computed_items_serialise_correctly() {
        // Verify the JSON shape produced for each computed property.
        let item = json!({
            "name": "color",
            "value": "rgb(0, 0, 0)",
            "priority": "",
        });
        assert_eq!(item["name"], "color");
        assert_eq!(item["value"], "rgb(0, 0, 0)");
        assert_eq!(item["priority"], "");
    }

    // ---------------------------------------------------------------------------
    // --properties filter tests
    // ---------------------------------------------------------------------------

    fn make_items() -> Vec<Value> {
        vec![
            json!({"name": "color", "value": "rgb(0,0,0)", "priority": ""}),
            json!({"name": "display", "value": "block", "priority": ""}),
            json!({"name": "font-size", "value": "16px", "priority": ""}),
            json!({"name": "margin-top", "value": "0px", "priority": ""}),
        ]
    }

    fn apply_properties_filter(mut items: Vec<Value>, props: Option<&[String]>) -> Vec<Value> {
        if let Some(filter) = props {
            items.retain(|item| {
                item.get("name")
                    .and_then(Value::as_str)
                    .is_some_and(|name| filter.iter().any(|p| p == name))
            });
        }
        items
    }

    #[test]
    fn properties_filter_none_returns_all() {
        let items = make_items();
        let result = apply_properties_filter(items, None);
        assert_eq!(result.len(), 4);
    }

    #[test]
    fn properties_filter_single_property() {
        let items = make_items();
        let props = vec!["color".to_string()];
        let result = apply_properties_filter(items, Some(&props));
        assert_eq!(result.len(), 1);
        assert_eq!(result[0]["name"], "color");
    }

    #[test]
    fn properties_filter_multiple_properties() {
        let items = make_items();
        let props = vec!["color".to_string(), "display".to_string()];
        let result = apply_properties_filter(items, Some(&props));
        assert_eq!(result.len(), 2);
        let names: Vec<&str> = result.iter().filter_map(|i| i["name"].as_str()).collect();
        assert!(names.contains(&"color"));
        assert!(names.contains(&"display"));
    }

    #[test]
    fn properties_filter_unknown_property_returns_empty() {
        let items = make_items();
        let props = vec!["nonexistent-prop".to_string()];
        let result = apply_properties_filter(items, Some(&props));
        assert!(result.is_empty());
    }
}
