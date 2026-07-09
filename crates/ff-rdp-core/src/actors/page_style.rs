use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::actor::actor_request;
use crate::error::ProtocolError;
use crate::specs::types::resolve_long_string_slot;
use crate::transport::RdpTransport;
use crate::types::ActorId;

/// A computed CSS property (name, value, priority).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComputedProperty {
    pub name: String,
    pub value: String,
    pub priority: String,
}

/// A property declaration within an applied CSS rule.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuleProperty {
    pub name: String,
    pub value: String,
    pub priority: String,
}

/// An applied CSS rule with its source location and declarations.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppliedRule {
    /// Selector text (multiple selectors joined by ", ").
    pub selector: String,
    /// Stylesheet href (None for inline styles).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    /// Line number in the stylesheet.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
    /// Column number in the stylesheet.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub column: Option<u32>,
    /// CSS declarations in this rule.
    pub properties: Vec<RuleProperty>,
    /// The subset of the rule's selectors that actually matched the node.
    ///
    /// Populated by `getApplied` when `matchedSelectors: true` is sent.
    /// Empty when Firefox omits the field (older versions or non-matching mode).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub matched_selectors: Vec<String>,
    /// Media query text(s) wrapping the rule (e.g. `"(max-width: 600px)"`).
    ///
    /// Empty when the rule is not inside an `@media` block.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub media: Vec<String>,
    /// The stable RDP actor ID for this rule (e.g. `"conn0/styleRuleActor1"`).
    ///
    /// Used by `styles --applied` to deduplicate entries: Firefox sometimes
    /// sends the same rule multiple times when multiple stylesheets share the
    /// same compiled rule object.  Keying on `rule_actor_id` (rather than on
    /// `(selector, property)` pairs) is the correct deduplication level.
    ///
    /// `None` when Firefox omits the `actor` field (unusual but safe to ignore).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rule_actor_id: Option<ActorId>,
}

/// The four sides of a CSS box model dimension.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BoxSides {
    pub top: f64,
    pub right: f64,
    pub bottom: f64,
    pub left: f64,
}

/// Box model layout data for a DOM node.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BoxModelLayout {
    pub width: f64,
    pub height: f64,
    pub margin: BoxSides,
    pub border: BoxSides,
    pub padding: BoxSides,
    #[serde(rename = "boxSizing")]
    pub box_sizing: String,
    pub position: String,
    pub display: String,
    /// Auto-margin info from Firefox (e.g. `{"top": true}`).
    #[serde(rename = "autoMargins", skip_serializing_if = "Option::is_none")]
    pub auto_margins: Option<serde_json::Value>,
}

/// Operations on the Firefox PageStyleActor.
pub struct PageStyleActor;

impl PageStyleActor {
    /// Get computed styles for a node.
    ///
    /// Send: `{"to": pagestyle_actor, "type": "getComputed", "node": node_actor, "markMatched": true, "filter": "user"}`
    /// Response: `{"computed": {"color": {"value": "rgb(0,0,0)", "priority": ""}, ...}, ...}`
    pub fn get_computed(
        transport: &mut RdpTransport,
        page_style_actor: &ActorId,
        node_actor: &ActorId,
    ) -> Result<Vec<ComputedProperty>, ProtocolError> {
        let response = actor_request(
            transport,
            page_style_actor.as_ref(),
            "getComputed",
            Some(&json!({
                "node": node_actor.as_ref(),
                "markMatched": true,
                "filter": "user"
            })),
        )?;

        parse_computed_properties(transport, &response)
    }

    /// Get the raw `getApplied` reply from Firefox as an uninterpreted JSON value.
    ///
    /// This is the diagnostic counterpart to [`Self::get_applied`]: it sends the
    /// same request but returns the full server response before any field-name
    /// mapping occurs.  Useful for debugging protocol drift — e.g. when
    /// `--debug-raw` is passed to `ff-rdp cascade`.
    ///
    /// Send: `{"to": pagestyle_actor, "type": "getApplied", "node": node_actor, "inherited": false, "matchedSelectors": true, "filter": "user"}`
    /// Response: raw `serde_json::Value` of the entire reply packet.
    pub fn get_applied_raw(
        transport: &mut RdpTransport,
        page_style_actor: &ActorId,
        node_actor: &ActorId,
    ) -> Result<Value, ProtocolError> {
        actor_request(
            transport,
            page_style_actor.as_ref(),
            "getApplied",
            Some(&json!({
                "node": node_actor.as_ref(),
                "inherited": false,
                "matchedSelectors": true,
                "filter": "user"
            })),
        )
    }

    /// Get applied CSS rules for a node.
    ///
    /// Sends `inherited: false`, so inherited CSS rules from ancestor elements are excluded;
    /// only rules that directly target this node are returned.
    ///
    /// Send: `{"to": pagestyle_actor, "type": "getApplied", "node": node_actor, "inherited": false, "matchedSelectors": true, "filter": "user"}`
    /// Response: `{"entries": [{"rule": {...}, "declarations": [...]}, ...], ...}`
    pub fn get_applied(
        transport: &mut RdpTransport,
        page_style_actor: &ActorId,
        node_actor: &ActorId,
    ) -> Result<Vec<AppliedRule>, ProtocolError> {
        let response = actor_request(
            transport,
            page_style_actor.as_ref(),
            "getApplied",
            Some(&json!({
                "node": node_actor.as_ref(),
                "inherited": false,
                "matchedSelectors": true,
                "filter": "user"
            })),
        )?;

        let entries = response
            .get("entries")
            .and_then(Value::as_array)
            .ok_or_else(|| {
                ProtocolError::InvalidPacket("getApplied response missing 'entries' array".into())
            })?;

        Ok(entries.iter().filter_map(parse_applied_entry).collect())
    }

    /// Get the box model layout for a node.
    ///
    /// Send: `{"to": pagestyle_actor, "type": "getLayout", "node": node_actor, "autoMargins": true}`
    /// Response: `{"width": N, "height": N, "margin-top": "N", "border-top-width": "N", ..., "box-sizing": "content-box", ...}`
    pub fn get_layout(
        transport: &mut RdpTransport,
        page_style_actor: &ActorId,
        node_actor: &ActorId,
    ) -> Result<BoxModelLayout, ProtocolError> {
        let response = actor_request(
            transport,
            page_style_actor.as_ref(),
            "getLayout",
            Some(&json!({
                "node": node_actor.as_ref(),
                "autoMargins": true
            })),
        )?;

        parse_box_model_layout(&response)
    }
}

/// Parse the `computed` object from a `getComputed` response, resolving any
/// `longstring` grip in each property's `value` slot.
///
/// The `computed` object has CSS property names as keys, each mapping to
/// `{"value": "...", "priority": ""}`.  The `value` slot is declared
/// `longstring` in `devtools/shared/specs/style/style-types.js`: long values
/// (e.g. a large CSS custom property or a `background-image` with a big inline
/// data URI) arrive as `{type:"longString", …}` grips.  A bare `.as_str()`
/// dropped those to nothing; [`resolve_long_string_slot`] fetches the full
/// value.  Entries whose `value` slot is absent or `null` are skipped.
fn parse_computed_properties(
    transport: &mut RdpTransport,
    response: &Value,
) -> Result<Vec<ComputedProperty>, ProtocolError> {
    let computed = response.get("computed").ok_or_else(|| {
        ProtocolError::InvalidPacket("getComputed response missing 'computed' field".into())
    })?;
    let obj = computed.as_object().ok_or_else(|| {
        ProtocolError::InvalidPacket("'computed' field is not a JSON object".into())
    })?;

    let mut props: Vec<ComputedProperty> = Vec::with_capacity(obj.len());
    for (name, entry) in obj {
        let Some(value) = resolve_long_string_slot(transport, entry.get("value"))? else {
            continue;
        };
        let priority = entry
            .get("priority")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        props.push(ComputedProperty {
            name: name.clone(),
            value,
            priority,
        });
    }

    // Sort for stable output ordering.
    props.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(props)
}

/// Parse a single entry from the `entries` array of a `getApplied` response.
///
/// Accepts an entry as a CSS author rule when ANY of these hold (iter-88
/// predicate; previous single-field discriminators all failed against real
/// FF 151 replies — see dogfooding-session-58):
///   (a) `rule.type` is absent (some Firefox replies for external stylesheets)
///   (b) `rule.type == 1` (CSSOM `STYLE_RULE` numeric constant — older FF)
///   (c) `rule.className == "CSSStyleRule"` (string sentinel — FF 151 author rules)
///   (d) `matchedSelectorIndexes` is a non-empty array (legacy discriminator)
///
/// `rule.type == 100` alone is NOT sufficient: the FF 151 reply uses `100`
/// as an element-style sentinel (with a numeric `className: 100` and no
/// selectors). Only accept `type == 100` when paired with the string
/// `className == "CSSStyleRule"`. Inline style declarations
/// (`rule.type == 0`) satisfy none of (a)–(c) and lack a non-empty
/// `matchedSelectorIndexes`, so they are excluded.
fn parse_applied_entry(entry: &Value) -> Option<AppliedRule> {
    let rule = entry.get("rule")?;

    let rule_type = rule.get("type");
    let class_name = rule.get("className").and_then(Value::as_str);
    let has_matched = entry
        .get("matchedSelectorIndexes")
        .and_then(Value::as_array)
        .is_some_and(|arr| !arr.is_empty());

    let type_absent = rule_type.is_none();
    let class_is_style_rule = class_name == Some("CSSStyleRule");
    // `type == 100` alone is the FF 151 element-style sentinel (numeric
    // className, no selectors); require the string `CSSStyleRule` className
    // to disambiguate.
    let type_is_style_rule = rule_type
        .and_then(Value::as_u64)
        .is_some_and(|t| t == 1 || (t == 100 && class_is_style_rule));

    if !(type_absent || type_is_style_rule || class_is_style_rule || has_matched) {
        return None;
    }

    let selectors: Vec<&str> = rule
        .get("selectors")
        .and_then(Value::as_array)
        .map(|arr| arr.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    let selector = selectors.join(", ");

    let source = rule
        .get("href")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(String::from);

    let line = rule
        .get("line")
        .and_then(Value::as_u64)
        .and_then(|v| u32::try_from(v).ok());

    let column = rule
        .get("column")
        .and_then(Value::as_u64)
        .and_then(|v| u32::try_from(v).ok());

    // FF 151 nests declarations under `rule.declarations`; older replies put
    // them at the entry top level.  Try entry first for backwards-compat,
    // then fall back to the FF 151 location.
    let properties: Vec<RuleProperty> = entry
        .get("declarations")
        .or_else(|| rule.get("declarations"))
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|decl| {
                    let name = decl.get("name")?.as_str()?.to_string();
                    let value = decl.get("value")?.as_str()?.to_string();
                    let priority = decl
                        .get("priority")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string();
                    Some(RuleProperty {
                        name,
                        value,
                        priority,
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    // Resolve `matchedSelectors` from the canonical spec shape:
    // `appliedstyle.matchedSelectorIndexes: nullable:array:number` indexes
    // into the rule's `selectors` array (see
    // devtools/shared/specs/style/style-types.js).  Older devtools clients
    // resolved this client-side; we do the same here.
    let matched_selectors: Vec<String> = entry
        .get("matchedSelectorIndexes")
        .and_then(Value::as_array)
        .map(|idxs| {
            idxs.iter()
                .filter_map(|v| {
                    let idx = usize::try_from(v.as_u64()?).ok()?;
                    selectors.get(idx).map(|s| (*s).to_string())
                })
                .collect()
        })
        .unwrap_or_default();

    // Media queries live in `rule.ancestorData[]` entries where
    // `type === "media"`; `value` is the joined media text.  See
    // devtools/server/actors/style-rule.js:_getAncestorDataForForm.
    let media: Vec<String> = rule
        .get("ancestorData")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|d| {
                    let ty = d.get("type").and_then(Value::as_str)?;
                    if ty != "media" {
                        return None;
                    }
                    d.get("value").and_then(Value::as_str).map(String::from)
                })
                .collect()
        })
        .unwrap_or_default();

    // Extract the rule actor ID (`rule.actor`).  Firefox populates this with a
    // stable actor path like `conn0/styleRuleActor1`.  Used downstream for
    // deduplication in `styles --applied` (Theme E, iter-84).
    // Use `ActorId::try_new` so absent/empty fields are stored as None.
    let rule_actor_id = rule
        .get("actor")
        .and_then(Value::as_str)
        .and_then(ActorId::try_new);

    Some(AppliedRule {
        selector,
        source,
        line,
        column,
        properties,
        matched_selectors,
        media,
        rule_actor_id,
    })
}

/// Parse a `getLayout` response into a `BoxModelLayout`.
///
/// Firefox returns side values as separate string keys like `"margin-top"`, `"margin-right"`, etc.
/// Width and height may be numbers or numeric strings.
fn parse_box_model_layout(response: &Value) -> Result<BoxModelLayout, ProtocolError> {
    let width = parse_f64_field(response, "width")
        .ok_or_else(|| ProtocolError::InvalidPacket("getLayout response missing 'width'".into()))?;
    let height = parse_f64_field(response, "height").ok_or_else(|| {
        ProtocolError::InvalidPacket("getLayout response missing 'height'".into())
    })?;

    let margin = BoxSides {
        top: parse_f64_field(response, "margin-top").unwrap_or(0.0),
        right: parse_f64_field(response, "margin-right").unwrap_or(0.0),
        bottom: parse_f64_field(response, "margin-bottom").unwrap_or(0.0),
        left: parse_f64_field(response, "margin-left").unwrap_or(0.0),
    };

    let border = BoxSides {
        top: parse_f64_field(response, "border-top-width").unwrap_or(0.0),
        right: parse_f64_field(response, "border-right-width").unwrap_or(0.0),
        bottom: parse_f64_field(response, "border-bottom-width").unwrap_or(0.0),
        left: parse_f64_field(response, "border-left-width").unwrap_or(0.0),
    };

    let padding = BoxSides {
        top: parse_f64_field(response, "padding-top").unwrap_or(0.0),
        right: parse_f64_field(response, "padding-right").unwrap_or(0.0),
        bottom: parse_f64_field(response, "padding-bottom").unwrap_or(0.0),
        left: parse_f64_field(response, "padding-left").unwrap_or(0.0),
    };

    let box_sizing = response
        .get("box-sizing")
        .and_then(Value::as_str)
        .unwrap_or("content-box")
        .to_string();

    let position = response
        .get("position")
        .and_then(Value::as_str)
        .unwrap_or("static")
        .to_string();

    let display = response
        .get("display")
        .and_then(Value::as_str)
        .unwrap_or("block")
        .to_string();

    let auto_margins = response.get("autoMargins").cloned();

    Ok(BoxModelLayout {
        width,
        height,
        margin,
        border,
        padding,
        box_sizing,
        position,
        display,
        auto_margins,
    })
}

/// Parse a field that Firefox may send as either a JSON number or a numeric string.
fn parse_f64_field(response: &Value, key: &str) -> Option<f64> {
    let v = response.get(key)?;
    if let Some(n) = v.as_f64() {
        return Some(n);
    }
    v.as_str().and_then(|s| s.parse::<f64>().ok())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// A transport backed by a loopback TCP pair.  For inline (non-longString)
    /// computed values `parse_computed_properties` never touches the socket, so
    /// the pair only satisfies the signature.
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
    fn parse_computed_properties_extracts_sorted_properties() {
        let response = json!({ "computed": {
            "color": {"value": "rgb(0, 0, 0)", "priority": ""},
            "font-size": {"value": "16px", "priority": ""},
            "background-color": {"value": "rgba(0, 0, 0, 0)", "priority": "important"}
        }});
        let props = parse_computed_properties(&mut dummy_transport(), &response).unwrap();
        // Should be sorted alphabetically
        assert_eq!(props.len(), 3);
        assert_eq!(props[0].name, "background-color");
        assert_eq!(props[0].value, "rgba(0, 0, 0, 0)");
        assert_eq!(props[0].priority, "important");
        assert_eq!(props[1].name, "color");
        assert_eq!(props[1].value, "rgb(0, 0, 0)");
        assert_eq!(props[2].name, "font-size");
    }

    #[test]
    fn parse_computed_properties_empty_object() {
        let response = json!({ "computed": {} });
        let props = parse_computed_properties(&mut dummy_transport(), &response).unwrap();
        assert!(props.is_empty());
    }

    #[test]
    fn parse_computed_properties_non_object_returns_error() {
        let response = json!({ "computed": ["not", "an", "object"] });
        assert!(parse_computed_properties(&mut dummy_transport(), &response).is_err());
    }

    #[test]
    fn parse_computed_properties_missing_computed_field_returns_error() {
        let response = json!({ "other": {} });
        assert!(parse_computed_properties(&mut dummy_transport(), &response).is_err());
    }

    /// iter-102 Theme A: a computed property `value` arriving as a longString
    /// grip (e.g. a large CSS custom property) is resolved to its full content
    /// — previously `.as_str()` dropped the whole property.
    #[test]
    fn parse_computed_properties_resolves_longstring_value() {
        use std::io::{BufReader, Write};
        use std::net::TcpListener;
        use std::time::Duration;

        use crate::transport::{encode_frame, recv_from};

        let full = "v".repeat(20_000);
        let full_for_server = full.clone();
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
            let req = recv_from(&mut reader).unwrap();
            assert_eq!(req["type"], "substring");
            assert_eq!(req["to"], "conn0/longString4");
            let resp = json!({"from":"conn0/longString4","substring": full_for_server});
            writer
                .write_all(encode_frame(&serde_json::to_string(&resp).unwrap()).as_bytes())
                .unwrap();
        });

        let mut transport =
            RdpTransport::connect("127.0.0.1", port, Duration::from_secs(5)).unwrap();
        let response = json!({ "computed": {
            "--big-token": {
                "value": {
                    "type": "longString",
                    "actor": "conn0/longString4",
                    "length": 20_000,
                    "initial": "v".repeat(1024),
                },
                "priority": ""
            },
            "color": {"value": "rgb(0, 0, 0)", "priority": ""}
        }});
        let props = parse_computed_properties(&mut transport, &response).unwrap();
        assert_eq!(props.len(), 2);
        let big = props.iter().find(|p| p.name == "--big-token").unwrap();
        assert_eq!(big.value.len(), 20_000);
        assert_eq!(big.value, full);
        handle.join().unwrap();
    }

    #[test]
    fn parse_applied_entry_extracts_css_rule() {
        let entry = json!({
            "rule": {
                "type": 1,
                "selectors": ["h1", ".title"],
                "href": "https://example.com/style.css",
                "line": 42,
                "column": 1
            },
            "matchedSelectorIndexes": [0],
            "declarations": [
                {"name": "color", "value": "red", "priority": ""},
                {"name": "font-weight", "value": "bold", "priority": "important"}
            ]
        });
        let rule = parse_applied_entry(&entry).unwrap();
        assert_eq!(rule.selector, "h1, .title");
        assert_eq!(
            rule.source.as_deref(),
            Some("https://example.com/style.css")
        );
        assert_eq!(rule.line, Some(42));
        assert_eq!(rule.column, Some(1));
        assert_eq!(rule.properties.len(), 2);
        assert_eq!(rule.properties[0].name, "color");
        assert_eq!(rule.properties[0].value, "red");
        assert_eq!(rule.properties[1].name, "font-weight");
        assert_eq!(rule.properties[1].priority, "important");
    }

    #[test]
    fn parse_applied_entry_resolves_matched_selector_indexes() {
        // Spec shape (style-types.js): matchedSelectorIndexes is on the
        // entry and indexes into rule.selectors.
        let entry = json!({
            "rule": {
                "type": 1,
                "selectors": ["h1", ".title", "#x"],
                "href": "https://example.com/style.css",
                "line": 42,
                "column": 1
            },
            "matchedSelectorIndexes": [1, 2],
            "declarations": []
        });
        let rule = parse_applied_entry(&entry).unwrap();
        assert_eq!(rule.matched_selectors, vec![".title", "#x"]);
    }

    #[test]
    fn parse_applied_entry_extracts_media_from_ancestor_data() {
        // Spec shape (style-rule.js _getAncestorDataForForm): media query
        // text is surfaced in ancestorData[] entries of type "media".
        let entry = json!({
            "rule": {
                "type": 1,
                "selectors": ["p"],
                "href": "https://example.com/style.css",
                "line": 1,
                "column": 1,
                "ancestorData": [
                    {"type": "media", "value": "(max-width: 600px)"},
                    {"type": "supports", "value": "(display: flex)"}
                ]
            },
            "matchedSelectorIndexes": [0],
            "declarations": []
        });
        let rule = parse_applied_entry(&entry).unwrap();
        assert_eq!(rule.media, vec!["(max-width: 600px)"]);
    }

    #[test]
    fn parse_applied_entry_skips_inline_style_without_matched_selectors() {
        // Inline style declarations (type 0) never have matchedSelectorIndexes
        // populated — the entry is rejected by the non-empty guard.
        let entry = json!({
            "rule": {"type": 0, "selectors": [], "href": ""},
            "declarations": [{"name": "color", "value": "blue", "priority": ""}]
        });
        assert!(parse_applied_entry(&entry).is_none());
    }

    #[test]
    fn parse_applied_entry_absent_type_accepted_when_matched_selectors_present() {
        // Theme A (iter-84): some Firefox versions omit `type` for external
        // stylesheet rules.  These are accepted as long as matchedSelectorIndexes
        // is non-empty — the `type` field is no longer the discriminator.
        let entry = json!({
            "rule": {
                "selectors": ["h1"],
                "href": "https://example.com/style.css",
                "line": 1,
                "column": 1
            },
            "matchedSelectorIndexes": [0],
            "declarations": [{"name": "color", "value": "red", "priority": ""}]
        });
        let rule = parse_applied_entry(&entry).unwrap();
        assert_eq!(rule.selector, "h1");
        assert_eq!(rule.properties[0].name, "color");
    }

    #[test]
    fn parse_applied_entry_missing_rule_returns_none() {
        let entry = json!({"declarations": []});
        assert!(parse_applied_entry(&entry).is_none());
    }

    #[test]
    fn parse_applied_entry_empty_href_gives_none_source() {
        let entry = json!({
            "rule": {"type": 1, "selectors": ["p"], "href": "", "line": 1, "column": 1},
            "matchedSelectorIndexes": [0],
            "declarations": []
        });
        let rule = parse_applied_entry(&entry).unwrap();
        assert!(rule.source.is_none());
    }

    #[test]
    fn parse_box_model_layout_numeric_fields() {
        let response = json!({
            "width": 800.0,
            "height": 600.0,
            "margin-top": "10",
            "margin-right": "0",
            "margin-bottom": "10",
            "margin-left": "0",
            "border-top-width": "1",
            "border-right-width": "1",
            "border-bottom-width": "1",
            "border-left-width": "1",
            "padding-top": "8",
            "padding-right": "16",
            "padding-bottom": "8",
            "padding-left": "16",
            "box-sizing": "border-box",
            "position": "relative",
            "display": "block",
            "autoMargins": {"top": true}
        });
        let layout = parse_box_model_layout(&response).unwrap();
        assert!((layout.width - 800.0).abs() < f64::EPSILON);
        assert!((layout.height - 600.0).abs() < f64::EPSILON);
        assert!((layout.margin.top - 10.0).abs() < f64::EPSILON);
        assert!((layout.margin.bottom - 10.0).abs() < f64::EPSILON);
        assert!((layout.border.top - 1.0).abs() < f64::EPSILON);
        assert!((layout.padding.left - 16.0).abs() < f64::EPSILON);
        assert_eq!(layout.box_sizing, "border-box");
        assert_eq!(layout.position, "relative");
        assert_eq!(layout.display, "block");
        assert!(layout.auto_margins.is_some());
    }

    #[test]
    fn parse_box_model_layout_missing_width_returns_error() {
        let response = json!({"height": 100.0});
        assert!(parse_box_model_layout(&response).is_err());
    }

    #[test]
    fn parse_box_model_layout_missing_height_returns_error() {
        let response = json!({"width": 100.0});
        assert!(parse_box_model_layout(&response).is_err());
    }

    #[test]
    fn parse_box_model_layout_defaults_for_missing_sides() {
        let response = json!({"width": 100.0, "height": 50.0});
        let layout = parse_box_model_layout(&response).unwrap();
        assert!((layout.margin.top - 0.0).abs() < f64::EPSILON);
        assert!((layout.border.right - 0.0).abs() < f64::EPSILON);
        assert!((layout.padding.bottom - 0.0).abs() < f64::EPSILON);
        assert_eq!(layout.box_sizing, "content-box");
        assert_eq!(layout.position, "static");
        assert_eq!(layout.display, "block");
        assert!(layout.auto_margins.is_none());
    }

    #[test]
    fn parse_f64_field_accepts_number() {
        let v = json!({"width": 42.5});
        let result = parse_f64_field(&v, "width").unwrap();
        assert!((result - 42.5).abs() < f64::EPSILON);
    }

    #[test]
    fn parse_f64_field_accepts_numeric_string() {
        let v = json!({"margin-top": "8"});
        let result = parse_f64_field(&v, "margin-top").unwrap();
        assert!((result - 8.0).abs() < f64::EPSILON);
    }

    #[test]
    fn parse_f64_field_missing_key_returns_none() {
        let v = json!({});
        assert_eq!(parse_f64_field(&v, "width"), None);
    }

    #[test]
    fn parse_applied_entry_extracts_rule_actor_id() {
        let entry = json!({
            "rule": {
                "type": 1,
                "actor": "conn0/styleRuleActor42",
                "selectors": ["h1"],
                "href": "https://example.com/style.css",
                "line": 1,
                "column": 1
            },
            "matchedSelectorIndexes": [0],
            "declarations": [{"name": "color", "value": "red", "priority": ""}]
        });
        let rule = parse_applied_entry(&entry).unwrap();
        assert_eq!(
            rule.rule_actor_id.as_ref().map(std::convert::AsRef::as_ref),
            Some("conn0/styleRuleActor42")
        );
    }

    /// iter-85 Theme A: Firefox 151 sends `type: 100` (CSSStyleRule) for ordinary
    /// author rules.  The old type-based guard rejected these, returning an empty
    /// `rules[]`.  The new guard uses `matchedSelectorIndexes` instead.
    #[test]
    fn unit_cascade_accepts_css_type_100() {
        // Synthetic entry shaped like a real Firefox 151 `getApplied` response for
        // an ordinary author rule.  `type: 100` corresponds to `CSSStyleRule` in
        // the Firefox devtools spec — previously rejected as "unknown type".
        let entry = json!({
            "rule": {
                "type": 100,
                "className": "CSSStyleRule",
                "selectors": ["h1"],
                "href": "https://tennis-sepp.ch/style.css",
                "line": 10,
                "column": 1
            },
            "matchedSelectorIndexes": [0],
            "declarations": [{"name": "color", "value": "rgb(0,0,0)", "priority": ""}]
        });
        let rule = parse_applied_entry(&entry);
        assert!(
            rule.is_some(),
            "type-100 (CSSStyleRule) entry with non-empty matchedSelectorIndexes \
             must be accepted; got None"
        );
        let rule = rule.unwrap();
        assert_eq!(rule.selector, "h1");
        assert_eq!(rule.matched_selectors, vec!["h1"]);
        assert_eq!(rule.properties[0].name, "color");
    }

    /// iter-88 Theme A: the real FF 151 tennis-sepp.ch shape — `type: 1`,
    /// `className: "CSSStyleRule"`, non-empty `matchedSelectorIndexes`, and
    /// declarations nested under `rule.declarations` (NOT at entry top level).
    /// Exercises both the `className == "CSSStyleRule"` branch and the
    /// `rule.declarations` fallback that was the missing piece in iter-85.
    #[test]
    fn unit_cascade_accepts_css_style_rule_sentinel() {
        let entry = json!({
            "matchedSelectorIndexes": [0],
            "rule": {
                "type": 1,
                "className": "CSSStyleRule",
                "selectors": ["h1"],
                "href": "https://tennis-sepp.ch/style.css",
                "line": 137,
                "column": 1,
                "declarations": [
                    {"name": "color", "value": "rgb(34, 34, 34)", "priority": ""}
                ]
            }
        });
        let rule = parse_applied_entry(&entry)
            .expect("CSSStyleRule entry must be accepted via className branch");
        assert_eq!(rule.selector, "h1");
        assert_eq!(rule.properties.len(), 1);
        assert_eq!(rule.properties[0].name, "color");
        assert_eq!(rule.properties[0].value, "rgb(34, 34, 34)");
    }

    /// iter-88 Theme A: the FF 151 element-style sentinel (`type: 100`,
    /// numeric `className: 100`, no selectors / `matchedSelectorIndexes`)
    /// must be REJECTED. Previously `type == 100` alone was accepted; the
    /// CodeRabbit review on PR #125 caught the over-acceptance.
    #[test]
    fn unit_cascade_rejects_element_style_sentinel() {
        let entry = json!({
            "rule": {
                "type": 100,
                "className": 100,
                "href": "https://tennis-sepp.ch/"
            }
        });
        assert!(
            parse_applied_entry(&entry).is_none(),
            "FF 151 element-style sentinel (type==100 with numeric className) \
             must not produce an AppliedRule"
        );
    }

    /// iter-88 Theme A: entries with a non-empty `matchedSelectorIndexes`
    /// array are accepted regardless of `type`/`className` — backward-compat
    /// with older Firefox replies that relied on this discriminator.
    #[test]
    fn unit_cascade_accepts_non_empty_matched_selector_indexes() {
        let entry = json!({
            "rule": {
                "type": 999,
                "selectors": ["h3"],
                "href": "https://example.com/style.css",
                "line": 1,
                "column": 1
            },
            "matchedSelectorIndexes": [0],
            "declarations": [{"name": "color", "value": "blue", "priority": ""}]
        });
        let rule = parse_applied_entry(&entry)
            .expect("entry with non-empty matchedSelectorIndexes must be accepted");
        assert_eq!(rule.selector, "h3");
        assert_eq!(rule.matched_selectors, vec!["h3"]);
    }

    /// iter-88 pre-fix repro: loads the checked-in real-recording fixture
    /// `cascade_tennis_sepp_h1_color.json` and asserts the parser yields at
    /// least one rule.  Red on origin/main (fixture file absent + parser
    /// rejects the FF 151 shape); green on branch HEAD.
    #[test]
    fn pre_fix_repro_cascade_fixture_red_then_green() {
        let fixture_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join("cascade_tennis_sepp_h1_color.json");
        let fixture = std::fs::read_to_string(&fixture_path).unwrap_or_else(|e| {
            panic!(
                "pre-fix repro fixture missing at {}: {e} \
                 — on origin/main this is expected (file added in iter-88)",
                fixture_path.display()
            )
        });
        let response: Value = serde_json::from_str(&fixture).expect("fixture is not valid JSON");
        let entries = response
            .get("entries")
            .and_then(Value::as_array)
            .expect("fixture missing 'entries'");
        let rules: Vec<AppliedRule> = entries.iter().filter_map(parse_applied_entry).collect();
        // The fixture contains 3 entries: 1 element-style sentinel that must
        // be rejected, and 2 real CSSStyleRule author rules.
        assert_eq!(
            rules.len(),
            2,
            "fixture should yield exactly 2 author rules (sentinel excluded); got {}",
            rules.len()
        );
        assert!(
            rules.iter().all(|r| !r.selector.is_empty()),
            "every parsed rule must have a non-empty selector"
        );
        assert!(
            rules.iter().all(|r| !r.properties.is_empty()),
            "every parsed rule must have ≥1 property — exercises the \
             rule.declarations fallback that was the iter-85 → iter-88 fix"
        );
        let has_color = rules
            .iter()
            .any(|r| r.properties.iter().any(|p| p.name == "color"));
        assert!(
            has_color,
            "expected at least one rule with a `color` declaration in the \
             tennis-sepp.ch h1 cascade"
        );
    }

    #[test]
    fn applied_rule_serialization_skips_optional_fields() {
        let rule = AppliedRule {
            selector: "p".to_string(),
            source: None,
            line: None,
            column: None,
            properties: vec![],
            matched_selectors: vec![],
            media: vec![],
            rule_actor_id: None,
        };
        let v = serde_json::to_value(&rule).unwrap();
        assert!(v.get("source").is_none());
        assert!(v.get("line").is_none());
        assert!(v.get("column").is_none());
        assert!(
            v.get("rule_actor_id").is_none(),
            "None actor_id should not serialize"
        );
        assert_eq!(v["selector"], "p");
    }
}
