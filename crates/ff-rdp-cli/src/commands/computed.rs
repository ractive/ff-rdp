//! `computed` command — first-class wrapper around `getComputedStyle()`.
//!
//! The dogfooding session [[dogfooding-session-nova-template-jsonforms-index]]
//! reached for `getComputedStyle(sel)[prop]` four times in one sitting, which
//! motivates a dedicated subcommand. This module implements it as a one-shot
//! eval wrapper that connects directly to Firefox (daemon-bypass per iter-40):
//! the output is a synchronous JSON payload, not a stream, so the daemon's
//! watcher subscription would only add latency.

use ff_rdp_core::{Grip, LongStringActor};
use serde_json::{Value, json};

use crate::cli::args::Cli;
use crate::error::AppError;
use crate::hints::{HintContext, HintSource};
use crate::output;
use crate::output_pipeline::OutputPipeline;

use super::connect_tab::connect_direct;
use super::js_helpers::{JSON_SENTINEL, escape_selector, eval_or_bail};

/// Build the JavaScript that collects computed styles for every matching element.
///
/// Always returns a JSON array `[{selector, index, computed: {...}}, ...]` regardless
/// of how many `props` are requested (zero, one, or many). When `props` is non-empty,
/// `computed` contains only the requested properties. When `props` is empty, `computed`
/// is the full (or filtered) resolved-style object.
///
/// CSS custom properties (`--bg-color`, etc.) are supported via `getPropertyValue`,
/// which is the standard API for custom properties (bracket notation does not work
/// for them in `CSSStyleDeclaration`).
///
/// `include_all` selects the object payload when `props` is empty:
/// - `false`: only properties whose computed value differs from the element's
///   default (filtering happens page-side to keep the output readable —
///   practical tests routinely return 300+ properties per element).
/// - `true`: every property exposed by `getComputedStyle`.
fn build_js(selector: &str, props: &[String], include_all: bool) -> String {
    let escaped_sel = escape_selector(selector);

    if !props.is_empty() {
        // One or more explicit props: always `{selector, index, computed: {p: v, …}}`.
        let props_json = serde_json::to_string(props).unwrap_or_else(|_| "[]".to_owned());
        return format!(
            r"(function() {{
  var props = {props_json};
  var els = document.querySelectorAll('{escaped_sel}');
  var out = [];
  for (var i = 0; i < els.length; i++) {{
    var cs = getComputedStyle(els[i]);
    var computed = {{}};
    for (var j = 0; j < props.length; j++) {{
      var p = props[j];
      computed[p] = cs.getPropertyValue(p).trim() || cs[p] || '';
    }}
    out.push({{selector: '{escaped_sel}', index: i, computed: computed}});
  }}
  return '{JSON_SENTINEL}' + JSON.stringify(out);
}})()"
        );
    }

    // Full-object mode.  When `include_all` is true, dump every enumerable
    // index of the CSSStyleDeclaration.  Otherwise compare against a
    // freshly-created element of the same tag to filter out default values.
    let body = if include_all {
        r"
    var obj = {};
    for (var j = 0; j < cs.length; j++) {
      var name = cs[j];
      obj[name] = cs.getPropertyValue(name);
    }
    out.push({selector: sel, index: i, computed: obj});"
    } else {
        r"
    var container = document.body || document.documentElement;
    var refEl = document.createElement(el.tagName);
    var rcs = null;
    if (container) {
      container.appendChild(refEl);
      rcs = getComputedStyle(refEl);
    }
    var obj = {};
    for (var j = 0; j < cs.length; j++) {
      var name = cs[j];
      var v = cs.getPropertyValue(name);
      if (!rcs || rcs.getPropertyValue(name) !== v) {
        obj[name] = v;
      }
    }
    if (container) { refEl.remove(); }
    out.push({selector: sel, index: i, computed: obj});"
    };

    format!(
        r"(function() {{
  var sel = '{escaped_sel}';
  var els = document.querySelectorAll(sel);
  var out = [];
  for (var i = 0; i < els.length; i++) {{
    var el = els[i];
    var cs = getComputedStyle(el);{body}
  }}
  return '{JSON_SENTINEL}' + JSON.stringify(out);
}})()"
    )
}

/// Resolve the eval result: fetch LongStrings, strip the JSON sentinel, parse.
fn resolve_json_array(
    ctx: &mut super::connect_tab::ConnectedTab,
    grip: &Grip,
) -> Result<Vec<Value>, AppError> {
    let raw = match grip {
        Grip::Value(Value::String(s)) => s.clone(),
        Grip::LongString {
            actor,
            length,
            initial: _,
        } => LongStringActor::full_string(ctx.transport_mut(), actor.as_ref(), *length)
            .map_err(AppError::from)?,
        other => {
            return Err(AppError::User(format!(
                "computed: unexpected result type: {}",
                other.to_json()
            )));
        }
    };

    let stripped = raw
        .strip_prefix(JSON_SENTINEL)
        .ok_or_else(|| AppError::User("computed: missing JSON sentinel in result".to_owned()))?;

    serde_json::from_str::<Vec<Value>>(stripped).map_err(|e| {
        AppError::from(anyhow::anyhow!(
            "computed: failed to parse JSON result: {e}"
        ))
    })
}

pub fn run(cli: &Cli, selector: &str, props: &[String], include_all: bool) -> Result<(), AppError> {
    // One-shot eval wrapper: bypass the daemon per the iter-40 pattern.
    let mut ctx = connect_direct(cli)?;
    let console_actor = ctx.target.console_actor.clone();

    // Also support comma-list style for CSS custom properties that start with `--`
    // and cannot be passed as individual clap arguments with leading dashes.
    // Split every --prop entry on commas so mixed input like
    // `--prop color,font-size --prop=--bg-color` expands correctly.
    let expanded_props: Vec<String> = props
        .iter()
        .flat_map(|p| p.split(','))
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
        .collect();

    let js = build_js(selector, &expanded_props, include_all);
    let eval_result = eval_or_bail(&mut ctx, &console_actor, &js, "computed query failed")?;

    let entries = resolve_json_array(&mut ctx, &eval_result.result)?;

    if entries.is_empty() {
        return Err(AppError::User(format!(
            "computed: no element matching selector '{selector}'"
        )));
    }

    // Always return an array of `{selector, index, computed: {…}}` records,
    // regardless of --prop count (zero, one, or many). This uniform shape
    // means callers never need to special-case single-prop or single-match.
    let total = entries.len();
    let results = Value::Array(entries);
    let mut meta = json!({
        "selector": selector,
    });
    crate::connection_meta::merge_into_if_verbose(
        &mut meta,
        &cli.host,
        cli.port,
        None,
        cli.is_verbose(),
    );
    let envelope = output::envelope(&results, total, &meta);

    let hint_ctx = HintContext::new(HintSource::Computed).with_selector(selector);
    OutputPipeline::from_cli(cli)?
        .finalize_with_hints(&envelope, Some(&hint_ctx))
        .map_err(AppError::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn props(p: &[&str]) -> Vec<String> {
        p.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn build_js_prop_mode_references_property() {
        let js = build_js("h1", &props(&["color"]), false);
        // Single prop now uses the same props-array path as multi-prop.
        assert!(js.contains("getPropertyValue(p)"));
        assert!(js.contains(r#"["color"]"#));
        assert!(js.contains("querySelectorAll('h1')"));
    }

    /// Zero props: output shape is always `[{selector, index, computed:{…}}]`.
    #[test]
    fn build_js_zero_props_returns_computed_object() {
        let js = build_js("h1", &[], false);
        assert!(js.contains("computed: obj"), "expected 'computed: obj' key");
        // Zero-props path uses a `sel` JS variable rather than inlining the literal.
        assert!(
            js.contains("var sel = 'h1'"),
            "expected selector assigned to sel variable"
        );
        assert!(
            js.contains("querySelectorAll(sel)"),
            "expected querySelectorAll using sel variable"
        );
    }

    /// Single prop: output shape is `[{selector, index, computed:{prop: val}}]`.
    #[test]
    fn build_js_single_prop_uses_computed_key() {
        let js = build_js("h1", &props(&["color"]), false);
        // Must use props array path with `computed` key (not legacy `value`).
        assert!(js.contains("computed[p]"), "expected 'computed[p]' key");
        assert!(!js.contains("value:"), "must not use legacy 'value' key");
    }

    /// Multi prop: output shape is `[{selector, index, computed:{…}}]`.
    #[test]
    fn build_js_multi_prop_uses_computed_key() {
        let js = build_js("h1", &props(&["color", "font-size"]), false);
        assert!(js.contains("computed[p]"), "expected 'computed[p]' key");
        assert!(js.contains(r#"["color","font-size"]"#));
    }

    #[test]
    fn build_js_object_mode_non_default_filters() {
        let js = build_js(".card", &[], false);
        assert!(js.contains("rcs.getPropertyValue(name) !== v"));
        assert!(js.contains("document.createElement"));
    }

    #[test]
    fn build_js_all_mode_dumps_everything() {
        let js = build_js(".card", &[], true);
        // --all should not instantiate a reference element for diffing.
        assert!(!js.contains("document.createElement"));
        assert!(js.contains("cs.getPropertyValue(name)"));
    }

    #[test]
    fn build_js_escapes_selector() {
        let js = build_js("div[data-x='y']", &[], false);
        assert!(js.contains(r"div[data-x=\'y\']"));
    }

    #[test]
    fn build_js_multi_prop_uses_props_array() {
        let js = build_js("h1", &props(&["color", "font-size"]), false);
        // Multi-prop path uses a props JSON array.
        assert!(js.contains(r#"["color","font-size"]"#));
        assert!(js.contains("getPropertyValue(p)"));
        assert!(js.contains("querySelectorAll('h1')"));
    }

    #[test]
    fn build_js_custom_property_single_prop() {
        let js = build_js("div", &props(&["--bg-color"]), false);
        // Custom properties use the props-array path with getPropertyValue(p).
        assert!(js.contains(r#"["--bg-color"]"#));
        assert!(js.contains("getPropertyValue(p)"));
    }

    #[test]
    fn build_js_custom_property_multi_prop() {
        let js = build_js("div", &props(&["--bg-color", "color"]), false);
        assert!(js.contains(r#"["--bg-color","color"]"#));
        assert!(js.contains("getPropertyValue(p)"));
    }
}
