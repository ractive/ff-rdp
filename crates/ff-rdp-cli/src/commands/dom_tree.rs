use ff_rdp_core::{DomWalkerActor, InspectorActor};
use serde_json::{Value, json};

use crate::cli::args::Cli;
use crate::error::AppError;
use crate::output;
use crate::output_pipeline::OutputPipeline;

use super::connect_tab::connect_and_get_target;

pub fn run(cli: &Cli, selector: Option<&str>, depth: u32, max_chars: u32) -> Result<(), AppError> {
    let mut ctx = connect_and_get_target(cli)?;

    let inspector_actor = ctx
        .target
        .inspector_actor
        .clone()
        .ok_or_else(|| AppError::User("no inspector actor available".to_string()))?;

    let walker =
        InspectorActor::get_walker(ctx.transport_mut(), &inspector_actor).map_err(map_dom_error)?;

    let root =
        DomWalkerActor::document_element(ctx.transport_mut(), &walker).map_err(map_dom_error)?;

    let target_node = if let Some(sel) = selector {
        let node_actor = root.actor.as_deref().ok_or_else(|| {
            AppError::User(
                "document element has no actor ID — cannot run querySelector".to_string(),
            )
        })?;
        let node_actor_id = node_actor.into();

        DomWalkerActor::query_selector(ctx.transport_mut(), &walker, &node_actor_id, sel)
            .map_err(map_dom_error)?
            .ok_or_else(|| AppError::User(format!("no element matching selector '{sel}'")))?
    } else {
        root
    };

    let tree =
        DomWalkerActor::walk_tree(ctx.transport_mut(), &walker, &target_node, depth, max_chars)
            .map_err(map_dom_error)?;

    let mut results = serde_json::to_value(&tree).map_err(|e| AppError::Internal(e.into()))?;
    strip_actor_ids(&mut results);

    let meta = if let Some(sel) = selector {
        json!({
            "host": cli.host,
            "port": cli.port,
            "depth": depth,
            "max_chars": max_chars,
            "selector": sel,
        })
    } else {
        json!({
            "host": cli.host,
            "port": cli.port,
            "depth": depth,
            "max_chars": max_chars,
        })
    };

    // Text short-circuit: render indented tree instead of JSON.
    if cli.format == "text" && cli.jq.is_none() {
        render_dom_tree_text(&results);
        return Ok(());
    }

    let envelope = output::envelope(&results, 1, &meta);

    OutputPipeline::from_cli(cli)?
        .finalize(&envelope)
        .map_err(AppError::from)
}

/// Render DOM tree as indented text to stdout.
fn render_dom_tree_text(node: &Value) {
    if node.is_null() {
        println!("(empty tree)");
        return;
    }
    render_dom_node(node, 0);
}

fn render_dom_node(node: &Value, depth: usize) {
    use std::fmt::Write as _;
    let indent = "  ".repeat(depth);

    let node_name = node.get("nodeName").and_then(Value::as_str).unwrap_or("?");
    let node_type = node.get("nodeType").and_then(Value::as_u64).unwrap_or(0);

    match node_type {
        3 => {
            // Text node
            if let Some(text) = node.get("nodeValue").and_then(Value::as_str) {
                let text = text.trim();
                if !text.is_empty() {
                    if text.len() > 80 {
                        let truncated: String = text.chars().take(77).collect();
                        println!("{indent}\"{truncated}...\"");
                    } else {
                        println!("{indent}\"{text}\"");
                    }
                }
            }
        }
        1 => {
            // Element node
            let tag = node_name.to_lowercase();
            let mut line = format!("{indent}<{tag}");

            // Show key attributes
            if let Some(attrs) = node.get("attrs").and_then(Value::as_array) {
                for attr in attrs {
                    if let (Some(name), Some(value)) = (
                        attr.get("name").and_then(Value::as_str),
                        attr.get("value").and_then(Value::as_str),
                    ) && matches!(
                        name,
                        "id" | "class" | "href" | "src" | "type" | "name" | "role" | "aria-label"
                    ) {
                        let val = if value.len() > 40 {
                            format!("{}...", &value[..37])
                        } else {
                            value.to_string()
                        };
                        let _ = write!(line, " {name}={val:?}");
                    }
                }
            }

            if let Some(truncated) = node.get("truncated").and_then(Value::as_str) {
                let _ = write!(line, " ({truncated})");
            }

            println!("{line}");

            if let Some(children) = node.get("children").and_then(Value::as_array) {
                for child in children {
                    render_dom_node(child, depth + 1);
                }
            }
        }
        _ => {
            // Other node types (comment, document, etc.)
            println!("{indent}{node_name}");
            if let Some(children) = node.get("children").and_then(Value::as_array) {
                for child in children {
                    render_dom_node(child, depth + 1);
                }
            }
        }
    }
}

/// Map protocol errors to user-friendly messages, especially unknownActor / noSuchActor.
fn map_dom_error(err: ff_rdp_core::ProtocolError) -> AppError {
    match &err {
        ff_rdp_core::ProtocolError::ActorError { error, .. }
            if error == "noSuchActor" || error == "unknownActor" =>
        {
            AppError::User(
                "DOM walker actor is no longer valid \
                 — the DOM walker actor may have expired after navigation. Re-run the command"
                    .to_string(),
            )
        }
        _ => AppError::from(err),
    }
}

/// Strip actor IDs from the output JSON (internal detail not useful to users).
fn strip_actor_ids(value: &mut Value) {
    match value {
        Value::Object(map) => {
            map.remove("actor");
            for v in map.values_mut() {
                strip_actor_ids(v);
            }
        }
        Value::Array(arr) => {
            for v in arr {
                strip_actor_ids(v);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn render_dom_tree_text_does_not_panic_null() {
        render_dom_tree_text(&Value::Null);
    }

    #[test]
    fn render_dom_tree_text_element_with_children() {
        let tree = json!({
            "nodeType": 1,
            "nodeName": "HTML",
            "attrs": [{"name": "lang", "value": "en"}],
            "children": [
                {"nodeType": 1, "nodeName": "BODY", "children": [
                    {"nodeType": 3, "nodeName": "#text", "nodeValue": "Hello World"}
                ]}
            ]
        });
        render_dom_tree_text(&tree);
    }

    #[test]
    fn render_dom_tree_text_with_truncation() {
        let tree = json!({
            "nodeType": 1,
            "nodeName": "DIV",
            "truncated": "5 children not shown"
        });
        render_dom_tree_text(&tree);
    }

    #[test]
    fn strip_actor_ids_removes_actor_field() {
        let mut val = json!({
            "actor": "conn1/domwalker1",
            "nodeType": 1,
            "nodeName": "HTML",
            "children": [
                {"actor": "conn1/node2", "nodeType": 1, "nodeName": "BODY"}
            ]
        });
        strip_actor_ids(&mut val);
        assert!(val.get("actor").is_none());
        assert!(val["children"][0].get("actor").is_none());
        assert_eq!(val["children"][0]["nodeName"], "BODY");
    }

    #[test]
    fn strip_actor_ids_leaves_other_fields_intact() {
        let mut val = json!({
            "actor": "conn1/node1",
            "nodeType": 3,
            "nodeName": "#text",
            "nodeValue": "Hello"
        });
        strip_actor_ids(&mut val);
        assert!(val.get("actor").is_none());
        assert_eq!(val["nodeType"], 3);
        assert_eq!(val["nodeName"], "#text");
        assert_eq!(val["nodeValue"], "Hello");
    }

    #[test]
    fn strip_actor_ids_handles_no_actor_field() {
        let mut val = json!({"nodeType": 1, "nodeName": "DIV"});
        strip_actor_ids(&mut val);
        assert!(val.get("actor").is_none());
        assert_eq!(val["nodeName"], "DIV");
    }

    #[test]
    fn strip_actor_ids_handles_nested_arrays() {
        let mut val = json!({
            "actor": "root",
            "children": [
                {"actor": "child1", "nodeName": "P"},
                {"actor": "child2", "nodeName": "SPAN", "children": [
                    {"actor": "grandchild", "nodeName": "#text"}
                ]}
            ]
        });
        strip_actor_ids(&mut val);
        assert!(val.get("actor").is_none());
        assert!(val["children"][0].get("actor").is_none());
        assert!(val["children"][1].get("actor").is_none());
        assert!(val["children"][1]["children"][0].get("actor").is_none());
    }
}
