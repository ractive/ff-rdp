use serde_json::{Value, json};

use crate::cli::args::Cli;
use crate::error::AppError;
use crate::hints::{HintContext, HintSource};
use crate::output;
use crate::output_controls::QueryFilter;
use crate::output_pipeline::OutputPipeline;

use super::connect_tab::connect_and_get_target;
use super::js_helpers::{STAMP_REF_JS_FN, eval_or_bail, resolve_result};

/// JavaScript IIFE that walks the DOM and returns a compact tree for LLM consumption.
///
/// `__DEPTH__` and `__MAX_CHARS__` are replaced with the actual numeric values
/// before evaluation.
const SNAPSHOT_JS_TEMPLATE: &str = r#"(function() {
  __STAMP_REF_FN__
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
  var depthCut = 0;
  var INTERACTIVE_SEL = 'a,button,input,select,textarea,details,summary';

  function isHidden(el) {
    if (el.getAttribute && el.getAttribute('aria-hidden') === 'true') return true;
    try {
      var cs = window.getComputedStyle(el);
      if (cs.display === 'none' || cs.visibility === 'hidden') return true;
    } catch(e) {}
    return false;
  }

  function clip(s, n) {
    s = String(s).replace(/\s+/g, ' ').trim();
    return s.length > n ? s.slice(0, n) + '...' : s;
  }

  // The depth cut limits the tree that is printed, not what is clickable:
  // every visible interactive element under a cut node still gets a ref,
  // listed flat on that node (dogfooding session 64 #4 — HN and react.dev
  // returned 0 refs at the default depth).
  function refsBelow(node) {
    var out = [];
    var els = node.querySelectorAll(INTERACTIVE_SEL);
    for (var i = 0; i < els.length; i++) {
      var el = els[i];
      if (el.closest('svg,script,style,noscript,[aria-hidden="true"]')) continue;
      if (isHidden(el) || el.getClientRects().length === 0) continue;
      var r = __ffrdpStampRef(el);
      if (!r) continue;
      var e = {ref: r, tag: el.tagName.toLowerCase()};
      var label = el.getAttribute('aria-label') || el.textContent || el.getAttribute('placeholder')
        || el.getAttribute('value') || el.getAttribute('title') || el.getAttribute('name') || '';
      label = clip(label, 80);
      if (label) e.text = label;
      var href = el.getAttribute('href');
      if (href) e.href = clip(href, 120);
      var type = el.getAttribute('type');
      if (type) e.type = type;
      out.push(e);
    }
    return out;
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
    // iter-210 Theme B: an interactive node gets a `--ref` handle, stamped on
    // the element itself. Only interactive nodes get one — a ref per <div>
    // would bloat the payload for nothing clickable.
    if (INTERACTIVE[tag]) { o.interactive = true; var r = __ffrdpStampRef(node); if (r) o.ref = r; }

    var a = {};
    for (var i = 0; i < KEY_ATTRS.length; i++) {
      var v = node.getAttribute(KEY_ATTRS[i]);
      if (v != null && v !== '') a[KEY_ATTRS[i]] = v.length > 200 ? v.slice(0,200)+'...' : v;
    }
    if (Object.keys(a).length) o.attrs = a;

    if (depth >= maxDepth) {
      var cc = node.children.length;
      if (cc > 0) {
        o.truncated = cc + ' children not shown';
        depthCut++;
        var below = refsBelow(node);
        if (below.length) o.refs_below = below;
      }
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
  if (tree && depthCut) { tree.depthCut = depthCut; }
  return '__FF_RDP_JSON__' + JSON.stringify(tree);
})()"#;

pub fn run(cli: &Cli, depth: u32, max_chars: u32, query: &QueryFilter) -> Result<(), AppError> {
    let mut ctx = connect_and_get_target(cli)?;
    let console_actor = ctx.target().console_actor.clone();

    // `--query` searches the whole document: a depth cut or the leaf-text cap
    // applied before the filter would hide exactly the matches the caller
    // asked for (dogfooding session 64 #31 — 0 hits at the default depth, 2 at
    // `--depth 30`). The output is still bounded by `--max-chars` below.
    let (walk_depth, walk_text_chars) = if query.is_active() {
        (QUERY_WALK_DEPTH, u32::MAX)
    } else {
        (depth, max_chars)
    };
    let js = SNAPSHOT_JS_TEMPLATE
        .replace("__STAMP_REF_FN__", STAMP_REF_JS_FN)
        .replace("__DEPTH__", &walk_depth.to_string())
        .replace("__MAX_CHARS__", &walk_text_chars.to_string());

    let eval_result = eval_or_bail(&mut ctx, &console_actor, &js, "snapshot evaluation failed")?;

    let mut results = resolve_result(&mut ctx, &eval_result.result)?;
    let depth_cut = take_depth_cut(&mut results);

    // iter-211 Theme A: `--query` prunes the tree to the matching nodes and
    // their ancestors, BEFORE the `--max-chars` bounding pass — otherwise the
    // budget would be spent on the very subtrees the caller just said they
    // did not want, and a match deep in a long document would be cut before
    // the filter ever saw it. Refs were stamped by the walk, so a survivor
    // keeps the handle it was given.
    let query_matches = if query.is_active() {
        let (pruned, matches) = prune_to_query(results, query);
        results = pruned;
        Some(matches)
    } else {
        None
    };

    // Theme C (iter-131): `--max-chars` previously bounded only leaf text
    // content — the serialized tree (tags, attrs, structure) was unbounded,
    // making the flag a near-no-op (100 vs 5000 vs default all landed within
    // a few bytes of each other, s61 #9). Bound the *whole* serialized output
    // here, on the Rust side, after the JS walk returns.
    //
    // The budget is the size of the JSON actually printed (pretty, inside the
    // envelope), not of compact JSON: indentation made a 50 KB budget print
    // 351 KB on HN at `--depth 20` (dogfooding session 64 #21).
    let results = bound_to_printed_size(results, max_chars);

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
    //
    // A depth cut counts too: it drops nodes from the printed tree, and a
    // `truncated: false` beside "6 children not shown" was a lie (dogfooding
    // session 64 #4).
    let (truncated, text_truncated) = snapshot_truncation_flags(&results);
    let mut meta = json!({
        "depth": if query.is_active() { Value::Null } else { json!(depth) },
        "max_chars": max_chars,
        "truncated": truncated || depth_cut > 0,
        "text_truncated": text_truncated,
        "depth_truncated": depth_cut > 0,
    });
    if let Some(obj) = meta.as_object_mut() {
        if let Some(matches) = query_matches {
            obj.insert("matches".to_owned(), json!(matches));
        }
        if let Some(hint) = truncation_hint(depth, depth_cut, truncated, query.is_active()) {
            obj.insert("hint".to_owned(), json!(hint));
        }
    }
    crate::connection_meta::merge_into_if_verbose(
        &mut meta,
        &cli.host,
        cli.port,
        None,
        cli.is_verbose(),
    );

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

/// Walk depth used for `--query`: deep enough for any real document, so the
/// filter sees the whole page.
const QUERY_WALK_DEPTH: u32 = 10_000;

/// Remove the walker's root-level `depthCut` counter (how many nodes the
/// `--depth` cut folded) from `tree` and return it; it belongs in `meta`.
fn take_depth_cut(tree: &mut Value) -> u64 {
    tree.as_object_mut()
        .and_then(|map| map.remove("depthCut"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0)
}

/// The `meta.hint` for a truncated snapshot, naming the flags that recover
/// what was cut; `None` when nothing was.
fn truncation_hint(
    depth: u32,
    depth_cut: u64,
    size_cut: bool,
    query_active: bool,
) -> Option<String> {
    const SIZE: &str = "output was cut at --max-chars (see `children_omitted` / `refs_omitted`); \
                        raise it, or use --query <text> to keep only matching nodes";
    const SIZE_QUERY: &str = "output was cut at --max-chars (see `children_omitted` / \
                              `refs_omitted`); raise it, or narrow --query";
    match (depth_cut > 0, size_cut) {
        (false, true) if query_active => Some(SIZE_QUERY.to_owned()),
        (true, size) => Some(format!(
            "{depth_cut} subtree(s) were folded at --depth {depth}; their interactive elements \
             are listed with refs under `refs_below`. Use --depth <n> for more structure or \
             --query <text> to search the whole document{}",
            if size {
                format!(". Also: {SIZE}")
            } else {
                String::new()
            }
        )),
        (false, true) => Some(SIZE.to_owned()),
        (false, false) => None,
    }
}

/// Bytes the default output spends on `tree`: pretty JSON, indented one level
/// as the envelope's `results` value.
fn printed_len(tree: &Value) -> usize {
    serde_json::to_string_pretty(&json!({ "results": tree })).map_or(0, |s| s.len())
}

/// Bound `tree` so its printed form ([`printed_len`]) fits `max_chars`.
///
/// [`bound_snapshot_output`] budgets compact JSON, which pretty-printing
/// inflates by a factor that depends on nesting depth, so the compact budget
/// is shrunk until the printed size fits (a few rounds at most).
fn bound_to_printed_size(tree: Value, max_chars: u32) -> Value {
    let target = max_chars as usize;
    if tree.is_null() || printed_len(&tree) <= target {
        return tree;
    }
    let mut budget = max_chars;
    let mut bounded = bound_snapshot_output(tree.clone(), budget);
    for _ in 0..8 {
        let len = printed_len(&bounded);
        if len <= target || budget <= 64 {
            break;
        }
        // Scale the compact budget by how far over the printed size is, with
        // 5% headroom so the next round lands under rather than just over.
        let scaled = u64::from(budget) * target as u64 * 95 / (len as u64 * 100);
        budget = u32::try_from(scaled)
            .unwrap_or(budget)
            .clamp(64, budget - 1);
        bounded = bound_snapshot_output(tree.clone(), budget);
    }
    bounded
}

/// Prune `tree` to the nodes matching `query` plus their ancestors, returning
/// `(pruned_tree, match_count)` (iter-211 Theme A).
///
/// A node **matches** when one of its own attribute values matches, or when
/// one of its direct text children does. A matching node is kept **whole**,
/// subtree included — the point of `snapshot --query "1804"` on a table is to
/// get that row/cell with its contents, not a stripped shell of it. A
/// non-matching node survives only as a path to a match: its own tag, role
/// and attributes are kept and its children are replaced by the surviving
/// ones, so the root stays `html` and the caller can see where each hit sits.
///
/// A tree with no match at all prunes to `Value::Null` — `total: 0`,
/// `meta.matches: 0`. Returning the unpruned tree instead would be the worse
/// lie: an agent that asked for "billion" and got the whole page back would
/// read it as "here are your matches".
fn prune_to_query(tree: Value, query: &QueryFilter) -> (Value, usize) {
    let mut matches = 0usize;
    let pruned = prune_node(tree, query, &mut matches).unwrap_or(Value::Null);
    (pruned, matches)
}

/// Whether this node's own attributes or direct text children match.
///
/// Deliberately shallow: a deep test would report every ancestor of a hit as
/// a hit itself, which would both inflate `meta.matches` and keep the whole
/// document (the root is an ancestor of everything).
fn node_matches_query(map: &serde_json::Map<String, Value>, query: &QueryFilter) -> bool {
    if let Some(attrs) = map.get("attrs")
        && query.matches_shallow(attrs)
    {
        return true;
    }
    match map.get("children") {
        Some(Value::Array(kids)) => kids.iter().any(|kid| match kid {
            Value::String(text) => query.matches(text),
            _ => false,
        }),
        _ => false,
    }
}

/// Recursive worker for [`prune_to_query`]. `None` means "nothing under here
/// matched" and the caller drops the node.
fn prune_node(node: Value, query: &QueryFilter, matches: &mut usize) -> Option<Value> {
    let Value::Object(mut map) = node else {
        // A bare text leaf is judged by its parent in `node_matches_query` —
        // a matching string is what makes the element holding it a match, and
        // a naked string with no tag around it is not a useful result.
        return None;
    };
    if node_matches_query(&map, query) {
        *matches += 1;
        return Some(Value::Object(map));
    }
    let children = map.remove("children");
    let mut kept: Vec<Value> = Vec::new();
    if let Some(Value::Array(kids)) = children {
        for kid in kids {
            if let Some(kept_kid) = prune_node(kid, query, matches) {
                kept.push(kept_kid);
            }
        }
    }
    if kept.is_empty() {
        return None;
    }
    map.insert("children".to_owned(), Value::Array(kept));
    Some(Value::Object(map))
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
            let refs_below = map.remove("refs_below");
            let own_len = json_len_i64(&Value::Object(map.clone()));
            if own_len > *budget && !keep_always {
                return None;
            }
            *budget -= own_len;

            // A folded node's flat ref list is trimmed entry by entry like
            // children, so one long list cannot drop the whole node.
            if let Some(Value::Array(refs)) = refs_below {
                let total = refs.len();
                let mut kept = Vec::with_capacity(total);
                for entry in refs {
                    let len = json_len_i64(&entry) + 1;
                    if len > *budget {
                        break;
                    }
                    *budget -= len;
                    kept.push(entry);
                }
                if kept.len() < total {
                    *any_pruned = true;
                    map.insert("refs_omitted".to_string(), json!(total - kept.len()));
                }
                if !kept.is_empty() {
                    map.insert("refs_below".to_string(), Value::Array(kept));
                }
            }

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
            if let Some(r) = node.get("ref").and_then(Value::as_str) {
                let _ = write!(line, " ref={r}");
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

            if let Some(Value::Array(refs)) = node.get("refs_below") {
                for entry in refs {
                    let r = entry.get("ref").and_then(Value::as_str).unwrap_or("?");
                    let t = entry.get("tag").and_then(Value::as_str).unwrap_or("?");
                    let mut ref_line = format!("{indent}  - {r} <{t}>");
                    if let Some(text) = entry.get("text").and_then(Value::as_str) {
                        let _ = write!(ref_line, " {text:?}");
                    }
                    println!("{ref_line}");
                }
            }

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

    // ── iter-211 Theme A: `snapshot --query` ────────────────────────────────

    fn query(text: &str) -> QueryFilter {
        QueryFilter::from_query_args(&crate::cli::args::QueryArgs {
            query: Some(text.to_owned()),
            query_regex: None,
        })
    }

    /// A three-row table nested under `html > body > table`, the shape the
    /// benchmark's `tabular_data_analysis` task actually walks.
    fn table_tree() -> Value {
        json!({
            "tag": "html",
            "children": [{
                "tag": "body",
                "children": [
                    {"tag": "h1", "children": ["World population"]},
                    {"tag": "table", "children": [
                        {"tag": "tr", "children": [
                            {"tag": "td", "children": ["1804"]},
                            {"tag": "td", "children": ["1 billion"]}
                        ]},
                        {"tag": "tr", "children": [
                            {"tag": "td", "children": ["1927"]},
                            {"tag": "td", "children": ["2 billion"]}
                        ]}
                    ]}
                ]
            }]
        })
    }

    /// AC `live_snapshot_query_keeps_ancestors_of_matches`, in unit form: the
    /// root stays `html` and the surviving leaf is the matching cell.
    #[test]
    fn unit_211_query_keeps_ancestors_and_prunes_siblings() {
        let (pruned, matches) = prune_to_query(table_tree(), &query("1804"));
        assert_eq!(matches, 1);
        assert_eq!(pruned["tag"], "html", "the root must survive: {pruned}");
        let body = &pruned["children"][0];
        assert_eq!(body["tag"], "body");
        // The <h1> sibling and the second <tr> are gone; only the path to the
        // hit remains.
        assert_eq!(body["children"].as_array().map(Vec::len), Some(1));
        let table = &body["children"][0];
        assert_eq!(table["tag"], "table");
        assert_eq!(table["children"].as_array().map(Vec::len), Some(1));
        let row = &table["children"][0];
        assert_eq!(row["children"].as_array().map(Vec::len), Some(1));
        let cell = &row["children"][0];
        assert_eq!(cell["tag"], "td");
        assert_eq!(cell["children"][0], "1804");
    }

    /// A matching node is kept whole — `--query billion` on the table returns
    /// both cells' contents, not a stripped `<td>` shell.
    #[test]
    fn unit_211_matching_node_keeps_its_subtree() {
        let (pruned, matches) = prune_to_query(table_tree(), &query("billion"));
        assert_eq!(matches, 2, "one cell per row: {pruned}");
        let table = &pruned["children"][0]["children"][0];
        let rows = table["children"].as_array().expect("both rows survive");
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0]["children"][0]["children"][0], "1 billion");
    }

    /// Attribute values match too, so "find the link to /babbage" works
    /// without knowing the link's text.
    #[test]
    fn unit_211_attribute_values_match() {
        let tree = json!({
            "tag": "html",
            "children": [{"tag": "a", "attrs": {"href": "/babbage"}, "children": ["Charles"]}]
        });
        let (pruned, matches) = prune_to_query(tree, &query("babbage"));
        assert_eq!(matches, 1);
        assert_eq!(pruned["children"][0]["attrs"]["href"], "/babbage");
    }

    /// No match prunes to `null` rather than quietly handing back the whole
    /// page, which an agent would read as "here are your matches".
    #[test]
    fn unit_211_no_match_yields_null_not_the_whole_tree() {
        let (pruned, matches) = prune_to_query(table_tree(), &query("no-such-token"));
        assert_eq!(matches, 0);
        assert_eq!(pruned, Value::Null);
    }

    /// The match test is shallow: an ancestor is kept as a path, but is not
    /// itself counted as a match — otherwise `html` would match everything
    /// and `meta.matches` would be meaningless.
    #[test]
    fn unit_211_ancestors_are_not_counted_as_matches() {
        let (_, matches) = prune_to_query(table_tree(), &query("1 billion"));
        assert_eq!(matches, 1);
    }
    // ── dogfooding session 64 #4 / #21: depth cut and printed budget ──────

    #[test]
    fn take_depth_cut_moves_the_counter_out_of_the_tree() {
        let mut tree = json!({"tag": "html", "depthCut": 3});
        assert_eq!(take_depth_cut(&mut tree), 3);
        assert!(tree.get("depthCut").is_none());
        assert_eq!(take_depth_cut(&mut json!({"tag": "html"})), 0);
        assert_eq!(take_depth_cut(&mut Value::Null), 0);
    }

    #[test]
    fn truncation_hint_names_the_flags_that_recover_the_cut() {
        let depth = truncation_hint(6, 2, false, false).expect("depth cut has a hint");
        assert!(
            depth.contains("--depth 6") && depth.contains("--query"),
            "{depth}"
        );
        assert!(depth.contains("refs_below"), "{depth}");
        assert!(!depth.contains("--max-chars"), "{depth}");
        let both = truncation_hint(6, 2, true, false).expect("hint");
        assert!(both.contains("--max-chars"), "{both}");
        let size = truncation_hint(6, 0, true, false).expect("size cut has a hint");
        assert!(size.contains("--max-chars"), "{size}");
        assert_eq!(truncation_hint(6, 0, false, false), None);
        let query = truncation_hint(6, 0, true, true).expect("hint");
        assert!(query.contains("narrow --query"), "{query}");
    }

    /// A tree nested deep enough that indentation dominates its pretty form.
    fn deep_wide_tree() -> Value {
        let mut node = wide_tree(300);
        for _ in 0..15 {
            node = json!({"tag": "div", "children": [node]});
        }
        node
    }

    #[test]
    fn bound_to_printed_size_bounds_the_pretty_output() {
        let tree = deep_wide_tree();
        assert!(
            printed_len(&tree) > 100_000,
            "fixture must be far over budget"
        );
        let bounded = bound_to_printed_size(tree, 20_000);
        let len = printed_len(&bounded);
        assert!(len <= 20_000, "printed {len} bytes for a 20000 budget");
        assert_eq!(bounded.get("truncated"), Some(&json!(true)));
    }

    #[test]
    fn bound_to_printed_size_passes_a_small_tree_through() {
        let tree = wide_tree(2);
        assert_eq!(bound_to_printed_size(tree.clone(), 50_000), tree);
    }

    #[test]
    fn bound_node_trims_refs_below_instead_of_dropping_the_node() {
        let refs: Vec<Value> = (0..200)
            .map(|i| json!({"ref": format!("e{}", 100_001 + i), "tag": "a", "text": "a link"}))
            .collect();
        let node = json!({"tag": "td", "truncated": "1 children not shown", "refs_below": refs});
        let mut budget = 2_000_i64;
        let mut any_pruned = false;
        let out = bound_node(node, &mut budget, &mut any_pruned, false).expect("node is kept");
        let kept = out["refs_below"].as_array().map_or(0, Vec::len);
        assert!(kept > 0 && kept < 200, "kept {kept} refs");
        assert_eq!(out["refs_omitted"], json!(200 - kept));
        assert!(any_pruned);
    }

    #[test]
    fn snapshot_js_lists_refs_below_a_depth_cut() {
        assert!(SNAPSHOT_JS_TEMPLATE.contains("o.refs_below = below"));
        assert!(SNAPSHOT_JS_TEMPLATE.contains("tree.depthCut = depthCut"));
    }
}
