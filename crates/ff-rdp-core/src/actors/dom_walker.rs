// allow-actor-kb-skip: iter-74 additions (clear_picker oneway wrapper + unit test) do not change
// the walker actor's protocol surface described in kb/rdp/actors/walker.md.
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::actor::{actor_request, actor_send};
use crate::error::ProtocolError;
use crate::specs::types::resolve_long_string_slot;
use crate::transport::RdpTransport;
use crate::types::ActorId;

/// A DOM attribute (name/value pair).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DomAttr {
    pub name: String,
    pub value: String,
}

/// A DOM node returned by the walker.
///
/// Note: `Deserialize` is intentionally omitted — Firefox sends `attrs` as a flat alternating
/// string array (`["name","val",...]`), not `Vec<DomAttr>`, so standard deserialization would
/// produce incorrect results. Use `parse_dom_node` to construct values from Firefox wire data.
#[derive(Debug, Clone, Serialize)]
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

        parse_dom_node(transport, node_val)?.ok_or_else(|| {
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
                let node = parse_dom_node(transport, node_val)?.ok_or_else(|| {
                    ProtocolError::InvalidPacket("querySelector node data is invalid".into())
                })?;
                Ok(Some(node))
            }
        }
    }

    /// querySelectorAll — find all nodes matching a CSS selector.
    ///
    /// Send: `{"to": walker, "type": "querySelectorAll", "node": root_node_actor, "selector": "script"}`
    /// Response: `{"list": {"actor": "nodeListActor"}, ...}` then we send `items` to get the nodes.
    ///
    /// Returns `ProtocolError::InvalidPacket` if the response is missing the `list.actor` field.
    pub fn query_selector_all(
        transport: &mut RdpTransport,
        walker_actor: &ActorId,
        node_actor: &ActorId,
        selector: &str,
    ) -> Result<Vec<DomNode>, ProtocolError> {
        use crate::actor::actor_request;

        let response = actor_request(
            transport,
            walker_actor.as_ref(),
            "querySelectorAll",
            Some(&json!({
                "node": node_actor.as_ref(),
                "selector": selector
            })),
        )?;

        // Firefox returns {"list": {"actor": "nodeListActor1"}, ...}
        // We then need to call `items` on the nodelist actor to get the nodes.
        let nodelist_actor = response
            .get("list")
            .and_then(|l| l.get("actor"))
            .and_then(Value::as_str);

        let Some(nodelist_actor) = nodelist_actor else {
            return Err(ProtocolError::InvalidPacket(
                "querySelectorAll response missing 'list.actor' field".into(),
            ));
        };

        // `items` returns {"nodes": [...], ...}
        let items_response = actor_request(transport, nodelist_actor, "items", None)?;
        parse_dom_nodes(transport, &items_response, "nodes")
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

        if response.get("nodes").and_then(Value::as_array).is_none() {
            return Err(ProtocolError::InvalidPacket(
                "children response missing 'nodes' array field".into(),
            ));
        }
        parse_dom_nodes(transport, &response, "nodes")
    }

    /// Recursively walk the DOM tree from a root node, respecting depth and character limits.
    ///
    /// The `max_chars` budget counts Unicode characters from `nodeValue` and text node content.
    /// When the character budget is exceeded, a `"max characters reached"` truncation marker
    /// is set on the current node. When the depth limit truncates remaining siblings,
    /// a `"[... N more children]"` marker is used instead.
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

    /// Cancel any in-progress element picker.
    ///
    /// **Oneway** — `clearPicker` is declared `oneway: true` in
    /// `devtools/shared/specs/walker.js:378-381`. Firefox does not send a reply.
    ///
    /// Note: `releaseNode` (`devtools/shared/specs/walker.js:127-133`) is
    /// response-less but **not** marked `oneway` — it remains an
    /// `actor_request` in our implementation per spec intent.
    pub fn clear_picker(
        transport: &mut RdpTransport,
        walker_actor: &ActorId,
    ) -> Result<(), ProtocolError> {
        actor_send(transport, walker_actor.as_ref(), "clearPicker", None)
    }
}

#[allow(clippy::cast_possible_truncation)]
fn str_len_u32(s: &str) -> u32 {
    s.chars().count().min(u32::MAX as usize) as u32
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

/// Parse the array of DOM nodes under `response[field]`, resolving `longstring`
/// grips on each node.  Malformed node entries are skipped; a missing/non-array
/// field yields an empty vec (callers that require the field validate it first).
fn parse_dom_nodes(
    transport: &mut RdpTransport,
    response: &Value,
    field: &str,
) -> Result<Vec<DomNode>, ProtocolError> {
    let mut nodes = Vec::new();
    if let Some(arr) = response.get(field).and_then(Value::as_array) {
        for item in arr {
            if let Some(node) = parse_dom_node(transport, item)? {
                nodes.push(node);
            }
        }
    }
    Ok(nodes)
}

/// Parse a single DOM node from a JSON value, resolving any `longstring` grips.
///
/// Firefox sends attributes as a flat array of alternating name/value strings:
/// `["class", "example", "id", "main"]` → `[{name:"class",value:"example"},{name:"id",value:"main"}]`
///
/// Both the `nodeValue` slot and each attribute *value* slot are declared
/// `longstring` in `devtools/shared/specs/node.js`: values above Firefox's
/// long-string threshold (~10 KB) arrive as `{type:"longString", …}` grips
/// rather than inline strings.  Those slots are resolved through
/// [`resolve_long_string_slot`], which fetches the full content via the
/// long-string actor when needed — so a large `nodeValue` (e.g. a big text
/// node) or a large attribute value (e.g. an inline data URI) is never silently
/// dropped to empty.  Attribute *names* are always short and stay inline.
///
/// Returns `Ok(None)` when the value is not a well-formed node (missing
/// `nodeType`/`nodeName`); returns `Err` only when a long-string fetch fails.
pub(crate) fn parse_dom_node(
    transport: &mut RdpTransport,
    value: &Value,
) -> Result<Option<DomNode>, ProtocolError> {
    let Some(node_type) = value
        .get("nodeType")
        .and_then(Value::as_u64)
        .and_then(|v| u32::try_from(v).ok())
    else {
        return Ok(None);
    };

    let Some(node_name) = value.get("nodeName").and_then(Value::as_str) else {
        return Ok(None);
    };
    let node_name = node_name.to_string();

    let actor = value.get("actor").and_then(Value::as_str).map(String::from);

    // `nodeValue` is a `longstring` slot — resolve grips to full content.
    let node_value = resolve_long_string_slot(transport, value.get("nodeValue"))?;

    // Firefox sends attrs as a flat array: ["name1", "val1", "name2", "val2", ...]
    // Attribute *values* are `longstring` slots and may arrive as grips.
    let mut attrs = Vec::new();
    if let Some(arr) = value.get("attrs").and_then(Value::as_array) {
        for pair in arr.chunks(2) {
            if pair.len() != 2 {
                continue;
            }
            let Some(name) = pair[0].as_str() else {
                continue;
            };
            let Some(val) = resolve_long_string_slot(transport, Some(&pair[1]))? else {
                continue;
            };
            attrs.push(DomAttr {
                name: name.to_string(),
                value: val,
            });
        }
    }

    let num_children = value
        .get("numChildren")
        .and_then(Value::as_u64)
        .and_then(|v| u32::try_from(v).ok());

    Ok(Some(DomNode {
        actor,
        node_type,
        node_name,
        node_value,
        attrs,
        num_children,
        children: Vec::new(),
        truncated: None,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// A transport backed by a loopback TCP pair.  For inline (non-longString)
    /// values `parse_dom_node` never reads or writes the socket, so the pair is
    /// only needed to satisfy the signature.
    fn dummy_transport() -> RdpTransport {
        use std::io::BufReader;
        use std::net::{TcpListener, TcpStream};
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let client = TcpStream::connect(addr).unwrap();
        let (_server, _) = listener.accept().unwrap();
        let writer = client.try_clone().unwrap();
        let reader = BufReader::new(client);
        RdpTransport::from_parts(reader, writer)
    }

    #[test]
    fn parse_dom_node_element_with_attrs() {
        let v = json!({
            "actor": "server1.conn0.child0/node1",
            "nodeType": 1,
            "nodeName": "DIV",
            "attrs": ["class", "container", "id", "main"],
            "numChildren": 3
        });
        let node = parse_dom_node(&mut dummy_transport(), &v).unwrap().unwrap();
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
        let node = parse_dom_node(&mut dummy_transport(), &v).unwrap().unwrap();
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
        assert!(
            parse_dom_node(&mut dummy_transport(), &v)
                .unwrap()
                .is_none()
        );

        // Missing nodeName
        let v2 = json!({"nodeType": 1});
        assert!(
            parse_dom_node(&mut dummy_transport(), &v2)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn parse_dom_node_empty_attrs_array() {
        let v = json!({
            "nodeType": 1,
            "nodeName": "SPAN",
            "attrs": []
        });
        let node = parse_dom_node(&mut dummy_transport(), &v).unwrap().unwrap();
        assert!(node.attrs.is_empty());
    }

    #[test]
    fn parse_dom_node_no_attrs_field() {
        let v = json!({
            "nodeType": 1,
            "nodeName": "P",
            "numChildren": 1
        });
        let node = parse_dom_node(&mut dummy_transport(), &v).unwrap().unwrap();
        assert!(node.attrs.is_empty());
        assert_eq!(node.num_children, Some(1));
    }

    /// Serve a greeting then answer every `substring` request with
    /// `full` (single-chunk), on a fresh loopback listener.  Returns the port
    /// and the server thread's join handle.
    fn spawn_substring_server(
        actor: &'static str,
        full: String,
        expected_requests: usize,
    ) -> (u16, std::thread::JoinHandle<()>) {
        use std::io::{BufReader, Write};
        use std::net::TcpListener;

        use crate::transport::{encode_frame, recv_from};

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let handle = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut writer = stream.try_clone().unwrap();
            let mut reader = BufReader::new(stream);
            let greeting = json!({"from":"root","applicationType":"browser","traits":{}});
            writer
                .write_all(encode_frame(&serde_json::to_string(&greeting).unwrap()).as_bytes())
                .unwrap();
            for _ in 0..expected_requests {
                let req = recv_from(&mut reader).unwrap();
                assert_eq!(req["type"], "substring");
                assert_eq!(req["to"], actor);
                let resp = json!({"from": actor, "substring": full.clone()});
                writer
                    .write_all(encode_frame(&serde_json::to_string(&resp).unwrap()).as_bytes())
                    .unwrap();
            }
        });
        (port, handle)
    }

    /// iter-102 Theme A: a `nodeValue` arriving as a longString grip is
    /// resolved to its full content (previously `.as_str()` dropped it to
    /// `None`).
    #[test]
    fn parse_dom_node_resolves_longstring_node_value() {
        use std::time::Duration;

        let full = "T".repeat(20_000);
        let (port, handle) = spawn_substring_server("conn0/longString1", full.clone(), 1);
        let mut transport =
            RdpTransport::connect("127.0.0.1", port, Duration::from_secs(5)).unwrap();

        let v = json!({
            "actor": "conn0/textNode",
            "nodeType": 3,
            "nodeName": "#text",
            "nodeValue": {
                "type": "longString",
                "actor": "conn0/longString1",
                "length": 20_000,
                "initial": "T".repeat(1024),
            },
            "numChildren": 0
        });
        let node = parse_dom_node(&mut transport, &v).unwrap().unwrap();
        assert_eq!(node.node_value.as_deref().map(str::len), Some(20_000));
        assert_eq!(node.node_value.unwrap(), full);
        handle.join().unwrap();
    }

    /// iter-102 Theme A: a DOM attribute *value* arriving as a longString grip
    /// (e.g. a large inline data URI) is resolved to full content; the name
    /// stays inline.
    #[test]
    fn parse_dom_node_resolves_longstring_attr_value() {
        use std::time::Duration;

        let full = "u".repeat(15_000);
        let (port, handle) = spawn_substring_server("conn0/longString2", full.clone(), 1);
        let mut transport =
            RdpTransport::connect("127.0.0.1", port, Duration::from_secs(5)).unwrap();

        let v = json!({
            "actor": "conn0/imgNode",
            "nodeType": 1,
            "nodeName": "IMG",
            "attrs": [
                "src",
                {
                    "type": "longString",
                    "actor": "conn0/longString2",
                    "length": 15_000,
                    "initial": "u".repeat(1024),
                }
            ],
            "numChildren": 0
        });
        let node = parse_dom_node(&mut transport, &v).unwrap().unwrap();
        assert_eq!(node.attrs.len(), 1);
        assert_eq!(node.attrs[0].name, "src");
        assert_eq!(node.attrs[0].value.len(), 15_000);
        assert_eq!(node.attrs[0].value, full);
        handle.join().unwrap();
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

    /// `clear_picker_sends_oneway_packet`:
    /// Verifies that `DomWalkerActor::clear_picker` sends a `clearPicker` packet
    /// to the walker actor without waiting for a reply (oneway semantics).
    ///
    /// The server side reads the raw frame and asserts the JSON fields; the
    /// client side returns `Ok(())` immediately — no response expected.
    #[test]
    fn clear_picker_sends_oneway_packet() {
        use std::io::BufReader;
        use std::net::{TcpListener, TcpStream};

        use crate::transport::{RdpTransport, recv_from};

        const WALKER: &str = "conn0/walker1";

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();

        // Connect client before accepting so both sides are in memory.
        let client = TcpStream::connect(addr).unwrap();
        let (server_stream, _) = listener.accept().unwrap();

        let writer = client.try_clone().unwrap();
        let reader = BufReader::new(client);
        let mut transport = RdpTransport::from_parts(reader, writer);

        // Server: read exactly one frame then check it; no response sent.
        let server = std::thread::spawn(move || {
            let mut srv_reader = BufReader::new(server_stream);
            recv_from(&mut srv_reader).unwrap()
        });

        let walker_id = ActorId::from(WALKER);
        DomWalkerActor::clear_picker(&mut transport, &walker_id)
            .expect("clear_picker should succeed");

        // Drop the transport so the server's recv_from gets EOF and returns.
        drop(transport);

        let packet = server.join().unwrap();
        assert_eq!(packet["to"], WALKER, "packet must address the walker actor");
        assert_eq!(
            packet["type"], "clearPicker",
            "packet type must be clearPicker"
        );
    }
}
