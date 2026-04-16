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
use crate::output;
use crate::output_pipeline::OutputPipeline;

use super::connect_tab::connect_direct;
use super::js_helpers::{JSON_SENTINEL, escape_selector, eval_or_bail};

/// Build the JavaScript that collects computed styles for every matching element.
///
/// When `prop` is `Some`, returns a JSON array `[{selector, index, value}, ...]`
/// where `value` is the single resolved string.  When `prop` is `None`, returns
/// a JSON array `[{selector, index, computed: {...}}, ...]` where `computed` is
/// the full (or filtered) resolved-style object.
///
/// `include_all` selects the object payload:
/// - `false`: only properties whose computed value differs from the element's
///   default (filtering happens page-side to keep the output readable —
///   practical tests routinely return 300+ properties per element).
/// - `true`: every property exposed by `getComputedStyle`.
fn build_js(selector: &str, prop: Option<&str>, include_all: bool) -> String {
    let escaped_sel = escape_selector(selector);

    if let Some(p) = prop {
        // Escape the property name the same way as the selector — both end up
        // inside single-quoted JS literals.
        let escaped_prop = escape_selector(p);
        return format!(
            r"(function() {{
  var els = document.querySelectorAll('{escaped_sel}');
  var out = [];
  for (var i = 0; i < els.length; i++) {{
    var cs = getComputedStyle(els[i]);
    out.push({{selector: '{escaped_sel}', index: i, value: cs.getPropertyValue('{escaped_prop}') || cs['{escaped_prop}'] || ''}});
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

pub fn run(
    cli: &Cli,
    selector: &str,
    prop: Option<&str>,
    include_all: bool,
) -> Result<(), AppError> {
    // One-shot eval wrapper: bypass the daemon per the iter-40 pattern.
    let mut ctx = connect_direct(cli)?;
    let console_actor = ctx.target.console_actor.clone();

    let js = build_js(selector, prop, include_all);
    let eval_result = eval_or_bail(&mut ctx, &console_actor, &js, "computed query failed")?;

    let entries = resolve_json_array(&mut ctx, &eval_result.result)?;

    if entries.is_empty() {
        return Err(AppError::User(format!(
            "computed: no element matching selector '{selector}'"
        )));
    }

    // --prop mode: single string per entry, just like `dom`.  For a single
    // match we collapse to a scalar string for the most common case
    // (`ff-rdp computed h1 --prop color` → "rgb(0,0,0)"); multi-match returns
    // an array of {selector, index, value}.
    let total = entries.len();

    let results = if prop.is_some() {
        if entries.len() == 1 {
            entries
                .into_iter()
                .next()
                .and_then(|mut e| e.as_object_mut().and_then(|o| o.remove("value")))
                .unwrap_or(Value::Null)
        } else {
            Value::Array(entries)
        }
    } else if entries.len() == 1 {
        // Single match → unwrap to {selector, index, computed} so the common
        // case does not need `.results[0]` indexing.
        entries.into_iter().next().unwrap_or(Value::Null)
    } else {
        Value::Array(entries)
    };
    let meta = json!({
        "host": cli.host,
        "port": cli.port,
        "selector": selector,
    });
    let envelope = output::envelope(&results, total, &meta);

    OutputPipeline::from_cli(cli)?
        .finalize(&envelope)
        .map_err(AppError::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_js_prop_mode_references_property() {
        let js = build_js("h1", Some("color"), false);
        assert!(js.contains("getPropertyValue('color')"));
        assert!(js.contains("querySelectorAll('h1')"));
    }

    #[test]
    fn build_js_object_mode_non_default_filters() {
        let js = build_js(".card", None, false);
        assert!(js.contains("rcs.getPropertyValue(name) !== v"));
        assert!(js.contains("document.createElement"));
    }

    #[test]
    fn build_js_all_mode_dumps_everything() {
        let js = build_js(".card", None, true);
        // --all should not instantiate a reference element for diffing.
        assert!(!js.contains("document.createElement"));
        assert!(js.contains("cs.getPropertyValue(name)"));
    }

    #[test]
    fn build_js_escapes_selector() {
        let js = build_js("div[data-x='y']", None, false);
        assert!(js.contains(r"div[data-x=\'y\']"));
    }
}
