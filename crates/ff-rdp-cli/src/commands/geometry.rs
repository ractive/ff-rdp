use ff_rdp_core::WebConsoleActor;
use serde_json::{Value, json};

use crate::cli::args::Cli;
use crate::error::AppError;
use crate::output;
use crate::output_controls::{OutputControls, SortDir};
use crate::output_pipeline::OutputPipeline;

use super::connect_tab::connect_and_get_target;
use super::js_helpers::resolve_result;

/// JavaScript IIFE template for collecting element geometry data.
///
/// `__SELECTORS__` is replaced with the JSON-encoded array of CSS selectors
/// before evaluation.  All selectors come through `serde_json::to_string`
/// so no manual escaping is needed.
const GEOMETRY_JS_TEMPLATE: &str = r"(function() {
  var selectors = __SELECTORS__;
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

pub fn run(cli: &Cli, selectors: &[String]) -> Result<(), AppError> {
    let mut ctx = connect_and_get_target(cli)?;
    let console_actor = ctx.target.console_actor.clone();

    let js = build_js(selectors);

    let eval_result = WebConsoleActor::evaluate_js_async(ctx.transport_mut(), &console_actor, &js)
        .map_err(AppError::from)?;

    if let Some(ref exc) = eval_result.exception {
        let msg = exc
            .message
            .as_deref()
            .unwrap_or("geometry evaluation failed");
        eprintln!("error: {msg}");
        return Err(AppError::Exit(1));
    }

    let geometry = resolve_result(&mut ctx, &eval_result.result)?;

    // If the result is null (e.g. no elements matched) return an empty envelope.
    if geometry.is_null() {
        let meta = json!({"host": cli.host, "port": cli.port, "selectors": selectors});
        let empty = json!({"elements": [], "overlaps": [], "viewport": null});
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

    let envelope = output::envelope_with_truncation(&results, shown, total, truncated, &meta);

    OutputPipeline::from_cli(cli)?
        .finalize(&envelope)
        .map_err(AppError::from)
}

/// Build the JS IIFE by serializing selectors as a JSON array and substituting
/// the placeholder.
fn build_js(selectors: &[String]) -> String {
    // serde_json::to_string is infallible for Vec<String>
    let selectors_json = serde_json::to_string(selectors).unwrap_or_else(|e| {
        unreachable!("serde_json::to_string is infallible for Vec<String>: {e}")
    });
    GEOMETRY_JS_TEMPLATE.replace("__SELECTORS__", &selectors_json)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_js_inserts_selectors() {
        let selectors = vec!["h1".to_owned(), "p".to_owned()];
        let js = build_js(&selectors);
        assert!(js.contains(r#"["h1","p"]"#));
        assert!(!js.contains("__SELECTORS__"));
    }

    #[test]
    fn build_js_escapes_special_chars_in_selectors() {
        // Selectors with quotes — serde_json handles JSON escaping
        let selectors = vec!["[data-id=\"foo\"]".to_owned()];
        let js = build_js(&selectors);
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
        let js = build_js(&["div".to_owned()]);
        assert!(js.contains(super::super::js_helpers::JSON_SENTINEL));
    }

    #[test]
    fn build_js_contains_overlap_detection() {
        let js = build_js(&["div".to_owned()]);
        assert!(js.contains("overlaps"));
        assert!(js.contains("getBoundingClientRect"));
        assert!(js.contains("getComputedStyle"));
    }
}
