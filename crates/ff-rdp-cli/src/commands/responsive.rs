use std::time::Duration;

use ff_rdp_core::WebConsoleActor;
use serde_json::{Value, json};

use crate::cli::args::Cli;
use crate::error::AppError;
use crate::output;
use crate::output_pipeline::OutputPipeline;

use super::connect_tab::connect_and_get_target;
use super::js_helpers::resolve_result;

/// JavaScript IIFE template for collecting element geometry at a specific viewport width.
///
/// `__SELECTORS__` is replaced with the JSON-encoded array of CSS selectors
/// before evaluation.  All selectors come through `serde_json::to_string`
/// so no manual escaping is needed.
const RESPONSIVE_JS_TEMPLATE: &str = r"(function() {
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
          display: cs.display,
          visibility: cs.visibility,
          font_size: cs.fontSize,
          flex_direction: cs.flexDirection,
          flex_wrap: cs.flexWrap,
          grid_template_columns: cs.gridTemplateColumns
        },
        visible: vis,
        in_viewport: inVp
      });
    }
  }
  return '__FF_RDP_JSON__' + JSON.stringify({elements: elements, viewport: {width: vw, height: vh}});
})()";

/// JS snippet that retrieves the current outer window dimensions.
const GET_VIEWPORT_JS: &str = "JSON.stringify({width: window.outerWidth, height: window.outerHeight, innerWidth: window.innerWidth, innerHeight: window.innerHeight})";

/// Build a JS snippet that resizes the browser window to the given width,
/// keeping the current outer height.
fn resize_js(width: u32) -> String {
    format!("window.resizeTo({width}, window.outerHeight)")
}

/// Build the geometry IIFE by serializing selectors as a JSON array and
/// substituting the `__SELECTORS__` placeholder.
pub(crate) fn build_geometry_js(selectors: &[String]) -> String {
    // serde_json::to_string is infallible for Vec<String>
    let selectors_json = serde_json::to_string(selectors).unwrap_or_else(|e| {
        unreachable!("serde_json::to_string is infallible for Vec<String>: {e}")
    });
    RESPONSIVE_JS_TEMPLATE.replace("__SELECTORS__", &selectors_json)
}

/// Validate that all requested widths are non-zero.
fn validate_widths(widths: &[u32]) -> Result<(), AppError> {
    if widths.is_empty() {
        return Err(AppError::User(
            "at least one width must be specified via --widths".to_string(),
        ));
    }
    for &w in widths {
        if w == 0 {
            return Err(AppError::User(
                "viewport width must be greater than 0".to_string(),
            ));
        }
    }
    Ok(())
}

pub fn run(cli: &Cli, selectors: &[String], widths: &[u32]) -> Result<(), AppError> {
    validate_widths(widths)?;

    let mut ctx = connect_and_get_target(cli)?;
    let console_actor = ctx.target.console_actor.clone();

    // --- Step 1: capture original viewport dimensions -----------------------
    let vp_result =
        WebConsoleActor::evaluate_js_async(ctx.transport_mut(), &console_actor, GET_VIEWPORT_JS)
            .map_err(AppError::from)?;

    if let Some(ref exc) = vp_result.exception {
        let msg = exc.message.as_deref().unwrap_or("viewport query failed");
        return Err(AppError::User(format!("get viewport: {msg}")));
    }

    let vp_json_str = match &vp_result.result {
        ff_rdp_core::Grip::Value(Value::String(s)) => s.clone(),
        other => {
            return Err(AppError::User(format!(
                "unexpected viewport result: {}",
                other.to_json()
            )));
        }
    };
    let original_viewport: Value = serde_json::from_str(&vp_json_str)
        .map_err(|e| AppError::from(anyhow::anyhow!("failed to parse viewport JSON: {e}")))?;

    let original_outer_width =
        u32::try_from(original_viewport["width"].as_u64().unwrap_or(1280)).unwrap_or(1280);
    let original_outer_height =
        u32::try_from(original_viewport["height"].as_u64().unwrap_or(800)).unwrap_or(800);

    // --- Step 2: iterate over each breakpoint width -------------------------
    let geom_js = build_geometry_js(selectors);
    let mut breakpoints: Vec<Value> = Vec::with_capacity(widths.len());

    // We always attempt to restore the viewport, even when an error occurs
    // mid-loop.  Collect the first error encountered and restore before
    // returning it.
    let mut loop_error: Option<AppError> = None;

    'bp: for &width in widths {
        // Resize the viewport to the target width.
        let resize_script = resize_js(width);
        let resize_result =
            WebConsoleActor::evaluate_js_async(ctx.transport_mut(), &console_actor, &resize_script)
                .map_err(AppError::from);

        if let Err(e) = resize_result {
            loop_error = Some(e);
            break 'bp;
        }
        if let Some(ref exc) = resize_result.as_ref().unwrap().exception {
            let msg = exc
                .message
                .as_deref()
                .unwrap_or("resize failed")
                .to_string();
            loop_error = Some(AppError::User(format!("resize to {width}: {msg}")));
            break 'bp;
        }

        // Allow the browser layout to settle after resize.
        std::thread::sleep(Duration::from_millis(100));

        // Collect geometry at this viewport width.
        let geo_result =
            WebConsoleActor::evaluate_js_async(ctx.transport_mut(), &console_actor, &geom_js)
                .map_err(AppError::from);

        let geo_result = match geo_result {
            Ok(r) => r,
            Err(e) => {
                loop_error = Some(e);
                break 'bp;
            }
        };

        if let Some(ref exc) = geo_result.exception {
            let msg = exc
                .message
                .as_deref()
                .unwrap_or("geometry evaluation failed")
                .to_string();
            loop_error = Some(AppError::User(format!("geometry at {width}: {msg}")));
            break 'bp;
        }

        let geometry = match resolve_result(&mut ctx, &geo_result.result) {
            Ok(v) => v,
            Err(e) => {
                loop_error = Some(e);
                break 'bp;
            }
        };

        let elements = geometry["elements"].clone();
        let viewport = geometry["viewport"].clone();

        breakpoints.push(json!({
            "width": width,
            "viewport": viewport,
            "elements": elements,
        }));
    }

    // --- Step 3: restore original viewport ----------------------------------
    let restore_script =
        format!("window.resizeTo({original_outer_width}, {original_outer_height})");
    // Ignore restore errors — we've already collected our data (or have an
    // error to return).  Best-effort restore is the right tradeoff here.
    let _ =
        WebConsoleActor::evaluate_js_async(ctx.transport_mut(), &console_actor, &restore_script);

    // Return any loop error after restoring.
    if let Some(e) = loop_error {
        return Err(e);
    }

    // --- Step 4: build and emit output --------------------------------------
    let breakpoint_count = breakpoints.len();
    let results = json!({
        "breakpoints": breakpoints,
        "original_viewport": original_viewport,
    });

    let meta = json!({
        "host": cli.host,
        "port": cli.port,
        "selectors": selectors,
        "widths": widths,
    });

    let envelope = output::envelope(&results, breakpoint_count, &meta);

    OutputPipeline::from_cli(cli)?
        .finalize(&envelope)
        .map_err(AppError::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_geometry_js_inserts_selectors() {
        let selectors = vec!["h1".to_owned(), "p".to_owned()];
        let js = build_geometry_js(&selectors);
        assert!(js.contains(r#"["h1","p"]"#));
        assert!(!js.contains("__SELECTORS__"));
    }

    #[test]
    fn build_geometry_js_contains_sentinel() {
        let js = build_geometry_js(&["div".to_owned()]);
        assert!(js.contains(super::super::js_helpers::JSON_SENTINEL));
    }

    #[test]
    fn build_geometry_js_escapes_special_chars() {
        let selectors = vec!["[data-id=\"foo\"]".to_owned()];
        let js = build_geometry_js(&selectors);
        assert!(!js.contains("__SELECTORS__"));
        // The substituted value must be valid JSON
        let start = js.find("var selectors = ").expect("placeholder replaced") + 16;
        let end = js[start..].find(';').expect("semicolon") + start;
        let arr: serde_json::Value =
            serde_json::from_str(&js[start..end]).expect("selectors must be valid JSON");
        assert_eq!(arr[0], "[data-id=\"foo\"]");
    }

    #[test]
    fn validate_widths_rejects_zero() {
        let err = validate_widths(&[320, 0, 1024]).unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("greater than 0"), "unexpected message: {msg}");
    }

    #[test]
    fn validate_widths_rejects_empty() {
        let err = validate_widths(&[]).unwrap_err();
        let msg = format!("{err}");
        assert!(
            msg.contains("at least one width"),
            "unexpected message: {msg}"
        );
    }

    #[test]
    fn validate_widths_accepts_valid() {
        assert!(validate_widths(&[320, 768, 1024, 1440]).is_ok());
    }

    #[test]
    fn build_geometry_js_contains_responsive_properties() {
        let js = build_geometry_js(&["div".to_owned()]);
        assert!(js.contains("getBoundingClientRect"));
        assert!(js.contains("getComputedStyle"));
        assert!(js.contains("flexDirection"));
        assert!(js.contains("gridTemplateColumns"));
        assert!(js.contains("fontSize"));
    }

    #[test]
    fn resize_js_embeds_width() {
        let js = resize_js(768);
        assert_eq!(js, "window.resizeTo(768, window.outerHeight)");
    }
}
