use ff_rdp_core::{Grip, LongStringActor};
use serde_json::{Value, json};

use crate::cli::args::Cli;
use crate::error::AppError;
use crate::hints::{HintContext, HintSource};
use crate::output;
use crate::output_controls::{OutputControls, SortDir};
use crate::output_pipeline::OutputPipeline;

use super::connect_tab::connect_and_get_target;
use super::js_helpers::{JSON_SENTINEL, escape_selector, eval_or_bail, resolve_result};

#[derive(Debug, Clone, Copy)]
pub enum OutputMode {
    /// ARIA-tree JSON (default, iter-60+). Each node is `{ref,role,name,level,state,tag,attrs}`.
    AriaTree,
    /// Raw outer HTML strings (--format html escape hatch).
    OuterHtml,
    /// Raw inner HTML strings (--format html + --inner-html).
    InnerHtml,
    /// Text content only.
    Text,
    /// Attributes only as JSON objects.
    Attrs,
    /// Both text content and attributes per element.
    TextAttrs,
}

/// JavaScript IIFE that extracts an ARIA-tree record for a single element.
///
/// The ref ID is injected by the Rust caller as a counter (`__REF_START__`).
/// Actionable attributes only: id, name, type, href, aria-*, data-state, role,
/// placeholder, value (for inputs).
const ARIA_TREE_JS_TEMPLATE: &str = r"(function() {
  var ACTIONABLE_ATTRS = ['id','name','type','href','placeholder','value',
    'aria-label','aria-expanded','aria-hidden','aria-haspopup','aria-selected',
    'aria-checked','aria-disabled','aria-controls','aria-describedby',
    'aria-labelledby','aria-live','aria-atomic','aria-busy','aria-current',
    'aria-invalid','aria-multiline','aria-multiselectable','aria-orientation',
    'aria-pressed','aria-readonly','aria-required','aria-sort','aria-valuemax',
    'aria-valuemin','aria-valuenow','aria-valuetext','data-state','role'];
  var SEMANTIC_ROLES = {NAV:'navigation',HEADER:'banner',FOOTER:'contentinfo',
    MAIN:'main',ASIDE:'complementary',ARTICLE:'article',SECTION:'region',
    FORM:'form',DIALOG:'dialog',SEARCH:'search',H1:'heading',H2:'heading',
    H3:'heading',H4:'heading',H5:'heading',H6:'heading'};
  var HEADING_LEVELS = {H1:1,H2:2,H3:3,H4:4,H5:5,H6:6};
  var refCounter = __REF_START__;
  var els = document.querySelectorAll('__SELECTOR__');
  if (els.length === 0) return null;
  var results = [];
  for (var i = 0; i < els.length; i++) {
    var el = els[i];
    var tag = el.tagName;
    var role = el.getAttribute('role') || SEMANTIC_ROLES[tag] || null;
    var name = el.getAttribute('aria-label') ||
               el.getAttribute('alt') ||
               (el.textContent || '').trim().slice(0, 100) || null;
    var level = HEADING_LEVELS[tag] || null;
    var state = {};
    var ariaExpanded = el.getAttribute('aria-expanded');
    if (ariaExpanded !== null) state.expanded = ariaExpanded === 'true';
    var ariaDisabled = el.getAttribute('aria-disabled');
    if (ariaDisabled !== null) state.disabled = ariaDisabled === 'true';
    var ariaSelected = el.getAttribute('aria-selected');
    if (ariaSelected !== null) state.selected = ariaSelected === 'true';
    var ariaChecked = el.getAttribute('aria-checked');
    if (ariaChecked !== null) state.checked = ariaChecked === 'true';
    var attrs = {};
    for (var j = 0; j < ACTIONABLE_ATTRS.length; j++) {
      var attrName = ACTIONABLE_ATTRS[j];
      if (attrName === 'aria-expanded' || attrName === 'aria-disabled' ||
          attrName === 'aria-selected' || attrName === 'aria-checked') continue;
      var v = el.getAttribute(attrName);
      if (v !== null && v !== '') attrs[attrName] = v;
    }
    var refId = 'e' + (refCounter + i);
    var node = {'ref': refId, 'tag': tag.toLowerCase()};
    if (role) node.role = role;
    if (name) node.name = name;
    if (level !== null) node.level = level;
    if (Object.keys(state).length) node.state = state;
    if (Object.keys(attrs).length) node.attrs = attrs;
    // Shadow-DOM hints: flag host elements so callers know a shadow tree is
    // attached. shadowRoot is null for closed roots from content-scope JS,
    // so we check el.openOrClosedShadowRoot (chrome-privileged) when available,
    // then fall back to el.shadowRoot (open only).
    var sr = (typeof el.openOrClosedShadowRoot !== 'undefined')
      ? el.openOrClosedShadowRoot : el.shadowRoot;
    if (sr) {
      node.hasShadowRoot = true;
      node.shadowMode = sr.mode || 'open';
    }
    // Resolver expression: re-selects this element by its querySelectorAll index.
    node.__resolver = 'document.querySelectorAll(\'__SELECTOR__\')[' + i + ']';
    results.push(node);
  }
  if (results.length === 1) return '__FF_RDP_JSON__' + JSON.stringify(results[0]);
  return '__FF_RDP_JSON__' + JSON.stringify(results);
})()";

pub fn run(
    cli: &Cli,
    selector: &str,
    mode: OutputMode,
    first: bool,
    style_props: &[String],
    style_limit: usize,
) -> Result<(), AppError> {
    let mut ctx = connect_and_get_target(cli)?;
    let console_actor = ctx.target.console_actor.clone();

    // Determine effective output mode: --format html overrides to raw HTML.
    let effective_mode = if cli.format == "html" {
        match mode {
            OutputMode::InnerHtml => OutputMode::InnerHtml,
            _ => OutputMode::OuterHtml,
        }
    } else {
        mode
    };

    // For AriaTree mode in daemon context, pre-allocate ref IDs so the JS
    // uses stable, globally-unique handles across successive dom calls.
    // In --no-daemon mode, the JS falls back to a fixed start of 1 (local
    // to this invocation only — refs are not persisted between processes).
    let (ref_start, ref_nav_gen) =
        if ctx.via_daemon && matches!(effective_mode, OutputMode::AriaTree) {
            // Estimate element count conservatively — alloc 256 slots.  The JS
            // will only use as many as it finds; extra slots are wasted but that
            // is harmless since the counter just advances past them.
            match crate::daemon::client::alloc_refs(ctx.transport_mut(), 256) {
                Ok((start, nav_gen)) => (start, Some(nav_gen)),
                Err(_) => (1, None), // non-fatal: fall back to local counter
            }
        } else {
            (1, None)
        };

    let js = build_js_with_ref_start(selector, effective_mode, ref_start);

    let eval_result = eval_or_bail(&mut ctx, &console_actor, &js, "DOM query failed")?;

    let mut results = resolve_result(&mut ctx, &eval_result.result)?;

    // In AriaTree mode: strip `__resolver` fields from output (implementation
    // detail). When running via daemon with a successful alloc, also register
    // each ref with the daemon so `--ref e<N>` resolves later. When no daemon
    // is backing the refs (--no-daemon, or alloc failed), strip the `ref`
    // field entirely so callers don't see ref handles they can't use.
    let mut refs_registered = false;
    if matches!(effective_mode, OutputMode::AriaTree) {
        let entries = extract_and_strip_resolvers(&mut results);
        if let Some(nav_gen) = ref_nav_gen
            && !entries.is_empty()
        {
            if crate::daemon::client::register_refs(ctx.transport_mut(), nav_gen, &entries).is_ok()
            {
                refs_registered = true;
            } else {
                // Registration failed (e.g. page navigated mid-call).
                // Strip ref handles since they are guaranteed not to resolve.
                strip_ref_field(&mut results);
            }
        } else if !ctx.via_daemon || ref_nav_gen.is_none() {
            // No daemon backing — refs would be inert. Remove them.
            strip_ref_field(&mut results);
        }
    }

    // Theme D (iter-80) `--include-style`: after the primary DOM query has
    // resolved matches, run a second JS evaluation that pulls
    // getComputedStyle values for each match by querySelectorAll index and
    // merge them into the result objects. Done as a separate roundtrip so the
    // ARIA-tree JS stays focused and so callers without `--include-style` pay
    // no cost.
    let mut style_truncated = false;
    if !style_props.is_empty() && matches!(effective_mode, OutputMode::AriaTree) {
        let match_count = match &results {
            Value::Array(arr) => arr.len(),
            Value::Object(_) => 1,
            _ => 0,
        };
        if match_count > 0 {
            let take = match_count.min(style_limit);
            if match_count > style_limit {
                style_truncated = true;
            }
            let styles =
                fetch_computed_styles(&mut ctx, &console_actor, selector, style_props, take);
            attach_styles(&mut results, &styles);
        }
    }

    let mut meta = json!({"selector": selector});
    if matches!(effective_mode, OutputMode::AriaTree)
        && ctx.via_daemon
        && let Some(obj) = meta.as_object_mut()
    {
        // Always emit refs_registered so callers can reliably check whether
        // ref handles in the output are usable (iter-61j D1).
        obj.insert("refs_registered".to_string(), json!(refs_registered));
    }
    if style_truncated && let Some(obj) = meta.as_object_mut() {
        obj.insert("style_truncated".to_string(), json!(true));
        obj.insert("style_limit".to_string(), json!(style_limit));
    }
    crate::connection_meta::merge_into_if_verbose(
        &mut meta,
        &cli.host,
        cli.port,
        None,
        cli.is_verbose(),
    );

    // Normalise to an array unconditionally (dogfood-49 #3): every `dom`
    // call now returns `results` as an array regardless of match count so
    // agent `--jq '.results[0]'` recipes are no longer footguns.
    //
    // - null  → []          (no matches)
    // - {…}   → [{…}]       (single match, wrapped)
    // - [a,b] → [a,b]       (multi-match, unchanged)
    //
    // `--first` reverts to the legacy single-element-object shape for
    // callers who explicitly want it.
    let items_array: Vec<Value> = match results {
        Value::Null => Vec::new(),
        Value::Array(arr) => arr,
        single => vec![single],
    };

    // Apply output controls (sort/limit/fields) uniformly on the array.
    let controls = OutputControls::from_cli(cli, SortDir::Asc);
    let mut items = items_array;
    controls.apply_sort(&mut items);
    let (limited, total, truncated) = controls.apply_limit(items, Some(20));
    let shown = limited.len();
    let limited = controls.apply_fields(limited);

    let hint_ctx = HintContext::new(HintSource::Dom).with_selector(selector);

    if first {
        // Legacy shape: return the first element as a single value (object
        // in ARIA-tree mode, string in --text / --format html modes, null
        // when no match). `total` reflects the *post-limit* count, capped at 1.
        let first = limited.into_iter().next().unwrap_or(Value::Null);
        let total = usize::from(!matches!(first, Value::Null));
        let envelope = output::envelope(&first, total, &meta);
        return OutputPipeline::from_cli(cli)?
            .finalize_with_hints(&envelope, Some(&hint_ctx))
            .map_err(AppError::from);
    }

    let envelope =
        output::envelope_with_truncation(&json!(limited), shown, total, truncated, &meta);
    OutputPipeline::from_cli(cli)?
        .finalize_with_hints(&envelope, Some(&hint_ctx))
        .map_err(AppError::from)
}

/// Extract `__resolver` fields from ARIA-tree results and return them as
/// `RefEntry` pairs.  The `__resolver` field is removed from each node in
/// place — it is an implementation detail and must not appear in output.
fn extract_and_strip_resolvers(results: &mut Value) -> Vec<crate::daemon::client::RefEntry> {
    let mut entries = Vec::new();

    match results {
        Value::Object(map) => {
            if let (Some(Value::String(id)), Some(Value::String(resolver))) =
                (map.get("ref").cloned(), map.remove("__resolver"))
            {
                entries.push(crate::daemon::client::RefEntry { id, resolver });
            }
        }
        Value::Array(arr) => {
            for node in arr.iter_mut() {
                if let Value::Object(map) = node
                    && let (Some(Value::String(id)), Some(Value::String(resolver))) =
                        (map.get("ref").cloned(), map.remove("__resolver"))
                {
                    entries.push(crate::daemon::client::RefEntry { id, resolver });
                }
            }
        }
        _ => {}
    }

    entries
}

/// Fetch computed CSS values for the first `take` matches of `selector` and
/// return them as a parallel array `[{prop: value, ...}, ...]` of length
/// `take`.  Used by `dom --include-style` (Theme D, iter-80).
///
/// Returns an empty Vec on JS exception or parse failure rather than failing
/// the whole `dom` call — `--include-style` is best-effort polish, not core.
fn fetch_computed_styles(
    ctx: &mut super::connect_tab::ConnectedTab,
    console_actor: &ff_rdp_core::ActorId,
    selector: &str,
    props: &[String],
    take: usize,
) -> Vec<Value> {
    let escaped = escape_selector(selector);
    let props_json = serde_json::to_string(props).unwrap_or_else(|_| "[]".to_owned());
    let js = format!(
        r"(function() {{
  var props = {props_json};
  var take = {take};
  var els = document.querySelectorAll('{escaped}');
  var out = [];
  var n = Math.min(els.length, take);
  for (var i = 0; i < n; i++) {{
    var cs = getComputedStyle(els[i]);
    var obj = {{}};
    for (var j = 0; j < props.length; j++) {{
      var p = props[j];
      obj[p] = cs.getPropertyValue(p).trim() || cs[p] || '';
    }}
    out.push(obj);
  }}
  return '{JSON_SENTINEL}' + JSON.stringify(out);
}})()"
    );

    let Ok(eval_result) = eval_or_bail(ctx, console_actor, &js, "include-style query failed")
    else {
        return Vec::new();
    };
    let Ok(parsed) = resolve_result(ctx, &eval_result.result) else {
        return Vec::new();
    };
    match parsed {
        Value::Array(arr) => arr,
        _ => Vec::new(),
    }
}

/// Attach a `style` field to each entry in `results` from the parallel
/// `styles` array.  Entries beyond `styles.len()` are left untouched (so
/// `--include-style-limit` capping is visible to callers).
fn attach_styles(results: &mut Value, styles: &[Value]) {
    match results {
        Value::Object(map) => {
            if let Some(s) = styles.first() {
                map.insert("style".to_string(), s.clone());
            }
        }
        Value::Array(arr) => {
            for (i, node) in arr.iter_mut().enumerate() {
                if let (Value::Object(map), Some(s)) = (node, styles.get(i)) {
                    map.insert("style".to_string(), s.clone());
                }
            }
        }
        _ => {}
    }
}

/// Remove the `ref` field from each ARIA-tree node.  Used when refs cannot be
/// resolved later (no daemon, or registration failed) — emitting handles that
/// don't work would mislead agent callers.
fn strip_ref_field(results: &mut Value) {
    match results {
        Value::Object(map) => {
            map.remove("ref");
        }
        Value::Array(arr) => {
            for node in arr.iter_mut() {
                if let Value::Object(map) = node {
                    map.remove("ref");
                }
            }
        }
        _ => {}
    }
}

pub fn run_count(cli: &Cli, selector: &str) -> Result<(), AppError> {
    let mut ctx = connect_and_get_target(cli)?;
    let console_actor = ctx.target.console_actor.clone();

    let escaped = escape_selector(selector);
    let js = format!("document.querySelectorAll('{escaped}').length");

    let eval_result = eval_or_bail(&mut ctx, &console_actor, &js, "DOM count query failed")?;

    let count = match &eval_result.result {
        Grip::Value(v) => v.as_u64().unwrap_or(0),
        _ => 0,
    };

    let results = json!({"selector": selector, "count": count});
    let mut meta = json!({"selector": selector});
    crate::connection_meta::merge_into_if_verbose(
        &mut meta,
        &cli.host,
        cli.port,
        None,
        cli.is_verbose(),
    );
    let envelope = output::envelope(&results, usize::try_from(count).unwrap_or(0), &meta);

    let hint_ctx = HintContext::new(HintSource::Dom).with_selector(selector);
    OutputPipeline::from_cli(cli)?
        .finalize_with_hints(&envelope, Some(&hint_ctx))
        .map_err(AppError::from)
}

/// Wrapper used in tests (ref start defaults to 1, matching --no-daemon behaviour).
#[cfg(test)]
fn build_js(selector: &str, mode: OutputMode) -> String {
    build_js_with_ref_start(selector, mode, 1)
}

fn build_js_with_ref_start(selector: &str, mode: OutputMode, ref_start: u64) -> String {
    let escaped = escape_selector(selector);

    // Multi-element results and attrs are JSON.stringify'd with a sentinel
    // prefix so resolve_result can distinguish them from plain text that
    // happens to look like JSON.
    match mode {
        OutputMode::AriaTree => {
            // Replace the selector placeholder and inject the ref counter start.
            // In daemon mode the caller passes a globally-unique start value
            // from alloc_refs; in --no-daemon mode it defaults to 1.
            ARIA_TREE_JS_TEMPLATE
                .replace("__SELECTOR__", &escaped)
                .replace("__REF_START__", &ref_start.to_string())
        }
        OutputMode::OuterHtml => format!(
            r"(function() {{
  var els = document.querySelectorAll('{escaped}');
  if (els.length === 0) return null;
  if (els.length === 1) return els[0].outerHTML;
  return '{JSON_SENTINEL}' + JSON.stringify(Array.from(els, function(e) {{ return e.outerHTML; }}));
}})()"
        ),
        OutputMode::InnerHtml => format!(
            r"(function() {{
  var els = document.querySelectorAll('{escaped}');
  if (els.length === 0) return null;
  if (els.length === 1) return els[0].innerHTML;
  return '{JSON_SENTINEL}' + JSON.stringify(Array.from(els, function(e) {{ return e.innerHTML; }}));
}})()"
        ),
        OutputMode::Text => format!(
            r"(function() {{
  var els = document.querySelectorAll('{escaped}');
  if (els.length === 0) return null;
  if (els.length === 1) return els[0].textContent;
  return '{JSON_SENTINEL}' + JSON.stringify(Array.from(els, function(e) {{ return e.textContent; }}));
}})()"
        ),
        OutputMode::Attrs => format!(
            r"(function() {{
  function attrs(e) {{
    var o = {{}};
    for (var i = 0; i < e.attributes.length; i++) {{
      o[e.attributes[i].name] = e.attributes[i].value;
    }}
    return o;
  }}
  var els = document.querySelectorAll('{escaped}');
  if (els.length === 0) return null;
  if (els.length === 1) return '{JSON_SENTINEL}' + JSON.stringify(attrs(els[0]));
  return '{JSON_SENTINEL}' + JSON.stringify(Array.from(els, attrs));
}})()"
        ),
        OutputMode::TextAttrs => format!(
            r"(function() {{
  function textAttrs(e) {{
    var o = {{}};
    for (var i = 0; i < e.attributes.length; i++) {{
      o[e.attributes[i].name] = e.attributes[i].value;
    }}
    return {{textContent: e.textContent, attrs: o}};
  }}
  var els = document.querySelectorAll('{escaped}');
  if (els.length === 0) return null;
  if (els.length === 1) return '{JSON_SENTINEL}' + JSON.stringify(textAttrs(els[0]));
  return '{JSON_SENTINEL}' + JSON.stringify(Array.from(els, textAttrs));
}})()"
        ),
    }
}

/// Build the JavaScript IIFE that collects DOM statistics in a single evaluation.
///
/// The render-blocking predicate is sourced from `crate::render_blocking::RENDER_BLOCKING_JS_PREDICATE`
/// so that `dom stats` and `perf audit` always use the same rules.
/// See `crate::render_blocking::classify` for the Rust-side source-of-truth and rule documentation.
fn build_stats_js() -> String {
    let predicate = crate::render_blocking::RENDER_BLOCKING_JS_PREDICATE;
    format!(
        r"(function() {{
  var nodeCount = document.getElementsByTagName('*').length;
  var docSize = document.documentElement.outerHTML.length;
  var scripts = document.getElementsByTagName('script');
  var inlineScriptCount = 0;
  for (var i = 0; i < scripts.length; i++) {{
    if (!scripts[i].getAttribute('src')) inlineScriptCount++;
  }}
  var isRenderBlocking = {predicate};
  var renderBlockingCount = 0;
  document.querySelectorAll('link, script').forEach(function(el) {{
    if (isRenderBlocking(el)) renderBlockingCount++;
  }});
  var imgs = document.getElementsByTagName('img');
  var imagesWithoutLazy = 0;
  for (var m = 0; m < imgs.length; m++) {{
    var img = imgs[m];
    var rect = img.getBoundingClientRect();
    var inViewport = rect.top < window.innerHeight && rect.bottom >= 0;
    if (!inViewport && img.getAttribute('loading') !== 'lazy') imagesWithoutLazy++;
  }}
  return JSON.stringify({{
    node_count: nodeCount,
    document_size: docSize,
    inline_script_count: inlineScriptCount,
    render_blocking_count: renderBlockingCount,
    images_without_lazy: imagesWithoutLazy
  }});
}})()"
    )
}

pub fn run_stats(cli: &Cli) -> Result<(), AppError> {
    let mut ctx = connect_and_get_target(cli)?;
    let console_actor = ctx.target.console_actor.clone();

    let stats_js = build_stats_js();
    let eval_result = eval_or_bail(
        &mut ctx,
        &console_actor,
        &stats_js,
        "DOM stats query failed",
    )?;

    let json_str = match &eval_result.result {
        Grip::Value(Value::String(s)) => s.clone(),
        Grip::LongString {
            actor,
            length,
            initial: _,
        } => LongStringActor::full_string(ctx.transport_mut(), actor.as_ref(), *length)
            .map_err(AppError::from)?,
        Grip::Null | Grip::Undefined => {
            return Err(AppError::User("DOM stats returned no result".to_string()));
        }
        other => {
            return Err(AppError::User(format!(
                "unexpected DOM stats result type: {:?}",
                other.to_json()
            )));
        }
    };

    let stats: Value = serde_json::from_str(&json_str)
        .map_err(|e| AppError::from(anyhow::anyhow!("failed to parse DOM stats JSON: {e}")))?;

    let mut meta = json!({});
    crate::connection_meta::merge_into_if_verbose(
        &mut meta,
        &cli.host,
        cli.port,
        None,
        cli.is_verbose(),
    );
    let envelope = output::envelope(&stats, 1, &meta);

    let hint_ctx = HintContext::new(HintSource::DomStats);
    OutputPipeline::from_cli(cli)?
        .finalize_with_hints(&envelope, Some(&hint_ctx))
        .map_err(AppError::from)
}

#[cfg(test)]
mod tests {
    use super::super::js_helpers::escape_selector;
    use super::*;

    #[test]
    fn build_js_outer_html() {
        let js = build_js("h1", OutputMode::OuterHtml);
        assert!(js.contains("querySelectorAll('h1')"));
        assert!(js.contains("outerHTML"));
    }

    #[test]
    fn build_js_text() {
        let js = build_js(".content", OutputMode::Text);
        assert!(js.contains("textContent"));
    }

    #[test]
    fn build_js_attrs() {
        let js = build_js("a", OutputMode::Attrs);
        assert!(js.contains("attributes"));
    }

    #[test]
    fn build_js_inner_html() {
        let js = build_js("div", OutputMode::InnerHtml);
        assert!(js.contains("innerHTML"));
    }

    #[test]
    fn build_js_escapes_selector() {
        let js = build_js("div[data-name='test']", OutputMode::Text);
        // Single quotes are now escaped for safe embedding in '…' JS literals.
        assert!(js.contains(r"div[data-name=\'test\']"));
    }

    #[test]
    fn escape_selector_handles_special_chars() {
        // Newlines and backslashes should be escaped
        assert_eq!(escape_selector("a\nb"), r"a\nb");
        assert_eq!(escape_selector(r"a\b"), r"a\\b");
        // Double quotes are escaped (embedded in single-quoted JS literal)
        assert_eq!(escape_selector(r#"a"b"#), r#"a\"b"#);
    }

    #[test]
    fn build_js_multi_uses_sentinel() {
        let js = build_js("li", OutputMode::Text);
        assert!(js.contains(JSON_SENTINEL));
    }

    #[test]
    fn build_count_js() {
        let escaped = escape_selector("script");
        let js = format!("document.querySelectorAll('{escaped}').length");
        assert!(js.contains("querySelectorAll('script')"));
        assert!(js.contains(".length"));
    }

    #[test]
    fn build_js_text_attrs() {
        let js = build_js("a", OutputMode::TextAttrs);
        assert!(js.contains("querySelectorAll('a')"));
        assert!(js.contains("textContent"));
        assert!(js.contains("attributes"));
        assert!(js.contains("textAttrs"));
        // Returns a JSON object with textContent and attrs fields
        assert!(js.contains("\"attrs\"") || js.contains("attrs:"));
        assert!(js.contains(JSON_SENTINEL));
    }

    #[test]
    fn build_js_text_attrs_single_uses_sentinel() {
        let js = build_js("h1", OutputMode::TextAttrs);
        // Single-element path must also use the sentinel so resolve_result
        // can parse it as JSON rather than treating it as a plain string.
        assert!(js.contains(JSON_SENTINEL));
        assert!(js.contains("textAttrs(els[0])"));
    }

    #[test]
    fn build_js_text_attrs_multi_uses_array_from() {
        let js = build_js("li", OutputMode::TextAttrs);
        assert!(js.contains("Array.from(els, textAttrs)"));
    }

    // ── iter-60 ARIA-tree mode ───────────────────────────────────────────────

    #[test]
    fn build_js_aria_tree_uses_sentinel() {
        let js = build_js("button", OutputMode::AriaTree);
        assert!(
            js.contains(JSON_SENTINEL),
            "ARIA-tree JS must include sentinel: {js}"
        );
    }

    #[test]
    fn build_js_aria_tree_contains_selector() {
        let js = build_js("button.submit", OutputMode::AriaTree);
        assert!(
            js.contains("button.submit"),
            "ARIA-tree JS must embed selector: {js}"
        );
    }

    #[test]
    fn build_js_aria_tree_includes_role_extraction() {
        let js = build_js("h1", OutputMode::AriaTree);
        assert!(js.contains("role"), "ARIA-tree JS must extract role: {js}");
        assert!(js.contains("name"), "ARIA-tree JS must extract name: {js}");
        assert!(
            js.contains("level"),
            "ARIA-tree JS must extract level: {js}"
        );
    }

    #[test]
    fn build_js_aria_tree_restricts_attrs() {
        // ARIA-tree must only include actionable attributes.
        let js = build_js("a", OutputMode::AriaTree);
        assert!(
            js.contains("aria-label"),
            "ARIA-tree must include aria-label: {js}"
        );
        assert!(js.contains("href"), "ARIA-tree must include href: {js}");
        // Must NOT dump all attributes (no looping over all attributes).
        // The actionable list is explicit; classList/class is absent.
        assert!(
            !js.contains("e.attributes.length"),
            "ARIA-tree must not dump all attributes: {js}"
        );
    }

    #[test]
    fn aria_tree_js_template_has_ref_placeholder() {
        assert!(
            ARIA_TREE_JS_TEMPLATE.contains("__REF_START__"),
            "template must have ref start placeholder"
        );
        assert!(
            ARIA_TREE_JS_TEMPLATE.contains("__SELECTOR__"),
            "template must have selector placeholder"
        );
    }

    #[test]
    fn aria_tree_js_template_includes_shadow_root_hints() {
        // The template must check for shadow roots and emit hasShadowRoot +
        // shadowMode so agent callers can detect shadow-DOM hosts without
        // issuing a second query.
        assert!(
            ARIA_TREE_JS_TEMPLATE.contains("hasShadowRoot"),
            "ARIA-tree template must set hasShadowRoot: {ARIA_TREE_JS_TEMPLATE}"
        );
        assert!(
            ARIA_TREE_JS_TEMPLATE.contains("shadowMode"),
            "ARIA-tree template must set shadowMode: {ARIA_TREE_JS_TEMPLATE}"
        );
        assert!(
            ARIA_TREE_JS_TEMPLATE.contains("shadowRoot"),
            "ARIA-tree template must inspect shadowRoot: {ARIA_TREE_JS_TEMPLATE}"
        );
    }
}
