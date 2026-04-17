use serde_json::{Value, json};

use crate::cli::args::Cli;
use crate::error::AppError;
use crate::output;
use crate::output_controls::{OutputControls, SortDir};
use crate::output_pipeline::OutputPipeline;

use super::connect_tab::connect_and_get_target;
use super::js_helpers::{eval_or_bail, resolve_result};

/// JavaScript IIFE template for collecting element geometry data.
///
/// `__SELECTORS__` is replaced with the JSON-encoded array of CSS selectors
/// before evaluation.  All selectors come through `serde_json::to_string`
/// so no manual escaping is needed.
///
/// `__VISIBLE_ONLY__` is replaced with `true` or `false` to enable filtering
/// of invisible elements (zero-size, display:none, visibility:hidden, opacity:0)
/// before computing overlaps.
const GEOMETRY_JS_TEMPLATE: &str = r"(function() {
  var selectors = __SELECTORS__;
  var visibleOnly = __VISIBLE_ONLY__;
  var vw = window.innerWidth || document.documentElement.clientWidth;
  var vh = window.innerHeight || document.documentElement.clientHeight;
  var elements = [];

  for (var si = 0; si < selectors.length; si++) {
    var sel = selectors[si];
    var els = document.querySelectorAll(sel);
    for (var ei = 0; ei < els.length; ei++) {
      var el = els[ei];
      var r = el.getBoundingClientRect();
      var cs = window.getComputedStyle(el);
      var rect = {
        x: Math.round(r.x * 10) / 10,
        y: Math.round(r.y * 10) / 10,
        width: Math.round(r.width * 10) / 10,
        height: Math.round(r.height * 10) / 10,
        top: Math.round(r.top * 10) / 10,
        right: Math.round(r.right * 10) / 10,
        bottom: Math.round(r.bottom * 10) / 10,
        left: Math.round(r.left * 10) / 10
      };
      var vis = r.width > 0 && r.height > 0 &&
        cs.visibility !== 'hidden' && cs.display !== 'none' &&
        parseFloat(cs.opacity) > 0;
      if (visibleOnly && !vis) { continue; }
      var inVp = r.bottom > 0 && r.top < vh && r.right > 0 && r.left < vw;
      elements.push({
        selector: sel,
        index: ei,
        tag: el.tagName.toLowerCase(),
        rect: rect,
        computed: {
          position: cs.position,
          z_index: cs.zIndex,
          visibility: cs.visibility,
          display: cs.display,
          overflow: cs.overflow,
          opacity: cs.opacity
        },
        visible: vis,
        in_viewport: inVp
      });
    }
  }

  var overlaps = [];
  for (var i = 0; i < elements.length; i++) {
    for (var j = i + 1; j < elements.length; j++) {
      var a = elements[i].rect;
      var b = elements[j].rect;
      if (a.left < b.right && a.right > b.left && a.top < b.bottom && a.bottom > b.top) {
        overlaps.push([
          elements[i].selector + '[' + elements[i].index + ']',
          elements[j].selector + '[' + elements[j].index + ']'
        ]);
      }
    }
  }

  return '__FF_RDP_JSON__' + JSON.stringify({elements: elements, overlaps: overlaps, viewport: {width: vw, height: vh}});
})()";

pub fn run(cli: &Cli, selectors: &[String], visible_only: bool) -> Result<(), AppError> {
    let mut ctx = connect_and_get_target(cli)?;
    let console_actor = ctx.target.console_actor.clone();

    let js = build_js(selectors, visible_only);

    let eval_result = eval_or_bail(&mut ctx, &console_actor, &js, "geometry evaluation failed")?;

    let geometry = resolve_result(&mut ctx, &eval_result.result)?;

    // If the result is null (e.g. no elements matched) return an empty result.
    if geometry.is_null() {
        let empty = json!({"elements": [], "overlaps": [], "viewport": null});
        if cli.format == "text" && cli.jq.is_none() {
            render_geometry_text(&empty);
            return Ok(());
        }
        let meta = json!({"host": cli.host, "port": cli.port, "selectors": selectors});
        let envelope = output::envelope(&empty, 0, &meta);
        return OutputPipeline::from_cli(cli)?
            .finalize(&envelope)
            .map_err(AppError::from);
    }

    let elements_array = geometry["elements"].as_array().cloned().unwrap_or_default();
    let overlaps = geometry["overlaps"].clone();
    let viewport = geometry["viewport"].clone();

    let controls = OutputControls::from_cli(cli, SortDir::Asc);
    let mut items = elements_array;
    controls.apply_sort(&mut items);
    let (limited, total, truncated) = controls.apply_limit(items, Some(20));
    let shown = limited.len();

    // Filter overlaps so that only pairs where both element indices are within
    // the limited set are included.  Each element in `limited` has an `index`
    // field but the overlap entries use "selector[index]" strings — we compare
    // against the set of "selector[index]" keys present after limiting.
    let kept_keys: std::collections::HashSet<String> = limited
        .iter()
        .filter_map(|el| {
            let sel = el["selector"].as_str()?;
            let idx = el["index"].as_u64()?;
            Some(format!("{sel}[{idx}]"))
        })
        .collect();

    let filtered_overlaps: Vec<Value> = overlaps
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter(|pair| {
                    pair.as_array().is_some_and(|p| {
                        p.len() == 2
                            && p[0].as_str().is_some_and(|a| kept_keys.contains(a))
                            && p[1].as_str().is_some_and(|b| kept_keys.contains(b))
                    })
                })
                .cloned()
                .collect()
        })
        .unwrap_or_default();

    let limited = controls.apply_fields(limited);

    let results = json!({
        "elements": limited,
        "overlaps": filtered_overlaps,
        "viewport": viewport,
    });

    let meta = json!({"host": cli.host, "port": cli.port, "selectors": selectors});

    // Text short-circuit: render a human-readable table instead of JSON.
    if cli.format == "text" && cli.jq.is_none() {
        render_geometry_text(&results);
        return Ok(());
    }

    let envelope = output::envelope_with_truncation(&results, shown, total, truncated, &meta);

    OutputPipeline::from_cli(cli)?
        .finalize(&envelope)
        .map_err(AppError::from)
}

/// Render geometry results as human-readable text to stdout.
fn render_geometry_text(results: &Value) {
    // Viewport
    if let Some(vp) = results.get("viewport") {
        let w = vp.get("width").and_then(Value::as_u64).unwrap_or(0);
        let h = vp.get("height").and_then(Value::as_u64).unwrap_or(0);
        println!("Viewport: {w}x{h}");
        println!();
    }

    let elements = match results.get("elements").and_then(Value::as_array) {
        Some(e) if !e.is_empty() => e,
        _ => {
            println!("(no elements)");
            return;
        }
    };

    // Compute column widths for selector and tag
    let sel_width = elements
        .iter()
        .filter_map(|e| e.get("selector").and_then(Value::as_str))
        .map(str::len)
        .max()
        .unwrap_or(8)
        .max(8);
    let tag_width = elements
        .iter()
        .filter_map(|e| e.get("tag").and_then(Value::as_str))
        .map(str::len)
        .max()
        .unwrap_or(3)
        .max(3);

    println!(
        "{:<sel_width$}  {:<tag_width$}  {:>8}  {:>8}  {:>8}  {:>8}  {:>7}  {:>11}",
        "selector", "tag", "x", "y", "width", "height", "visible", "in_viewport"
    );
    println!(
        "{}  {}  {}  {}  {}  {}  {}  {}",
        "-".repeat(sel_width),
        "-".repeat(tag_width),
        "-".repeat(8),
        "-".repeat(8),
        "-".repeat(8),
        "-".repeat(8),
        "-".repeat(7),
        "-".repeat(11)
    );

    for el in elements {
        let selector = el.get("selector").and_then(Value::as_str).unwrap_or("?");
        let tag = el.get("tag").and_then(Value::as_str).unwrap_or("?");
        let rect = el.get("rect");
        let x = rect
            .and_then(|r| r.get("x"))
            .and_then(Value::as_f64)
            .unwrap_or(0.0);
        let y = rect
            .and_then(|r| r.get("y"))
            .and_then(Value::as_f64)
            .unwrap_or(0.0);
        let w = rect
            .and_then(|r| r.get("width"))
            .and_then(Value::as_f64)
            .unwrap_or(0.0);
        let h = rect
            .and_then(|r| r.get("height"))
            .and_then(Value::as_f64)
            .unwrap_or(0.0);
        let visible = el
            .get("visible")
            .and_then(Value::as_bool)
            .map_or("?", |b| if b { "yes" } else { "no" });
        let in_vp = el
            .get("in_viewport")
            .and_then(Value::as_bool)
            .map_or("?", |b| if b { "yes" } else { "no" });

        println!(
            "{selector:<sel_width$}  {tag:<tag_width$}  {x:>8.1}  {y:>8.1}  {w:>8.1}  {h:>8.1}  {visible:>7}  {in_vp:>11}"
        );
    }

    // Overlaps
    if let Some(overlaps) = results.get("overlaps").and_then(Value::as_array)
        && !overlaps.is_empty()
    {
        println!();
        println!("Overlaps:");
        for pair in overlaps {
            if let Some(arr) = pair.as_array()
                && arr.len() == 2
            {
                let a = arr[0].as_str().unwrap_or("?");
                let b = arr[1].as_str().unwrap_or("?");
                println!("  {a} <-> {b}");
            }
        }
    }
}

/// Build the JS IIFE by serializing selectors as a JSON array and substituting
/// the placeholders.
fn build_js(selectors: &[String], visible_only: bool) -> String {
    // serde_json::to_string is infallible for Vec<String>
    let selectors_json = serde_json::to_string(selectors).unwrap_or_else(|e| {
        unreachable!("serde_json::to_string is infallible for Vec<String>: {e}")
    });
    let visible_only_str = if visible_only { "true" } else { "false" };
    GEOMETRY_JS_TEMPLATE
        .replace("__SELECTORS__", &selectors_json)
        .replace("__VISIBLE_ONLY__", visible_only_str)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn render_geometry_text_does_not_panic_with_full_data() {
        let data = json!({
            "elements": [
                {"selector": "h1", "tag": "h1", "rect": {"x": 0.0, "y": 10.0, "width": 400.0, "height": 40.0}, "visible": true, "in_viewport": true},
                {"selector": "p", "tag": "p", "rect": {"x": 0.0, "y": 60.0, "width": 400.0, "height": 20.0}, "visible": true, "in_viewport": true},
            ],
            "overlaps": [],
            "viewport": {"width": 1024, "height": 768},
        });
        render_geometry_text(&data);
    }

    #[test]
    fn render_geometry_text_does_not_panic_with_empty_elements() {
        let data =
            json!({"elements": [], "overlaps": [], "viewport": {"width": 1024, "height": 768}});
        render_geometry_text(&data);
    }

    #[test]
    fn render_geometry_text_with_overlaps() {
        let data = json!({
            "elements": [
                {"selector": ".a", "tag": "div", "rect": {"x": 0.0, "y": 0.0, "width": 100.0, "height": 100.0}, "visible": true, "in_viewport": true},
            ],
            "overlaps": [[".a[0]", ".b[0]"]],
            "viewport": {"width": 1024, "height": 768},
        });
        render_geometry_text(&data);
    }

    #[test]
    fn build_js_inserts_selectors() {
        let selectors = vec!["h1".to_owned(), "p".to_owned()];
        let js = build_js(&selectors, false);
        assert!(js.contains(r#"["h1","p"]"#));
        assert!(!js.contains("__SELECTORS__"));
    }

    #[test]
    fn build_js_escapes_special_chars_in_selectors() {
        // Selectors with quotes — serde_json handles JSON escaping
        let selectors = vec!["[data-id=\"foo\"]".to_owned()];
        let js = build_js(&selectors, false);
        assert!(!js.contains("__SELECTORS__"));
        // The result must be valid JSON for the array element
        let start = js.find("var selectors = ").expect("placeholder replaced") + 16;
        let end = js[start..].find(';').expect("semicolon") + start;
        let arr: serde_json::Value =
            serde_json::from_str(&js[start..end]).expect("selectors must be valid JSON");
        assert_eq!(arr[0], "[data-id=\"foo\"]");
    }

    #[test]
    fn build_js_contains_sentinel() {
        let js = build_js(&["div".to_owned()], false);
        assert!(js.contains(super::super::js_helpers::JSON_SENTINEL));
    }

    #[test]
    fn build_js_contains_overlap_detection() {
        let js = build_js(&["div".to_owned()], false);
        assert!(js.contains("overlaps"));
        assert!(js.contains("getBoundingClientRect"));
        assert!(js.contains("getComputedStyle"));
    }

    #[test]
    fn build_js_visible_only_false_uses_false_literal() {
        let js = build_js(&["div".to_owned()], false);
        assert!(js.contains("var visibleOnly = false;"));
        assert!(!js.contains("__VISIBLE_ONLY__"));
    }

    #[test]
    fn build_js_visible_only_true_uses_true_literal() {
        let js = build_js(&["div".to_owned()], true);
        assert!(js.contains("var visibleOnly = true;"));
        assert!(!js.contains("__VISIBLE_ONLY__"));
    }

    #[test]
    fn build_js_visible_only_includes_filter_guard() {
        let js = build_js(&["div".to_owned()], true);
        assert!(js.contains("if (visibleOnly && !vis) { continue; }"));
    }
}
