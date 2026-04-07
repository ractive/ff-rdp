use ff_rdp_core::WebConsoleActor;
use serde_json::{Value, json};

use crate::cli::args::Cli;
use crate::error::AppError;
use crate::output;
use crate::output_pipeline::OutputPipeline;

use super::connect_tab::connect_and_get_target;
use super::js_helpers::resolve_result;

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

    let eval_result = WebConsoleActor::evaluate_js_async(ctx.transport_mut(), &console_actor, &js)
        .map_err(AppError::from)?;

    if let Some(ref exc) = eval_result.exception {
        let msg = exc
            .message
            .as_deref()
            .unwrap_or("snapshot evaluation failed");
        eprintln!("error: {msg}");
        return Err(AppError::Exit(1));
    }

    let results = resolve_result(&mut ctx, &eval_result.result)?;
    let meta = json!({"host": cli.host, "port": cli.port, "depth": depth, "max_chars": max_chars});

    let total = match &results {
        Value::Null => 0,
        _ => 1,
    };

    let envelope = output::envelope(&results, total, &meta);

    OutputPipeline::from_cli(cli)?
        .finalize(&envelope)
        .map_err(AppError::from)
}

#[cfg(test)]
mod tests {
    use super::*;

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
