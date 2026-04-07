use ff_rdp_core::{AccessibilityActor, AccessibleNode, WebConsoleActor, filter_interactive};
use serde_json::{Value, json};

use crate::cli::args::Cli;
use crate::error::AppError;
use crate::output;
use crate::output_pipeline::OutputPipeline;

use super::connect_tab::{ConnectedTab, connect_and_get_target};
use super::js_helpers::resolve_result;

pub fn run(
    cli: &Cli,
    depth: u32,
    max_chars: u32,
    selector: Option<&str>,
    interactive: bool,
) -> Result<(), AppError> {
    let mut ctx = connect_and_get_target(cli)?;

    let accessibility_actor = ctx.target.accessibility_actor.clone().ok_or_else(|| {
        AppError::User(
            "no accessibility actor available — accessibility may not be enabled in Firefox"
                .to_string(),
        )
    })?;

    // If selector is provided, use JS eval approach (similar to snapshot).
    let tree = if let Some(sel) = selector {
        run_selector_mode(&mut ctx, sel, depth, max_chars)?
    } else {
        // Use native RDP protocol.
        let walker = AccessibilityActor::get_walker(ctx.transport_mut(), &accessibility_actor)
            .map_err(|e| map_a11y_error(e, cli))?;

        let root = AccessibilityActor::get_root(ctx.transport_mut(), &walker)
            .map_err(|e| map_a11y_error(e, cli))?;

        AccessibilityActor::walk_tree(ctx.transport_mut(), &walker, &root, depth, max_chars)
            .map_err(|e| map_a11y_error(e, cli))?
    };

    // Apply interactive filter.
    let tree = if interactive {
        filter_interactive(&tree).unwrap_or_else(|| AccessibleNode {
            actor: None,
            role: "document".to_string(),
            name: Some("(no interactive elements)".to_string()),
            value: None,
            description: None,
            child_count: None,
            states: vec![],
            dom_node_type: None,
            index_in_parent: None,
            children: vec![],
            truncated: None,
        })
    } else {
        tree
    };

    // Strip internal actor IDs from output (not useful to end users).
    let mut results = serde_json::to_value(&tree).map_err(|e| AppError::Internal(e.into()))?;
    strip_actor_ids(&mut results);

    let meta = json!({
        "host": cli.host,
        "port": cli.port,
        "depth": depth,
        "max_chars": max_chars,
    });

    let envelope = output::envelope(&results, 1, &meta);

    OutputPipeline::new(cli.jq.clone())
        .finalize(&envelope)
        .map_err(AppError::from)
}

/// Selector-based subtree extraction via JS eval.
///
/// Uses ARIA properties and computed roles available on DOM elements to build
/// an accessibility-like tree rooted at the matched element.
fn run_selector_mode(
    ctx: &mut ConnectedTab,
    selector: &str,
    depth: u32,
    max_chars: u32,
) -> Result<AccessibleNode, AppError> {
    let js = A11Y_SELECTOR_JS_TEMPLATE
        .replace(
            "__SELECTOR__",
            &super::js_helpers::escape_selector(selector),
        )
        .replace("__DEPTH__", &depth.to_string())
        .replace("__MAX_CHARS__", &max_chars.to_string());

    let console_actor = ctx.target.console_actor.clone();
    let eval_result = WebConsoleActor::evaluate_js_async(ctx.transport_mut(), &console_actor, &js)
        .map_err(AppError::from)?;

    if let Some(ref exc) = eval_result.exception {
        let msg = exc
            .message
            .as_deref()
            .unwrap_or("a11y selector evaluation failed");
        return Err(AppError::User(format!("a11y --selector failed: {msg}")));
    }

    let result = resolve_result(ctx, &eval_result.result)?;

    if result.is_null() {
        return Err(AppError::User(format!(
            "no element matching selector '{selector}'"
        )));
    }

    parse_js_a11y_tree(&result).ok_or_else(|| {
        AppError::User("failed to parse accessibility tree from JS evaluation".to_string())
    })
}

fn parse_js_a11y_tree(value: &Value) -> Option<AccessibleNode> {
    let role = value.get("role")?.as_str()?.to_string();
    let name = value
        .get("name")
        .and_then(Value::as_str)
        .map(String::from)
        .filter(|s| !s.is_empty());
    let value_str = value
        .get("value")
        .and_then(Value::as_str)
        .map(String::from)
        .filter(|s| !s.is_empty());
    let description = value
        .get("description")
        .and_then(Value::as_str)
        .map(String::from)
        .filter(|s| !s.is_empty());

    let children: Vec<AccessibleNode> = value
        .get("children")
        .and_then(Value::as_array)
        .map(|arr| arr.iter().filter_map(parse_js_a11y_tree).collect())
        .unwrap_or_default();

    let truncated = value
        .get("truncated")
        .and_then(Value::as_str)
        .map(String::from);

    Some(AccessibleNode {
        actor: None,
        role,
        name,
        value: value_str,
        description,
        child_count: None,
        states: vec![],
        dom_node_type: None,
        index_in_parent: None,
        children,
        truncated,
    })
}

/// Map protocol errors to user-friendly messages, especially unknownActor.
fn map_a11y_error(err: ff_rdp_core::ProtocolError, cli: &Cli) -> AppError {
    match &err {
        ff_rdp_core::ProtocolError::ActorError { error, .. }
            if error == "noSuchActor" || error == "unknownActor" =>
        {
            let hint = if cli.no_daemon {
                " — the accessibility actor may have expired after navigation. Re-run the command"
            } else {
                " — the accessibility actor may have expired after navigation. Re-run the command to get a fresh actor"
            };
            AppError::User(format!("accessibility actor is no longer valid{hint}"))
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

/// JS template for selector-based accessibility tree extraction.
///
/// Uses ARIA properties and computed roles available on DOM elements.
/// `__SELECTOR__`, `__DEPTH__`, and `__MAX_CHARS__` are replaced before evaluation.
const A11Y_SELECTOR_JS_TEMPLATE: &str = r#"(function() {
  var SKIP = {SCRIPT:1,STYLE:1,NOSCRIPT:1,SVG:1};
  var ROLE_MAP = {NAV:'navigation',HEADER:'banner',FOOTER:'contentinfo',MAIN:'main',
    ASIDE:'complementary',ARTICLE:'article',SECTION:'region',FORM:'form',
    DIALOG:'dialog',A:'link',BUTTON:'button',INPUT:'textbox',SELECT:'combobox',
    TEXTAREA:'textbox',H1:'heading',H2:'heading',H3:'heading',H4:'heading',
    H5:'heading',H6:'heading',IMG:'img',TABLE:'table',UL:'list',OL:'list',
    LI:'listitem',DETAILS:'group',SUMMARY:'button'};
  var maxDepth = __DEPTH__;
  var maxChars = __MAX_CHARS__;
  var totalChars = 0;

  function getRole(el) {
    var explicit = el.getAttribute && el.getAttribute('role');
    if (explicit) return explicit;
    if (el.computedRole && el.computedRole !== 'generic') return el.computedRole;
    return ROLE_MAP[el.tagName] || 'generic';
  }

  function getName(el) {
    if (el.ariaLabel) return el.ariaLabel;
    var labelledBy = el.getAttribute && el.getAttribute('aria-labelledby');
    if (labelledBy) {
      var label = document.getElementById(labelledBy);
      if (label) return label.textContent.trim();
    }
    if (el.tagName === 'IMG') return el.alt || '';
    if (el.tagName === 'INPUT' || el.tagName === 'TEXTAREA' || el.tagName === 'SELECT') {
      if (el.labels && el.labels.length) return el.labels[0].textContent.trim();
      return el.placeholder || '';
    }
    if (!el.children || el.children.length === 0) {
      var t = el.textContent && el.textContent.trim();
      if (t && t.length <= 200) return t;
      if (t) return t.slice(0, 200) + '...';
    }
    return '';
  }

  function walk(node, depth) {
    if (node.nodeType === 3) {
      var t = node.textContent.trim();
      if (!t || totalChars >= maxChars) return null;
      totalChars += t.length;
      return {role: 'text', name: t.length > 200 ? t.slice(0,200)+'...' : t};
    }
    if (node.nodeType !== 1) return null;
    if (SKIP[node.tagName]) return null;

    try {
      var cs = window.getComputedStyle(node);
      if (cs.display === 'none' || cs.visibility === 'hidden') return null;
    } catch(e) {}
    if (node.getAttribute && node.getAttribute('aria-hidden') === 'true') return null;

    var role = getRole(node);
    var name = getName(node);
    var o = {role: role};
    if (name) o.name = name;

    var desc = node.getAttribute && node.getAttribute('aria-description');
    if (desc) o.description = desc;

    var val = node.value;
    if (val && (node.tagName === 'INPUT' || node.tagName === 'TEXTAREA' || node.tagName === 'SELECT')) {
      o.value = String(val);
    }

    if (depth >= maxDepth) {
      if (node.children.length > 0) o.truncated = node.children.length + ' children not shown';
      return o;
    }

    var children = [];
    var charCapped = false;
    for (var i = 0; i < node.childNodes.length; i++) {
      if (totalChars >= maxChars) { charCapped = true; break; }
      var c = walk(node.childNodes[i], depth + 1);
      if (c !== null && c.role !== 'generic') children.push(c);
      else if (c !== null && c.children) {
        for (var j = 0; j < c.children.length; j++) children.push(c.children[j]);
      }
    }
    if (children.length) o.children = children;
    if (charCapped) o.truncated = 'max characters reached';
    return o;
  }

  var root = document.querySelector("__SELECTOR__");
  if (!root) return '__FF_RDP_JSON__null';
  var tree = walk(root, 0);
  return '__FF_RDP_JSON__' + JSON.stringify(tree);
})()"#;

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a11y_js_template_substitution() {
        let js = A11Y_SELECTOR_JS_TEMPLATE
            .replace("__SELECTOR__", "main")
            .replace("__DEPTH__", "4")
            .replace("__MAX_CHARS__", "20000");
        assert!(js.contains("var maxDepth = 4;"));
        assert!(js.contains("var maxChars = 20000;"));
        assert!(!js.contains("__DEPTH__"));
        assert!(!js.contains("__MAX_CHARS__"));
    }

    #[test]
    fn a11y_js_template_has_sentinel() {
        assert!(A11Y_SELECTOR_JS_TEMPLATE.contains("__FF_RDP_JSON__"));
    }

    #[test]
    fn a11y_js_template_skips_hidden_elements() {
        assert!(A11Y_SELECTOR_JS_TEMPLATE.contains("aria-hidden"));
        assert!(A11Y_SELECTOR_JS_TEMPLATE.contains("display === 'none'"));
        assert!(A11Y_SELECTOR_JS_TEMPLATE.contains("visibility === 'hidden'"));
    }

    #[test]
    fn a11y_js_template_has_role_map() {
        assert!(A11Y_SELECTOR_JS_TEMPLATE.contains("ROLE_MAP"));
        assert!(A11Y_SELECTOR_JS_TEMPLATE.contains("BUTTON"));
        assert!(A11Y_SELECTOR_JS_TEMPLATE.contains("INPUT"));
        assert!(A11Y_SELECTOR_JS_TEMPLATE.contains("'link'"));
    }

    #[test]
    fn parse_js_a11y_tree_minimal() {
        let val = json!({"role": "button", "name": "Submit"});
        let node = parse_js_a11y_tree(&val).expect("should parse");
        assert_eq!(node.role, "button");
        assert_eq!(node.name.as_deref(), Some("Submit"));
        assert!(node.children.is_empty());
    }

    #[test]
    fn parse_js_a11y_tree_with_children() {
        let val = json!({
            "role": "list",
            "children": [
                {"role": "listitem", "name": "First"},
                {"role": "listitem", "name": "Second"},
            ]
        });
        let node = parse_js_a11y_tree(&val).expect("should parse");
        assert_eq!(node.role, "list");
        assert_eq!(node.children.len(), 2);
        assert_eq!(node.children[0].name.as_deref(), Some("First"));
    }

    #[test]
    fn parse_js_a11y_tree_empty_name_filtered() {
        let val = json!({"role": "generic", "name": "", "value": ""});
        let node = parse_js_a11y_tree(&val).expect("should parse");
        assert!(node.name.is_none());
        assert!(node.value.is_none());
    }

    #[test]
    fn parse_js_a11y_tree_missing_role_returns_none() {
        let val = json!({"name": "No role here"});
        assert!(parse_js_a11y_tree(&val).is_none());
    }

    #[test]
    fn strip_actor_ids_removes_actor_field() {
        let mut val = json!({
            "actor": "conn1/accessibility1",
            "role": "document",
            "children": [
                {"actor": "conn1/accessible2", "role": "button", "name": "OK"}
            ]
        });
        strip_actor_ids(&mut val);
        assert!(val.get("actor").is_none());
        assert!(val["children"][0].get("actor").is_none());
        assert_eq!(val["children"][0]["role"], "button");
    }
}
