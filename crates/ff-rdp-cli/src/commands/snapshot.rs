use serde_json::{Value, json};

use crate::cli::args::Cli;
use crate::error::AppError;
use crate::hints::{HintContext, HintSource};
use crate::output;
use crate::output_pipeline::OutputPipeline;

use super::connect_tab::connect_and_get_target;
use super::js_helpers::{eval_or_bail, resolve_result};

/// JavaScript IIFE that walks the DOM and returns a compact tree for LLM consumption.
///
/// `__DEPTH__` and `__MAX_CHARS__` are replaced with the actual numeric values
/// before evaluation.
const SNAPSHOT_JS_TEMPLATE: &str = r"(function() {
  var SKIP = {SCRIPT:1,STYLE:1,NOSCRIPT:1,SVG:1};
  var INTERACTIVE = {A:1,BUTTON:1,INPUT:1,SELECT:1,TEXTAREA:1,DETAILS:1,SUMMARY:1};
  var SEMANTIC = {NAV:'navigation',HEADER:'banner',FOOTER:'contentinfo',MAIN:'main',
    ASIDE:'complementary',ARTICLE:'article',SECTION:'region',FORM:'form',
    DIALOG:'dialog',SEARCH:'search'};
  var KEY_ATTRS = ['id','class','href','src','alt','type','name','value',
    'placeholder','aria-label','aria-expanded','aria-hidden','data-testid'];
  var maxDepth = __DEPTH__;
  var maxChars = __MAX_CHARS__;
  var totalChars = 0;
  var textTruncated = false;

  function isHidden(el) {
    if (el.getAttribute && el.getAttribute('aria-hidden') === 'true') return true;
    try {
      var cs = window.getComputedStyle(el);
      if (cs.display === 'none' || cs.visibility === 'hidden') return true;
    } catch(e) {}
    return false;
  }

  function walk(node, depth) {
    if (node.nodeType === 3) {
      var t = node.textContent.trim();
      if (!t) return null;
      if (totalChars >= maxChars) { textTruncated = true; return null; }
      if (t.length > 200) t = t.slice(0, 200) + '...';
      totalChars += t.length;
      return t;
    }
    if (node.nodeType !== 1) return null;
    var tag = node.tagName;
    if (SKIP[tag]) return null;
    if (isHidden(node)) return null;

    var o = {tag: tag.toLowerCase()};
    var role = node.getAttribute('role') || SEMANTIC[tag] || null;
    if (role) o.role = role;
    if (INTERACTIVE[tag]) o.interactive = true;

    var a = {};
    for (var i = 0; i < KEY_ATTRS.length; i++) {
      var v = node.getAttribute(KEY_ATTRS[i]);
      if (v != null && v !== '') a[KEY_ATTRS[i]] = v.length > 200 ? v.slice(0,200)+'...' : v;
    }
    if (Object.keys(a).length) o.attrs = a;

    if (depth >= maxDepth) {
      var cc = node.children.length;
      if (cc > 0) o.truncated = cc + ' children not shown';
      return o;
    }

    var children = [];
    for (var j = 0; j < node.childNodes.length; j++) {
      var c = walk(node.childNodes[j], depth + 1);
      if (c !== null) children.push(c);
    }
    if (children.length) o.children = children;
    return o;
  }

  var tree = walk(document.documentElement, 0);
  if (tree && textTruncated) { tree.textTruncated = true; }
  return '__FF_RDP_JSON__' + JSON.stringify(tree);
})()";

pub fn run(cli: &Cli, depth: u32, max_chars: u32) -> Result<(), AppError> {
    let mut ctx = connect_and_get_target(cli)?;
    let console_actor = ctx.target.console_actor.clone();

    let js = SNAPSHOT_JS_TEMPLATE
        .replace("__DEPTH__", &depth.to_string())
        .replace("__MAX_CHARS__", &max_chars.to_string());

    let eval_result = eval_or_bail(&mut ctx, &console_actor, &js, "snapshot evaluation failed")?;

    let results = resolve_result(&mut ctx, &eval_result.result)?;
    let meta = json!({"host": cli.host, "port": cli.port, "depth": depth, "max_chars": max_chars});

    let total = match &results {
        Value::Null => 0,
        _ => 1,
    };

    let envelope = output::envelope(&results, total, &meta);

    if cli.format == "text" && cli.jq.is_none() {
        render_snapshot_text(&results);
        return Ok(());
    }

    let hint_ctx = HintContext::new(HintSource::Snapshot);
    OutputPipeline::from_cli(cli)?
        .finalize_with_hints(&envelope, Some(&hint_ctx))
        .map_err(AppError::from)
}

/// Render a DOM snapshot as an indented tree.
///
/// Each node is printed as:
///   `<indent><tag>[role=…][interactive] [attr=val …] "text content"`
///
/// String nodes (raw text) are printed inline as quoted strings.
/// Truncation and depth-limit notices from the JS walker are preserved.
fn render_snapshot_text(node: &Value) {
    if node.is_null() {
        println!("(empty snapshot)");
        return;
    }
    render_node(node, 0);
}

const SNAPSHOT_TEXT_ATTRS: &[&str] = &[
    "id",
    "class",
    "href",
    "src",
    "type",
    "aria-label",
    "data-testid",
];

fn render_node(node: &Value, depth: usize) {
    use std::fmt::Write as _;
    let indent = "  ".repeat(depth);

    match node {
        // Leaf text node: a plain JSON string
        Value::String(text) => {
            // Truncate long text to keep output readable
            if text.chars().count() > 80 {
                let truncated = text.chars().take(77).collect::<String>();
                println!("{indent}\"{truncated}...\"");
            } else {
                println!("{indent}\"{text}\"");
            }
        }
        Value::Object(_) => {
            let tag = node.get("tag").and_then(Value::as_str).unwrap_or("?");

            let mut line = format!("{indent}<{tag}");

            if let Some(role) = node.get("role").and_then(Value::as_str) {
                let _ = write!(line, " role={role}");
            }
            if node
                .get("interactive")
                .and_then(Value::as_bool)
                .unwrap_or(false)
            {
                line.push_str(" [interactive]");
            }

            if let Some(attrs) = node.get("attrs").and_then(Value::as_object) {
                for key in SNAPSHOT_TEXT_ATTRS {
                    if let Some(val) = attrs.get(*key).and_then(Value::as_str) {
                        let val = if val.chars().count() > 40 {
                            format!("{}...", val.chars().take(37).collect::<String>())
                        } else {
                            val.to_string()
                        };
                        let _ = write!(line, " {key}={val:?}");
                    }
                }
            }

            if let Some(truncated) = node.get("truncated").and_then(Value::as_str) {
                let _ = write!(line, " ({truncated})");
            }

            println!("{line}");

            if let Some(Value::Array(children)) = node.get("children") {
                for child in children {
                    render_node(child, depth + 1);
                }
            }

            if node
                .get("textTruncated")
                .and_then(Value::as_bool)
                .unwrap_or(false)
            {
                println!("{indent}  [text truncated — increase --max-chars]");
            }
        }
        // Unexpected node shape: fall back to compact JSON
        other => {
            println!("{indent}{other}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── render_snapshot_text smoke tests ─────────────────────────────────────
    //
    // stdout cannot easily be captured in unit tests, so we verify the
    // rendering functions do not panic on representative inputs.

    #[test]
    fn render_snapshot_null_does_not_panic() {
        render_snapshot_text(&Value::Null);
    }

    #[test]
    fn render_snapshot_simple_element_does_not_panic() {
        let node = json!({
            "tag": "div",
            "attrs": {"id": "main", "class": "container"},
            "children": [
                {"tag": "h1", "children": ["Hello World"]},
                {"tag": "a", "interactive": true, "attrs": {"href": "https://example.com"}}
            ]
        });
        render_snapshot_text(&node);
    }

    #[test]
    fn render_snapshot_with_role_and_truncated_does_not_panic() {
        let node = json!({
            "tag": "nav",
            "role": "navigation",
            "truncated": "3 children not shown"
        });
        render_snapshot_text(&node);
    }

    #[test]
    fn render_snapshot_text_truncated_flag_does_not_panic() {
        let node = json!({
            "tag": "body",
            "textTruncated": true,
            "children": ["some text"]
        });
        render_snapshot_text(&node);
    }

    #[test]
    fn render_snapshot_long_text_does_not_panic() {
        let long_text = "a".repeat(200);
        let node = json!({
            "tag": "p",
            "children": [long_text]
        });
        render_snapshot_text(&node);
    }

    #[test]
    fn render_snapshot_long_attr_does_not_panic() {
        let long_class = "x".repeat(100);
        let node = json!({
            "tag": "div",
            "attrs": {"class": long_class}
        });
        render_snapshot_text(&node);
    }

    #[test]
    fn snapshot_js_template_substitution() {
        let js = SNAPSHOT_JS_TEMPLATE
            .replace("__DEPTH__", "3")
            .replace("__MAX_CHARS__", "10000");
        assert!(js.contains("var maxDepth = 3;"));
        assert!(js.contains("var maxChars = 10000;"));
        assert!(!js.contains("__DEPTH__"));
        assert!(!js.contains("__MAX_CHARS__"));
    }

    #[test]
    fn snapshot_js_contains_sentinel() {
        assert!(SNAPSHOT_JS_TEMPLATE.contains("__FF_RDP_JSON__"));
    }

    #[test]
    fn snapshot_js_skips_script_style() {
        assert!(SNAPSHOT_JS_TEMPLATE.contains("SKIP"));
        assert!(SNAPSHOT_JS_TEMPLATE.contains("SCRIPT"));
        assert!(SNAPSHOT_JS_TEMPLATE.contains("STYLE"));
        assert!(SNAPSHOT_JS_TEMPLATE.contains("NOSCRIPT"));
        assert!(SNAPSHOT_JS_TEMPLATE.contains("SVG"));
    }

    #[test]
    fn snapshot_js_handles_interactive_elements() {
        assert!(SNAPSHOT_JS_TEMPLATE.contains("INTERACTIVE"));
        assert!(SNAPSHOT_JS_TEMPLATE.contains("BUTTON"));
        assert!(SNAPSHOT_JS_TEMPLATE.contains("INPUT"));
    }
}
