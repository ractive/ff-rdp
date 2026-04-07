use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::actor::actor_request;
use crate::error::ProtocolError;
use crate::transport::RdpTransport;
use crate::types::ActorId;

/// A DOM attribute (name/value pair).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DomAttr {
    pub name: String,
    pub value: String,
}

/// A DOM node returned by the walker.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DomNode {
    /// The node's actor ID (for further queries).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub actor: Option<String>,
    /// DOM node type (1=Element, 3=Text, 8=Comment, 9=Document, etc.).
    #[serde(rename = "nodeType")]
    pub node_type: u32,
    /// DOM node name (e.g. "HTML", "BODY", "#text").
    #[serde(rename = "nodeName")]
    pub node_name: String,
    /// Node value — populated for text nodes (nodeType 3).
    #[serde(rename = "nodeValue", skip_serializing_if = "Option::is_none")]
    pub node_value: Option<String>,
    /// Element attributes.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attrs: Vec<DomAttr>,
    /// Number of children this node has (as reported by Firefox).
    #[serde(rename = "numChildren", skip_serializing_if = "Option::is_none")]
    pub num_children: Option<u32>,
    /// Child nodes — populated during tree walk.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<DomNode>,
    /// Truncation marker when depth/chars limit was reached.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub truncated: Option<String>,
}

/// Operations on the Firefox DOMWalkerActor.
pub struct DomWalkerActor;

impl DomWalkerActor {
    /// Get the document element node (the `<html>` element).
    ///
    /// Send: `{"to": walker, "type": "documentElement"}`
    /// Response: `{"node": {"actor": "...", "nodeType": 1, "nodeName": "HTML", ...}, ...}`
    pub fn document_element(
        transport: &mut RdpTransport,
        walker_actor: &ActorId,
    ) -> Result<DomNode, ProtocolError> {
        let response = actor_request(transport, walker_actor.as_ref(), "documentElement", None)?;

        let node_val = response.get("node").ok_or_else(|| {
            ProtocolError::InvalidPacket("documentElement response missing 'node' field".into())
        })?;

        parse_dom_node(node_val).ok_or_else(|| {
            ProtocolError::InvalidPacket("documentElement node data is invalid".into())
        })
    }

    /// querySelector — find a single node matching a CSS selector.
    ///
    /// Send: `{"to": walker, "type": "querySelector", "node": root_node_actor, "selector": "h1"}`
    /// Response: same shape as `documentElement`, wrapped in a `"node"` field, or `{}` if not found.
    pub fn query_selector(
        transport: &mut RdpTransport,
        walker_actor: &ActorId,
        node_actor: &ActorId,
        selector: &str,
    ) -> Result<Option<DomNode>, ProtocolError> {
        let response = actor_request(
            transport,
            walker_actor.as_ref(),
            "querySelector",
            Some(&json!({
                "node": node_actor.as_ref(),
                "selector": selector
            })),
        )?;

        // Firefox returns {} (no "node" key) when no element matches.
        match response.get("node") {
            None => Ok(None),
            Some(node_val) => {
                let node = parse_dom_node(node_val).ok_or_else(|| {
                    ProtocolError::InvalidPacket("querySelector node data is invalid".into())
                })?;
                Ok(Some(node))
            }
        }
    }

    /// Get children of a node.
    ///
    /// Send: `{"to": walker, "type": "children", "node": node_actor, "maxNodes": -1}`
    /// Response: `{"nodes": [...], ...}`
    pub fn children(
        transport: &mut RdpTransport,
        walker_actor: &ActorId,
        node_actor: &ActorId,
    ) -> Result<Vec<DomNode>, ProtocolError> {
        let response = actor_request(
            transport,
            walker_actor.as_ref(),
            "children",
            Some(&json!({
                "node": node_actor.as_ref(),
                "maxNodes": -1
            })),
        )?;

        response
            .get("nodes")
            .and_then(Value::as_array)
            .map(|arr| arr.iter().filter_map(parse_dom_node).collect())
            .ok_or_else(|| {
                ProtocolError::InvalidPacket("children response missing 'nodes' array field".into())
            })
    }

    /// Recursively walk the DOM tree from a root node, respecting depth and character limits.
    ///
    /// The `max_chars` budget counts characters from `nodeValue` and text node content.
    /// When a limit is reached, a `"[... N more children]"` truncation marker is set.
    pub fn walk_tree(
        transport: &mut RdpTransport,
        walker_actor: &ActorId,
        root: &DomNode,
        max_depth: u32,
        max_chars: u32,
    ) -> Result<DomNode, ProtocolError> {
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
    node: &DomNode,
    depth: u32,
    max_depth: u32,
    max_chars: u32,
    char_count: &mut u32,
) -> Result<DomNode, ProtocolError> {
    let mut result = DomNode {
        actor: node.actor.clone(),
        node_type: node.node_type,
        node_name: node.node_name.clone(),
        node_value: node.node_value.clone(),
        attrs: node.attrs.clone(),
        num_children: node.num_children,
        children: Vec::new(),
        truncated: None,
    };

    // Count chars from this node's text content.
    if let Some(ref val) = result.node_value {
        *char_count = char_count.saturating_add(str_len_u32(val));
    }

    if *char_count >= max_chars {
        result.truncated = Some("max characters reached".to_string());
        return Ok(result);
    }

    if depth >= max_depth {
        let count = node.num_children.unwrap_or(0);
        if count > 0 {
            result.truncated = Some(format!("[... {count} more children]"));
        }
        return Ok(result);
    }

    let num_children = node.num_children.unwrap_or(0);
    if num_children > 0
        && let Some(ref actor_id) = node.actor
    {
        let actor = ActorId::from(actor_id.as_str());
        let children = DomWalkerActor::children(transport, walker_actor, &actor)?;
        let total = children.len();

        for (walked_count, child) in children.iter().enumerate() {
            if *char_count >= max_chars {
                let remaining = total - walked_count;
                result.truncated = Some(format!("[... {remaining} more children]"));
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

/// Parse a single DOM node from a JSON value.
///
/// Firefox sends attributes as a flat array of alternating name/value strings:
/// `["class", "example", "id", "main"]` → `[{name:"class",value:"example"},{name:"id",value:"main"}]`
pub fn parse_dom_node(value: &Value) -> Option<DomNode> {
    let node_type = value
        .get("nodeType")
        .and_then(Value::as_u64)
        .and_then(|v| u32::try_from(v).ok())?;

    let node_name = value.get("nodeName")?.as_str()?.to_string();

    let actor = value.get("actor").and_then(Value::as_str).map(String::from);

    let node_value = value
        .get("nodeValue")
        .and_then(Value::as_str)
        .map(String::from);

    // Firefox sends attrs as a flat array: ["name1", "val1", "name2", "val2", ...]
    let attrs = value
        .get("attrs")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.chunks(2)
                .filter_map(|pair| {
                    if pair.len() == 2 {
                        let name = pair[0].as_str()?.to_string();
                        let val = pair[1].as_str()?.to_string();
                        Some(DomAttr { name, value: val })
                    } else {
                        None
                    }
                })
                .collect()
        })
        .unwrap_or_default();

    let num_children = value
        .get("numChildren")
        .and_then(Value::as_u64)
        .and_then(|v| u32::try_from(v).ok());

    Some(DomNode {
        actor,
        node_type,
        node_name,
        node_value,
        attrs,
        num_children,
        children: Vec::new(),
        truncated: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parse_dom_node_element_with_attrs() {
        let v = json!({
            "actor": "server1.conn0.child0/node1",
            "nodeType": 1,
            "nodeName": "DIV",
            "attrs": ["class", "container", "id", "main"],
            "numChildren": 3
        });
        let node = parse_dom_node(&v).unwrap();
        assert_eq!(node.node_type, 1);
        assert_eq!(node.node_name, "DIV");
        assert_eq!(node.actor.as_deref(), Some("server1.conn0.child0/node1"));
        assert_eq!(node.attrs.len(), 2);
        assert_eq!(node.attrs[0].name, "class");
        assert_eq!(node.attrs[0].value, "container");
        assert_eq!(node.attrs[1].name, "id");
        assert_eq!(node.attrs[1].value, "main");
        assert_eq!(node.num_children, Some(3));
        assert!(node.node_value.is_none());
    }

    #[test]
    fn parse_dom_node_text_node() {
        let v = json!({
            "actor": "server1.conn0.child0/node2",
            "nodeType": 3,
            "nodeName": "#text",
            "nodeValue": "Hello, world!",
            "numChildren": 0
        });
        let node = parse_dom_node(&v).unwrap();
        assert_eq!(node.node_type, 3);
        assert_eq!(node.node_name, "#text");
        assert_eq!(node.node_value.as_deref(), Some("Hello, world!"));
        assert!(node.attrs.is_empty());
        assert_eq!(node.num_children, Some(0));
    }

    #[test]
    fn parse_dom_node_missing_required_fields_returns_none() {
        // Missing nodeType
        let v = json!({"nodeName": "DIV"});
        assert!(parse_dom_node(&v).is_none());

        // Missing nodeName
        let v2 = json!({"nodeType": 1});
        assert!(parse_dom_node(&v2).is_none());
    }

    #[test]
    fn parse_dom_node_empty_attrs_array() {
        let v = json!({
            "nodeType": 1,
            "nodeName": "SPAN",
            "attrs": []
        });
        let node = parse_dom_node(&v).unwrap();
        assert!(node.attrs.is_empty());
    }

    #[test]
    fn parse_dom_node_no_attrs_field() {
        let v = json!({
            "nodeType": 1,
            "nodeName": "P",
            "numChildren": 1
        });
        let node = parse_dom_node(&v).unwrap();
        assert!(node.attrs.is_empty());
        assert_eq!(node.num_children, Some(1));
    }

    #[test]
    fn dom_node_serialization_skips_empty_optional_fields() {
        let node = DomNode {
            actor: None,
            node_type: 1,
            node_name: "DIV".to_string(),
            node_value: None,
            attrs: vec![],
            num_children: None,
            children: vec![],
            truncated: None,
        };
        let v = serde_json::to_value(&node).unwrap();
        // Only nodeType and nodeName should appear
        assert_eq!(v["nodeType"], 1);
        assert_eq!(v["nodeName"], "DIV");
        assert!(v.get("actor").is_none());
        assert!(v.get("nodeValue").is_none());
        assert!(v.get("numChildren").is_none());
        assert!(v.get("truncated").is_none());
        // attrs and children are empty vecs — they should be skipped
        assert!(v.get("attrs").is_none());
        assert!(v.get("children").is_none());
    }

    #[test]
    fn walk_tree_via_tcp_char_budget_applied_to_text_nodes() {
        use std::io::BufReader;
        use std::net::{TcpListener, TcpStream};

        use crate::transport::RdpTransport;

        // A leaf text node with 50 chars of content — no children, no transport call needed.
        let leaf = DomNode {
            actor: Some("conn0/leaf".to_string()),
            node_type: 3,
            node_name: "#text".to_string(),
            node_value: Some("A".repeat(50)),
            attrs: vec![],
            num_children: Some(0), // leaf — walk_recursive won't call transport
            children: vec![],
            truncated: None,
        };

        // Set up a dummy TCP pair so we can construct a transport (even though it won't be used).
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let client = TcpStream::connect(addr).unwrap();
        let (_server_stream, _) = listener.accept().unwrap();
        let writer = client.try_clone().unwrap();
        let reader = BufReader::new(client);
        let mut transport = RdpTransport::from_parts(reader, writer);
        let walker_id = ActorId::from("walker");

        // Walk the first leaf: its nodeValue (50 chars) exceeds max_chars=30.
        // walk_recursive counts the chars, then immediately sets truncated on this node.
        let mut char_count = 0u32;
        let max_chars = 30u32;
        let result_a = walk_recursive(
            &mut transport,
            &walker_id,
            &leaf,
            0,
            10,
            max_chars,
            &mut char_count,
        )
        .unwrap();
        assert!(char_count >= max_chars);
        // The char budget was exceeded while processing this node — it sets truncated.
        assert_eq!(
            result_a.truncated.as_deref(),
            Some("max characters reached")
        );

        // Second call: char_count is still >= max_chars, truncated again immediately.
        let result_b = walk_recursive(
            &mut transport,
            &walker_id,
            &leaf,
            0,
            10,
            max_chars,
            &mut char_count,
        )
        .unwrap();
        assert_eq!(
            result_b.truncated.as_deref(),
            Some("max characters reached")
        );
    }

    #[test]
    fn walk_tree_depth_limit_sets_truncation_marker() {
        use std::io::BufReader;
        use std::net::{TcpListener, TcpStream};

        use crate::transport::RdpTransport;

        // A node that has children but we cap depth at 0 — it should set the truncation marker.
        let node_with_children = DomNode {
            actor: Some("conn0/parent".to_string()),
            node_type: 1,
            node_name: "DIV".to_string(),
            node_value: None,
            attrs: vec![],
            num_children: Some(5),
            children: vec![],
            truncated: None,
        };

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let client = TcpStream::connect(addr).unwrap();
        let (_server_stream, _) = listener.accept().unwrap();
        let writer = client.try_clone().unwrap();
        let reader = BufReader::new(client);
        let mut transport = RdpTransport::from_parts(reader, writer);
        let walker_id = ActorId::from("walker");

        let mut char_count = 0u32;
        // depth == max_depth (both 0) — depth limit fires, no transport call.
        let result = walk_recursive(
            &mut transport,
            &walker_id,
            &node_with_children,
            0,
            0, // max_depth = 0 — hit immediately
            10_000,
            &mut char_count,
        )
        .unwrap();

        assert_eq!(result.truncated.as_deref(), Some("[... 5 more children]"));
    }

    #[test]
    fn str_len_u32_does_not_overflow() {
        assert_eq!(str_len_u32("hello"), 5);
        assert_eq!(str_len_u32(""), 0);
    }
}
