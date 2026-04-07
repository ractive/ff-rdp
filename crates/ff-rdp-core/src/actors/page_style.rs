use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::actor::actor_request;
use crate::error::ProtocolError;
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

        let computed = response.get("computed").ok_or_else(|| {
            ProtocolError::InvalidPacket("getComputed response missing 'computed' field".into())
        })?;

        parse_computed_properties(computed)
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

/// Parse the `computed` object from a `getComputed` response.
///
/// The object has CSS property names as keys, each mapping to `{"value": "...", "priority": ""}`.
fn parse_computed_properties(computed: &Value) -> Result<Vec<ComputedProperty>, ProtocolError> {
    let obj = computed.as_object().ok_or_else(|| {
        ProtocolError::InvalidPacket("'computed' field is not a JSON object".into())
    })?;

    let mut props: Vec<ComputedProperty> = obj
        .iter()
        .filter_map(|(name, entry)| {
            let value = entry.get("value")?.as_str()?.to_string();
            let priority = entry
                .get("priority")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            Some(ComputedProperty {
                name: name.clone(),
                value,
                priority,
            })
        })
        .collect();

    // Sort for stable output ordering.
    props.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(props)
}

/// Parse a single entry from the `entries` array of a `getApplied` response.
///
/// Only includes entries where `rule.type == 1` (stylesheet CSS rules, not inline styles).
fn parse_applied_entry(entry: &Value) -> Option<AppliedRule> {
    let rule = entry.get("rule")?;

    // Only process CSS rules (type == 1), skip inline/other rule types.
    let rule_type = rule.get("type").and_then(Value::as_u64)?;
    if rule_type != 1 {
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

    let properties: Vec<RuleProperty> = entry
        .get("declarations")
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

    Some(AppliedRule {
        selector,
        source,
        line,
        column,
        properties,
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

    #[test]
    fn parse_computed_properties_extracts_sorted_properties() {
        let computed = json!({
            "color": {"value": "rgb(0, 0, 0)", "priority": ""},
            "font-size": {"value": "16px", "priority": ""},
            "background-color": {"value": "rgba(0, 0, 0, 0)", "priority": "important"}
        });
        let props = parse_computed_properties(&computed).unwrap();
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
        let computed = json!({});
        let props = parse_computed_properties(&computed).unwrap();
        assert!(props.is_empty());
    }

    #[test]
    fn parse_computed_properties_non_object_returns_error() {
        let computed = json!(["not", "an", "object"]);
        assert!(parse_computed_properties(&computed).is_err());
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
    fn parse_applied_entry_skips_non_type_1_rules() {
        // type 0 = inline style
        let entry = json!({
            "rule": {"type": 0, "selectors": [], "href": ""},
            "declarations": [{"name": "color", "value": "blue", "priority": ""}]
        });
        assert!(parse_applied_entry(&entry).is_none());
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
    fn applied_rule_serialization_skips_optional_fields() {
        let rule = AppliedRule {
            selector: "p".to_string(),
            source: None,
            line: None,
            column: None,
            properties: vec![],
        };
        let v = serde_json::to_value(&rule).unwrap();
        assert!(v.get("source").is_none());
        assert!(v.get("line").is_none());
        assert!(v.get("column").is_none());
        assert_eq!(v["selector"], "p");
    }
}
