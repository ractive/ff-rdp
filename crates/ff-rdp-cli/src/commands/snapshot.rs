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
    // Theme C (iter-131): `--max-chars` previously bounded only leaf text
    // content — the serialized tree (tags, attrs, structure) was unbounded,
    // making the flag a near-no-op (100 vs 5000 vs default all landed within
    // a few bytes of each other, s61 #9). Bound the *whole* serialized output
    // here, on the Rust side, after the JS walk returns.
    let results = bound_snapshot_output(results, max_chars);

    // iter-141 Theme C: surface truncation in `meta`. Previously the only
    // signal was a `truncated: true` marker buried inside `results` at
    // whatever depth the pruning happened to stop — dogfooding session 63
    // found it at line 3248 of a 231 KB response, with `meta` silent on the
    // subject entirely, so a caller had no cheap way (e.g. `--jq '.meta'`)
    // to detect a partial snapshot without scanning the whole tree. Both
    // keys are always present (iter-128's always-present-nullable-key
    // convention) so `capped: false` reads as an explicit "no, nothing was
    // cut" rather than an absent key that's indistinguishable from "unknown".
    //
    // `truncated` is true if either mechanism cut anything: the whole-tree
    // `--max-chars` budget (`bound_snapshot_output`, root-level `truncated:
    // true`/`children_omitted`) or the JS walker's own per-leaf text cap
    // (`textTruncated`, iter-131). `text_truncated` isolates the latter so a
    // caller can tell which kind of truncation happened.
    let (truncated, text_truncated) = snapshot_truncation_flags(&results);
    let mut meta = json!({
        "depth": depth,
        "max_chars": max_chars,
        "truncated": truncated,
        "text_truncated": text_truncated,
    });
    crate::connection_meta::merge_into_if_verbose(
        &mut meta,
        &cli.host,
        cli.port,
        None,
        cli.is_verbose(),
    );
    // iter-134: always present, not gated by --verbose — an
    // agent can tell how this command executed without a
    // separate `daemon status` round-trip.
    crate::connection_meta::merge_route(&mut meta, ctx.via_daemon);

    let total = match &results {
        Value::Null => 0,
        _ => 1,
    };

    let envelope = output::envelope(&results, total, &meta);

    // Text-only short-circuit (no jq filter): render indented tree directly.
    // When --jq is also set, fall through to the pipeline which applies jq
    // first, then renders text (iter-60 D2 behaviour).
    if cli.format == "text" && cli.jq.is_none() {
        render_snapshot_text(&results);
        return Ok(());
    }

    let hint_ctx = HintContext::new(HintSource::Snapshot);
    OutputPipeline::from_cli(cli)?.finalize_with_hints(&envelope, Some(&hint_ctx))
}

/// Derive the `(truncated, text_truncated)` pair reported in `meta` from a
/// bounded snapshot tree (iter-141 Theme C).
///
/// `text_truncated` reflects the JS walker's own per-leaf `--max-chars` text
/// cap (`textTruncated`, iter-131); `truncated` is `true` when either that
/// or the whole-tree Rust-side bounding pass (`bound_snapshot_output`'s
/// root-level `truncated: true`) cut anything. Split out from `run` so the
/// flag derivation is unit-testable without a live Firefox connection.
fn snapshot_truncation_flags(results: &Value) -> (bool, bool) {
    let structure_truncated = matches!(results.get("truncated"), Some(Value::Bool(true)));
    let text_truncated = matches!(results.get("textTruncated"), Some(Value::Bool(true)));
    (structure_truncated || text_truncated, text_truncated)
}

/// Bound the whole serialized snapshot tree to (approximately) `max_chars`
/// bytes of compact JSON: each node is tried whole first (cheapest, and
/// avoids ever admitting a node that would blow the budget); only when a
/// node doesn't fit whole does its subtree get pruned child-by-child in
/// document order, and a child that still doesn't fit even pruned is dropped
/// entirely rather than included oversized.
///
/// Theme C (iter-131): the JS walker's `maxChars` only bounds the sum of leaf
/// *text* lengths — tags, attributes, and tree structure are unbounded, so a
/// tag/attribute-heavy page barely shrinks between `--max-chars 100` and
/// `--max-chars 5000` (s61 #9: 1741/1742/1743 bytes across three settings).
/// This bounds the actual output a caller receives, and marks `truncated:
/// true` at the root when anything was cut, so a bounded-but-silently-partial
/// tree is never mistaken for the complete page.
///
/// `Value::Null` (empty snapshot) passes through unchanged — there is nothing
/// to bound.
fn bound_snapshot_output(tree: Value, max_chars: u32) -> Value {
    if tree.is_null() {
        return tree;
    }
    let full_len = serde_json::to_string(&tree).map_or(0, |s| s.len());
    if full_len <= max_chars as usize {
        return tree;
    }

    let mut budget: i64 = i64::from(max_chars);
    let mut any_pruned = false;
    // `keep_always = true`: the root's own tag/attrs are kept even if that
    // alone exceeds the budget — there must be *something* to return, so a
    // pathologically small `--max-chars` overshoots slightly rather than
    // yielding an empty tree.
    let mut bounded =
        bound_node(tree, &mut budget, &mut any_pruned, true).unwrap_or_else(|| json!({}));
    if any_pruned && let Value::Object(ref mut map) = bounded {
        map.insert("truncated".to_string(), json!(true));
    }
    bounded
}

/// Compact-JSON serialized length of `v`, as `i64` (the budget's unit).
/// Saturates to `i64::MAX` rather than wrapping on the (practically
/// unreachable) case of a multi-exabyte string.
fn json_len_i64(v: &Value) -> i64 {
    let len = serde_json::to_string(v).map_or(0, |s| s.len());
    i64::try_from(len).unwrap_or(i64::MAX)
}

/// Recursive worker for [`bound_snapshot_output`].
///
/// Tries `node` whole against `budget` first; if it fits, the whole subtree
/// is kept and `budget` decreases by exactly its serialized length (no
/// overshoot from this node is possible). Only when it doesn't fit does an
/// object node get pruned: children are admitted one at a time (also
/// whole-first) until the budget or the list runs out, and any child that
/// still doesn't fit even after pruning is dropped and `None` is returned for
/// it. A bare text leaf that doesn't fit is dropped whole (never partially
/// quoted) — the JS walker's own `--max-chars` leaf-text bounding is what
/// shrinks individual strings, not this pass.
///
/// `children_omitted` is a distinct field from the JS walker's existing
/// per-node `truncated: "<n> children not shown"` string (emitted when
/// `--depth`/`--max-depth` cuts a subtree) so the two truncation mechanisms
/// never clobber each other's marker on the same node.
///
/// Returns `None` when `node` cannot fit at all (not even pruned down to just
/// its own tag/attrs) and `keep_always` is `false` — the caller drops it.
/// `keep_always` forces `Some` regardless, for the snapshot root.
fn bound_node(
    node: Value,
    budget: &mut i64,
    any_pruned: &mut bool,
    keep_always: bool,
) -> Option<Value> {
    let whole_len = json_len_i64(&node);
    if whole_len <= *budget {
        *budget -= whole_len;
        return Some(node);
    }

    match node {
        Value::Object(mut map) => {
            let children = map.remove("children");
            let own_len = json_len_i64(&Value::Object(map.clone()));
            if own_len > *budget && !keep_always {
                return None;
            }
            *budget -= own_len;

            if let Some(Value::Array(kids)) = children {
                let total = kids.len();
                let mut kept = Vec::with_capacity(total);
                for kid in kids {
                    if *budget <= 0 {
                        break;
                    }
                    match bound_node(kid, budget, any_pruned, false) {
                        Some(bounded_kid) => kept.push(bounded_kid),
                        // This child (even pruned) doesn't fit — later
                        // siblings are no smaller in expectation, so stop
                        // admitting rather than skip-and-continue.
                        None => break,
                    }
                }
                if kept.len() < total {
                    *any_pruned = true;
                    map.insert("children_omitted".to_string(), json!(total - kept.len()));
                }
                if !kept.is_empty() {
                    map.insert("children".to_string(), Value::Array(kept));
                }
            }
            Some(Value::Object(map))
        }
        // A text leaf that doesn't fit whole is dropped rather than
        // partially quoted — see the doc comment.
        Value::String(_) if !keep_always => None,
        other => Some(other),
    }
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
    // Theme C (iter-131): root-level marker set by `bound_snapshot_output`
    // when the whole-tree --max-chars budget cut anything from the output.
    if node.get("truncated") == Some(&Value::Bool(true)) {
        println!("  [output truncated — increase --max-chars for the full tree]");
    }
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
            // Theme C (iter-131): whole-output --max-chars bounding notice —
            // distinct from the depth-limit `truncated` string above.
            if let Some(omitted) = node.get("children_omitted").and_then(Value::as_u64) {
                let _ = write!(line, " ({omitted} children not shown — max-chars)");
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

    // ── iter-141 Theme C: snapshot_truncation_flags ──────────────────────
    //
    // AC `live_141_snapshot_truncation_in_meta`: `meta` must report
    // truncation and the effective bound rather than leaving the caller to
    // find a `truncated: true` marker buried inside `results`.

    #[test]
    fn snapshot_truncation_flags_neither_truncated() {
        let results = json!({"tag": "div", "children": []});
        assert_eq!(snapshot_truncation_flags(&results), (false, false));
    }

    #[test]
    fn snapshot_truncation_flags_structure_truncated_only() {
        let results = json!({"tag": "div", "truncated": true, "children_omitted": 5});
        assert_eq!(snapshot_truncation_flags(&results), (true, false));
    }

    #[test]
    fn snapshot_truncation_flags_text_truncated_only() {
        let results = json!({"tag": "div", "textTruncated": true});
        assert_eq!(snapshot_truncation_flags(&results), (true, true));
    }

    #[test]
    fn snapshot_truncation_flags_both_truncated() {
        let results = json!({"tag": "div", "truncated": true, "textTruncated": true});
        assert_eq!(snapshot_truncation_flags(&results), (true, true));
    }

    /// A non-boolean/absent `truncated` value (e.g. the depth-limit marker's
    /// own `truncated: "<n> children not shown"` string on a *child* node,
    /// which is a different, node-scoped marker — only the root's own
    /// literal `Bool(true)` counts) must not be mistaken for `true`.
    #[test]
    fn snapshot_truncation_flags_ignores_non_bool_truncated_value() {
        let results = json!({"tag": "div", "truncated": "3 children not shown"});
        assert_eq!(snapshot_truncation_flags(&results), (false, false));
    }

    #[test]
    fn snapshot_truncation_flags_null_tree() {
        assert_eq!(snapshot_truncation_flags(&Value::Null), (false, false));
    }

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

    // ── bound_snapshot_output (Theme C, iter-131) ────────────────────────────

    /// Build a synthetic tree with `n` top-level `<div>` children, each
    /// carrying a `data-testid` attribute long enough to add real weight to
    /// the serialized output — otherwise a huge `n` would still round-trip
    /// under a small budget and the test would not exercise pruning.
    fn wide_tree(n: usize) -> Value {
        let children: Vec<Value> = (0..n)
            .map(|i| {
                json!({
                    "tag": "div",
                    "attrs": {"data-testid": format!("item-{i}-{}", "x".repeat(20))},
                    "children": ["some leaf text content here"]
                })
            })
            .collect();
        json!({"tag": "body", "children": children})
    }

    #[test]
    fn bound_snapshot_output_passthrough_under_budget() {
        let tree = wide_tree(2);
        let full_len = serde_json::to_string(&tree).unwrap().len();
        let bounded = bound_snapshot_output(tree.clone(), u32::try_from(full_len + 1000).unwrap());
        assert_eq!(
            bounded, tree,
            "small tree under budget must pass through unchanged"
        );
        assert!(bounded.get("truncated").is_none());
    }

    #[test]
    fn bound_snapshot_output_null_passthrough() {
        assert_eq!(bound_snapshot_output(Value::Null, 10), Value::Null);
    }

    #[test]
    fn bound_snapshot_output_bounds_large_tree_and_marks_truncated() {
        let tree = wide_tree(200);
        let full_len = serde_json::to_string(&tree).unwrap().len();
        let max_chars = 500u32;
        assert!(
            full_len > max_chars as usize,
            "fixture must exceed the budget to exercise pruning"
        );

        let bounded = bound_snapshot_output(tree, max_chars);
        let bounded_len = serde_json::to_string(&bounded).unwrap().len();

        assert_eq!(bounded.get("truncated"), Some(&json!(true)));
        // Slack covers the "children_omitted"/"truncated" markers, which are
        // inserted after budgeting and so aren't themselves counted against
        // it — the AC's own wording allows "± envelope overhead".
        assert!(
            bounded_len <= max_chars as usize + 100,
            "bounded output ({bounded_len} bytes) should stay close to the {max_chars}-byte budget"
        );
        // Some children must actually have been dropped.
        let kept = bounded
            .get("children")
            .and_then(Value::as_array)
            .map_or(0, Vec::len);
        assert!(
            kept < 200,
            "expected fewer than 200 children to survive pruning, got {kept}"
        );
    }

    #[test]
    fn bound_node_reports_children_omitted_distinct_from_depth_truncated() {
        // A node that already carries the JS depth-limit `truncated` string
        // must keep it untouched — the char-budget mechanism uses a
        // different field (`children_omitted`) so the two never collide.
        let node = json!({
            "tag": "div",
            "truncated": "3 children not shown",
        });
        let mut budget = 1_000_i64;
        let mut any_pruned = false;
        let out = bound_node(node, &mut budget, &mut any_pruned, false).expect("fits whole");
        assert_eq!(out.get("truncated"), Some(&json!("3 children not shown")));
        assert!(!any_pruned);
    }

    #[test]
    fn bound_node_drops_child_that_cannot_fit_even_pruned() {
        // A non-root child whose own tag/attrs alone exceed the remaining
        // budget must be dropped (`None`), not included oversized.
        let node = json!({"tag": "div", "attrs": {"data-testid": "x".repeat(500)}});
        let mut budget = 10_i64;
        let mut any_pruned = false;
        assert!(bound_node(node, &mut budget, &mut any_pruned, false).is_none());
    }
}
