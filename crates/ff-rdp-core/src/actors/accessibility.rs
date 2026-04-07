use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::actor::actor_request;
use crate::error::ProtocolError;
use crate::transport::RdpTransport;
use crate::types::ActorId;

/// A node in the accessibility tree.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccessibleNode {
    /// The accessible's actor ID (for further queries).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub actor: Option<String>,
    /// ARIA role (e.g. "document", "button", "link", "heading").
    pub role: String,
    /// Accessible name (computed from aria-label, content, etc.).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Accessible value (for inputs, sliders, etc.).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    /// Accessible description.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Number of children this node has.
    #[serde(rename = "childCount", skip_serializing_if = "Option::is_none")]
    pub child_count: Option<i64>,
    /// States (e.g. `focusable`, `enabled`, `sensitive`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub states: Vec<String>,
    /// DOM node type info.
    #[serde(rename = "domNodeType", skip_serializing_if = "Option::is_none")]
    pub dom_node_type: Option<u32>,
    /// Index in parent.
    #[serde(rename = "indexInParent", skip_serializing_if = "Option::is_none")]
    pub index_in_parent: Option<i64>,
    /// Child nodes (populated during tree walk).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<AccessibleNode>,
    /// Truncation marker when depth/chars limit reached.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub truncated: Option<String>,
}

/// Operations on the Firefox AccessibilityActor.
pub struct AccessibilityActor;

impl AccessibilityActor {
    /// Call `getWalker` on the accessibility actor to obtain the walker actor ID.
    pub fn get_walker(
        transport: &mut RdpTransport,
        accessibility_actor: &ActorId,
    ) -> Result<ActorId, ProtocolError> {
        let response = actor_request(transport, accessibility_actor.as_ref(), "getWalker", None)?;

        // Response: {"walker": {"actor": "server1.conn0.child2/accessibleWalkerActor1"}, ...}
        let walker_actor = response
            .get("walker")
            .and_then(|w| w.get("actor"))
            .and_then(Value::as_str)
            .ok_or_else(|| {
                ProtocolError::InvalidPacket(
                    "getWalker response missing 'walker.actor' field".into(),
                )
            })?;

        Ok(walker_actor.into())
    }

    /// Get the children of an accessible node via the walker.
    pub fn children(
        transport: &mut RdpTransport,
        walker_actor: &ActorId,
        accessible_actor: &ActorId,
    ) -> Result<Vec<AccessibleNode>, ProtocolError> {
        let response = actor_request(
            transport,
            walker_actor.as_ref(),
            "children",
            Some(&json!({"accessible": accessible_actor.as_ref()})),
        )?;

        Ok(parse_children(&response))
    }

    /// Get the document root accessible via the walker.
    pub fn get_root(
        transport: &mut RdpTransport,
        walker_actor: &ActorId,
    ) -> Result<AccessibleNode, ProtocolError> {
        let response = actor_request(transport, walker_actor.as_ref(), "getRootNode", None)?;

        parse_accessible_node(&response).ok_or_else(|| {
            ProtocolError::InvalidPacket("getRootNode response missing accessible data".into())
        })
    }

    /// Recursively walk the accessibility tree from the root, respecting depth and character limits.
    pub fn walk_tree(
        transport: &mut RdpTransport,
        walker_actor: &ActorId,
        root: &AccessibleNode,
        max_depth: u32,
        max_chars: u32,
    ) -> Result<AccessibleNode, ProtocolError> {
        let mut char_count = 0u32;
        walk_recursive(
            transport,
            walker_actor,
            root,
            0,
            max_depth,
            max_chars,
            &mut char_count,
        )
    }
}

#[allow(clippy::cast_possible_truncation)]
fn str_len_u32(s: &str) -> u32 {
    s.len().min(u32::MAX as usize) as u32
}

fn walk_recursive(
    transport: &mut RdpTransport,
    walker_actor: &ActorId,
    node: &AccessibleNode,
    depth: u32,
    max_depth: u32,
    max_chars: u32,
    char_count: &mut u32,
) -> Result<AccessibleNode, ProtocolError> {
    let mut result = AccessibleNode {
        actor: node.actor.clone(),
        role: node.role.clone(),
        name: node.name.clone(),
        value: node.value.clone(),
        description: node.description.clone(),
        child_count: node.child_count,
        states: node.states.clone(),
        dom_node_type: node.dom_node_type,
        index_in_parent: node.index_in_parent,
        children: Vec::new(),
        truncated: None,
    };

    // Count characters from this node's text content.
    if let Some(ref name) = result.name {
        *char_count = char_count.saturating_add(str_len_u32(name));
    }
    if let Some(ref value) = result.value {
        *char_count = char_count.saturating_add(str_len_u32(value));
    }

    if *char_count >= max_chars {
        result.truncated = Some("max characters reached".to_string());
        return Ok(result);
    }

    if depth >= max_depth {
        let count = node.child_count.unwrap_or(0);
        if count > 0 {
            result.truncated = Some(format!("{count} children not shown"));
        }
        return Ok(result);
    }

    // Get children if this node has any.
    let child_count = node.child_count.unwrap_or(0);
    if child_count > 0
        && let Some(ref actor_id) = node.actor
    {
        let actor = ActorId::from(actor_id.as_str());
        let children = AccessibilityActor::children(transport, walker_actor, &actor)?;

        for child in &children {
            if *char_count >= max_chars {
                result.truncated = Some("max characters reached".to_string());
                break;
            }
            let walked = walk_recursive(
                transport,
                walker_actor,
                child,
                depth + 1,
                max_depth,
                max_chars,
                char_count,
            )?;
            result.children.push(walked);
        }
    }

    Ok(result)
}

/// Parse the children array from a `children` response.
fn parse_children(response: &Value) -> Vec<AccessibleNode> {
    response
        .get("children")
        .and_then(Value::as_array)
        .map(|arr| arr.iter().filter_map(parse_accessible_node).collect())
        .unwrap_or_default()
}

/// Parse a single accessible node from a JSON value.
fn parse_accessible_node(value: &Value) -> Option<AccessibleNode> {
    let role = value.get("role")?.as_str()?.to_string();

    let actor = value.get("actor").and_then(Value::as_str).map(String::from);
    let name = value
        .get("name")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(String::from);
    let value_str = value
        .get("value")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(String::from);
    let description = value
        .get("description")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(String::from);
    let child_count = value.get("childCount").and_then(Value::as_i64);
    let dom_node_type = value
        .get("domNodeType")
        .and_then(Value::as_u64)
        .and_then(|v| u32::try_from(v).ok());
    let index_in_parent = value.get("indexInParent").and_then(Value::as_i64);

    let states = value
        .get("states")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(Value::as_str)
                .map(String::from)
                .collect()
        })
        .unwrap_or_default();

    Some(AccessibleNode {
        actor,
        role,
        name,
        value: value_str,
        description,
        child_count,
        states,
        dom_node_type,
        index_in_parent,
        children: Vec::new(),
        truncated: None,
    })
}

/// Filter a tree to only keep interactive elements (buttons, links, inputs, etc.).
pub fn filter_interactive(node: &AccessibleNode) -> Option<AccessibleNode> {
    const INTERACTIVE_ROLES: &[&str] = &[
        "button",
        "link",
        "textbox",
        "combobox",
        "listbox",
        "option",
        "checkbox",
        "radio",
        "slider",
        "spinbutton",
        "switch",
        "menuitem",
        "menuitemcheckbox",
        "menuitemradio",
        "tab",
        "searchbox",
        "entry",
        "pushbutton",
        "pagetab",
    ];

    let is_interactive = INTERACTIVE_ROLES
        .iter()
        .any(|r| node.role.eq_ignore_ascii_case(r));

    // Recurse into children.
    let filtered_children: Vec<AccessibleNode> = node
        .children
        .iter()
        .filter_map(filter_interactive)
        .collect();

    // Keep this node if it's interactive or has interactive descendants.
    if is_interactive || !filtered_children.is_empty() {
        let mut result = node.clone();
        result.children = filtered_children;
        Some(result)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parse_accessible_node_full() {
        let v = json!({
            "actor": "server1.conn0.child0/accessible1",
            "role": "button",
            "name": "Submit",
            "value": "",
            "description": "Submit the form",
            "childCount": 0,
            "domNodeType": 1,
            "indexInParent": 2,
            "states": ["focusable", "enabled"]
        });
        let node = parse_accessible_node(&v).unwrap();
        assert_eq!(node.role, "button");
        assert_eq!(node.name.as_deref(), Some("Submit"));
        assert!(node.value.is_none()); // empty string filtered out
        assert_eq!(node.description.as_deref(), Some("Submit the form"));
        assert_eq!(node.child_count, Some(0));
        assert_eq!(node.dom_node_type, Some(1));
        assert_eq!(node.index_in_parent, Some(2));
        assert_eq!(node.states, vec!["focusable", "enabled"]);
        assert!(node.children.is_empty());
    }

    #[test]
    fn parse_accessible_node_minimal() {
        let v = json!({"role": "document"});
        let node = parse_accessible_node(&v).unwrap();
        assert_eq!(node.role, "document");
        assert!(node.name.is_none());
        assert!(node.value.is_none());
        assert!(node.actor.is_none());
        assert!(node.states.is_empty());
    }

    #[test]
    fn parse_accessible_node_missing_role_returns_none() {
        let v = json!({"name": "no role"});
        assert!(parse_accessible_node(&v).is_none());
    }

    #[test]
    fn parse_children_from_response() {
        let response = json!({
            "children": [
                {"role": "heading", "name": "Title", "childCount": 0},
                {"role": "paragraph", "name": "Text", "childCount": 0}
            ]
        });
        let children = parse_children(&response);
        assert_eq!(children.len(), 2);
        assert_eq!(children[0].role, "heading");
        assert_eq!(children[1].role, "paragraph");
    }

    #[test]
    fn parse_children_empty_response() {
        let response = json!({"children": []});
        let children = parse_children(&response);
        assert!(children.is_empty());
    }

    #[test]
    fn parse_children_missing_field() {
        let response = json!({});
        let children = parse_children(&response);
        assert!(children.is_empty());
    }

    #[test]
    fn filter_interactive_keeps_buttons() {
        let tree = AccessibleNode {
            actor: None,
            role: "document".to_string(),
            name: Some("Page".to_string()),
            value: None,
            description: None,
            child_count: Some(2),
            states: vec![],
            dom_node_type: None,
            index_in_parent: None,
            children: vec![
                AccessibleNode {
                    actor: None,
                    role: "paragraph".to_string(),
                    name: Some("Just text".to_string()),
                    value: None,
                    description: None,
                    child_count: Some(0),
                    states: vec![],
                    dom_node_type: None,
                    index_in_parent: None,
                    children: vec![],
                    truncated: None,
                },
                AccessibleNode {
                    actor: None,
                    role: "button".to_string(),
                    name: Some("Click me".to_string()),
                    value: None,
                    description: None,
                    child_count: Some(0),
                    states: vec!["focusable".to_string()],
                    dom_node_type: None,
                    index_in_parent: None,
                    children: vec![],
                    truncated: None,
                },
            ],
            truncated: None,
        };

        let filtered = filter_interactive(&tree).unwrap();
        assert_eq!(filtered.role, "document");
        assert_eq!(filtered.children.len(), 1);
        assert_eq!(filtered.children[0].role, "button");
    }

    #[test]
    fn filter_interactive_removes_non_interactive_tree() {
        let tree = AccessibleNode {
            actor: None,
            role: "paragraph".to_string(),
            name: Some("Text".to_string()),
            value: None,
            description: None,
            child_count: Some(0),
            states: vec![],
            dom_node_type: None,
            index_in_parent: None,
            children: vec![],
            truncated: None,
        };
        assert!(filter_interactive(&tree).is_none());
    }

    #[test]
    fn accessible_node_serialization_skips_empty_fields() {
        let node = AccessibleNode {
            actor: None,
            role: "button".to_string(),
            name: Some("OK".to_string()),
            value: None,
            description: None,
            child_count: None,
            states: vec![],
            dom_node_type: None,
            index_in_parent: None,
            children: vec![],
            truncated: None,
        };
        let json = serde_json::to_value(&node).unwrap();
        assert_eq!(json, json!({"role": "button", "name": "OK"}));
    }
}
