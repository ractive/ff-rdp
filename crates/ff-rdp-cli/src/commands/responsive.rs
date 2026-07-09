use ff_rdp_core::WebConsoleActor;
use serde_json::{Value, json};

use crate::cli::args::Cli;
use crate::error::AppError;
use crate::hints::{HintContext, HintSource};
use crate::output;
use crate::output_pipeline::OutputPipeline;

use super::connect_tab::{ConnectedTab, connect_and_get_target};
use super::js_helpers::resolve_result;

/// JavaScript IIFE template for collecting element geometry at a specific viewport width.
///
/// `__SELECTORS__` is replaced with the JSON-encoded array of CSS selectors
/// before evaluation.  All selectors come through `serde_json::to_string`
/// so no manual escaping is needed.
///
/// `vw` is read from `document.documentElement.offsetWidth` rather than
/// `window.innerWidth` because the CSS-based viewport simulation (see
/// `SET_VIEWPORT_CSS_JS`) constrains layout by setting an inline `width` on
/// `<html>`.  `offsetWidth` reflects that constraint; `window.innerWidth`
/// always returns the physical viewport width and is unaffected by inline CSS.
const RESPONSIVE_JS_TEMPLATE: &str = r"(function() {
  var selectors = __SELECTORS__;
  var visibleOnly = __VISIBLE_ONLY__;
  var vw = document.documentElement.offsetWidth || window.innerWidth;
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

/// JS snippet that retrieves the current viewport dimensions.
const GET_VIEWPORT_JS: &str =
    "JSON.stringify({innerWidth: window.innerWidth, innerHeight: window.innerHeight})";

/// JS template that constrains the page layout to a specific width by setting
/// inline CSS on `<html>` and `<body>`.
///
/// This is the only reliable way to simulate a narrower viewport in headless
/// Firefox without browser-chrome APIs:
///
/// - `window.resizeTo()` is silently ignored in headless mode and blocked for
///   non-popup windows in windowed mode.
/// - The `responsiveActor` RDP actor no longer exposes a `setViewportSize`
///   packet type in Firefox 149+; viewport sizing was moved to browser-chrome
///   APIs (`synchronouslyUpdateRemoteBrowserDimensions`) that are inaccessible
///   from the RDP protocol's content-process execution context.
/// - WebDriver BiDi `browsingContext.setViewport` requires the BiDi WebSocket
///   transport, not the RDP TCP socket used by this tool.
///
/// Setting `document.documentElement.style.width` causes `getBoundingClientRect`
/// and all layout geometry to reflect the simulated width accurately.  CSS
/// `@media` queries still fire on the physical viewport width, but element
/// geometry and computed layout values are correct for the requested width.
///
/// `__WIDTH__` is replaced with a bare numeric pixel value (e.g. `320`).
const SET_VIEWPORT_CSS_JS: &str = "(function(){
  var w = __WIDTH__;
  document.documentElement.style.setProperty('width', w + 'px', 'important');
  document.documentElement.style.setProperty('max-width', w + 'px', 'important');
  document.documentElement.style.setProperty('overflow-x', 'hidden', 'important');
  document.body.style.setProperty('max-width', w + 'px', 'important');
})()";

/// JS snippet that waits for layout to stabilize after a viewport resize.
///
/// Uses `requestAnimationFrame` + `setTimeout(0)` to ensure at least one
/// layout/paint cycle has completed before resolving.  A fixed `sleep` is
/// unreliable on complex pages (e.g. MDN Web Docs) where layout hasn't
/// finished when `getBoundingClientRect` is called, producing wildly wrong
/// values such as `rect.y: -117096.5`.
const WAIT_LAYOUT_STABLE_JS: &str = "new Promise(function(resolve) {
  requestAnimationFrame(function() { setTimeout(resolve, 0); });
})";

/// JS snippet that removes the inline styles applied by `SET_VIEWPORT_CSS_JS`,
/// restoring the original layout.
const RESTORE_VIEWPORT_CSS_JS: &str = "(function(){
  document.documentElement.style.removeProperty('width');
  document.documentElement.style.removeProperty('max-width');
  document.documentElement.style.removeProperty('overflow-x');
  document.body.style.removeProperty('max-width');
})()";

/// JS template for the media-query self-check (iter-98 Theme A).
///
/// After the layout has been constrained to the requested width, this probes
/// whether the page's media queries actually flipped to that width by reading
/// `matchMedia("(width: <requested>px)").matches` alongside
/// `window.innerWidth`. Because the CSS layout-only emulation cannot resize the
/// real top-level window over RDP, `matches` is expected to be `false` whenever
/// the physical viewport differs from the requested width — the caller records
/// that in the envelope and warns rather than presenting a media-query-untruthful
/// state silently.
///
/// `__WIDTH__` is replaced with the requested pixel width. The exact-width query
/// `(width: Npx)` is the truthful probe: it is `true` only when the media
/// environment genuinely reports `N` CSS pixels of viewport width.
const MEDIA_QUERY_CHECK_JS: &str = "(function(){
  var w = __WIDTH__;
  var mq = window.matchMedia('(width: ' + w + 'px)');
  return JSON.stringify({inner_width: window.innerWidth, matches: mq.matches});
})()";

/// Build the media-query self-check JS for a requested width.
fn build_media_query_check_js(width: u32) -> String {
    MEDIA_QUERY_CHECK_JS.replace("__WIDTH__", &width.to_string())
}

/// Build the geometry IIFE by serializing selectors as a JSON array and
/// substituting the `__SELECTORS__` and `__VISIBLE_ONLY__` placeholders.
///
/// `visible_only = true` means hidden/zero-sized elements are skipped (the default).
pub(crate) fn build_geometry_js(selectors: &[String], visible_only: bool) -> String {
    // serde_json::to_string is infallible for Vec<String>
    let selectors_json = serde_json::to_string(selectors).unwrap_or_else(|e| {
        unreachable!("serde_json::to_string is infallible for Vec<String>: {e}")
    });
    let visible_only_str = if visible_only { "true" } else { "false" };
    RESPONSIVE_JS_TEMPLATE
        .replace("__VISIBLE_ONLY__", visible_only_str)
        .replace("__SELECTORS__", &selectors_json)
}

/// Build a JS snippet that applies the CSS-based viewport simulation for the
/// given pixel width by substituting `__WIDTH__`.
fn build_set_viewport_js(width: u32) -> String {
    SET_VIEWPORT_CSS_JS.replace("__WIDTH__", &width.to_string())
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

pub fn run(
    cli: &Cli,
    selectors: &[String],
    widths: &[u32],
    include_hidden: bool,
    strict: bool,
) -> Result<(), AppError> {
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

    // --- Step 2: iterate over each breakpoint width -------------------------
    let geom_js = build_geometry_js(selectors, !include_hidden);
    let mut breakpoints: Vec<Value> = Vec::with_capacity(widths.len());

    // Warnings accumulated across breakpoints — currently the media-query
    // self-check (iter-98 Theme A). Surfaced in the envelope's `warnings`
    // array; with --strict any entry makes the command exit non-zero.
    let mut warnings: Vec<String> = Vec::new();
    let mut any_mq_mismatch = false;

    // We always attempt to restore the page styles, even when an error occurs
    // mid-loop.  Collect the first error encountered and restore before
    // returning it.
    let mut loop_error: Option<AppError> = None;

    'bp: for &width in widths {
        // Simulate a narrower viewport by constraining the layout width via
        // inline CSS on <html> and <body>.  See SET_VIEWPORT_CSS_JS for the
        // full rationale of why this approach is used.
        let set_vp_js = build_set_viewport_js(width);
        match WebConsoleActor::evaluate_js_async(ctx.transport_mut(), &console_actor, &set_vp_js)
            .map_err(AppError::from)
        {
            Err(e) => {
                loop_error = Some(e);
                break 'bp;
            }
            Ok(r) => {
                if let Some(ref exc) = r.exception {
                    let msg = exc.message.as_deref().unwrap_or("set viewport failed");
                    loop_error = Some(AppError::User(format!("set viewport at {width}: {msg}")));
                    break 'bp;
                }
            }
        }

        // Wait for layout to stabilize after the CSS width change.
        // A fixed sleep is unreliable on complex pages; requestAnimationFrame
        // + setTimeout(0) ensures at least one paint cycle has completed.
        match WebConsoleActor::evaluate_js_async(
            ctx.transport_mut(),
            &console_actor,
            WAIT_LAYOUT_STABLE_JS,
        )
        .map_err(AppError::from)
        {
            Err(e) => {
                loop_error = Some(e);
                break 'bp;
            }
            Ok(r) => {
                if let Some(ref exc) = r.exception {
                    let msg = exc.message.as_deref().unwrap_or("layout wait failed");
                    loop_error = Some(AppError::User(format!("layout wait at {width}: {msg}")));
                    break 'bp;
                }
            }
        }

        // Collect geometry at this simulated viewport width.
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
                .unwrap_or("geometry evaluation failed");
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

        // Media-query self-check (iter-98 Theme A): while the layout is still
        // constrained to `width`, probe whether the page's media queries
        // actually report that width. A `false` here means the emulation is
        // layout-only (media queries did not flip) — record it truthfully and
        // warn instead of presenting a media-query-untruthful state silently.
        let mq_check = match evaluate_media_query_check(&mut ctx, &console_actor, width) {
            Ok(check) => check,
            Err(e) => {
                loop_error = Some(e);
                break 'bp;
            }
        };
        if mq_check.matches == Some(false) {
            any_mq_mismatch = true;
            warnings.push(format!(
                "at requested width {width}px the page's media queries did not flip \
                 (matchMedia(\"(width: {width}px)\").matches == false, innerWidth == {}); \
                 geometry is accurate for the requested width but @media-dependent \
                 styles reflect the physical viewport, not {width}px",
                mq_check
                    .inner_width
                    .map_or_else(|| "unknown".to_owned(), |w| w.to_string()),
            ));
        }

        breakpoints.push(json!({
            "width": width,
            "viewport": viewport,
            "media_query_check": mq_check.to_json(width),
            "elements": elements,
        }));
    }

    // --- Step 3: restore original page styles -------------------------------
    // Ignore restore errors — we've already collected our data (or have an
    // error to return).  Best-effort restore is the right tradeoff here.
    let _ = WebConsoleActor::evaluate_js_async(
        ctx.transport_mut(),
        &console_actor,
        RESTORE_VIEWPORT_CSS_JS,
    );

    // Return any loop error after restoring.
    if let Some(e) = loop_error {
        return Err(e);
    }

    // --- Step 4: build and emit output --------------------------------------
    let breakpoint_count = breakpoints.len();
    let mut results = json!({
        "breakpoints": breakpoints,
        "original_viewport": original_viewport,
    });
    // Only attach `warnings` when there are any, so the happy-path envelope
    // stays lean.
    if !warnings.is_empty()
        && let Some(obj) = results.as_object_mut()
    {
        obj.insert("warnings".to_string(), json!(warnings));
    }

    let mut meta = json!({
        "selectors": selectors,
        "widths": widths,
    });
    crate::connection_meta::merge_into_if_verbose(
        &mut meta,
        &cli.host,
        cli.port,
        None,
        cli.is_verbose(),
    );

    // Text short-circuit: render a human-readable breakpoint table instead of JSON.
    if cli.format == "text" && cli.jq.is_none() {
        render_responsive_text(&results);
        // Even in text mode --strict must still gate the exit code.
        if strict && any_mq_mismatch {
            return Err(AppError::Exit(1));
        }
        return Ok(());
    }

    let envelope = output::envelope(&results, breakpoint_count, &meta);

    let hint_ctx = HintContext::new(HintSource::Responsive);
    OutputPipeline::from_cli(cli)?
        .finalize_with_hints(&envelope, Some(&hint_ctx))
        .map_err(AppError::from)?;

    // --strict: the envelope has been emitted; a media-query mismatch now
    // becomes a non-zero exit (iter-98 Theme A). The default (non-strict) run
    // still exits 0 — the warning in the envelope is the signal.
    if strict && any_mq_mismatch {
        return Err(AppError::Exit(1));
    }
    Ok(())
}

/// Result of the media-query self-check for one breakpoint.
///
/// `inner_width`/`matches` are `None` when the probe could not be parsed (a
/// shape we never expect from `MEDIA_QUERY_CHECK_JS`, but handled defensively);
/// a `None` `matches` is treated as "not a mismatch" so a parse failure never
/// fabricates a warning.
#[derive(Debug, Clone, Default)]
struct MediaQueryCheck {
    inner_width: Option<u64>,
    matches: Option<bool>,
}

impl MediaQueryCheck {
    fn to_json(&self, requested: u32) -> Value {
        json!({
            "requested": requested,
            "inner_width": self.inner_width,
            "matches": self.matches,
        })
    }
}

/// Run the media-query self-check for `width` on the current page (which must
/// already be constrained to that layout width) and parse the result.
fn evaluate_media_query_check(
    ctx: &mut ConnectedTab,
    console_actor: &ff_rdp_core::ActorId,
    width: u32,
) -> Result<MediaQueryCheck, AppError> {
    let js = build_media_query_check_js(width);
    let result = WebConsoleActor::evaluate_js_async(ctx.transport_mut(), console_actor, &js)
        .map_err(AppError::from)?;
    if let Some(ref exc) = result.exception {
        let msg = exc
            .message
            .as_deref()
            .unwrap_or("media-query self-check failed");
        return Err(AppError::User(format!(
            "media-query self-check at {width}: {msg}"
        )));
    }
    let parsed = match &result.result {
        ff_rdp_core::Grip::Value(Value::String(s)) => {
            serde_json::from_str::<Value>(s).unwrap_or(Value::Null)
        }
        other => other.to_json(),
    };
    Ok(MediaQueryCheck {
        inner_width: parsed.get("inner_width").and_then(Value::as_u64),
        matches: parsed.get("matches").and_then(Value::as_bool),
    })
}

/// Render `responsive` results as human-readable text to stdout.
///
/// Prints a section per breakpoint width showing the viewport dimensions and
/// a table of element geometry (selector, tag, width, height, visible, in_viewport).
fn render_responsive_text(results: &Value) {
    let Some(breakpoints) = results.get("breakpoints").and_then(Value::as_array) else {
        return;
    };

    for bp in breakpoints {
        let width = bp.get("width").and_then(Value::as_u64).unwrap_or(0);
        let vp_w = bp
            .get("viewport")
            .and_then(|v| v.get("width"))
            .and_then(Value::as_u64)
            .unwrap_or(width);
        let vp_h = bp
            .get("viewport")
            .and_then(|v| v.get("height"))
            .and_then(Value::as_u64)
            .unwrap_or(0);

        println!("=== Breakpoint {width}px (viewport {vp_w}x{vp_h}) ===");

        // Media-query self-check line (iter-98 Theme A): make the layout-only
        // caveat visible in text mode too. Only printed when the check ran.
        if let Some(mq) = bp.get("media_query_check") {
            let matches = mq.get("matches").and_then(Value::as_bool);
            let inner = mq
                .get("inner_width")
                .and_then(Value::as_u64)
                .map_or_else(|| "?".to_owned(), |w| w.to_string());
            match matches {
                Some(true) => {
                    println!("  media queries: flipped to {width}px (innerWidth {inner})");
                }
                Some(false) => {
                    println!(
                        "  media queries: NOT flipped — matchMedia(width:{width}px) is false, \
                         innerWidth {inner} (layout-only; @media styles reflect the physical viewport)"
                    );
                }
                None => {}
            }
        }

        let elements = match bp.get("elements").and_then(Value::as_array) {
            Some(e) if !e.is_empty() => e,
            _ => {
                println!("  (no elements)");
                println!();
                continue;
            }
        };

        // Compute column widths for: selector, tag, w, h, visible, in_vp
        let sel_width = elements
            .iter()
            .filter_map(|e| e.get("selector").and_then(Value::as_str))
            .map(str::len)
            .max()
            .unwrap_or(8)
            .max(8);

        println!(
            "  {:<sel_width$}  {:>5}  {:>8}  {:>8}  {:>7}  {:>10}",
            "selector", "tag", "width", "height", "visible", "in_viewport"
        );
        println!(
            "  {}  {}  {}  {}  {}  {}",
            "-".repeat(sel_width),
            "-".repeat(5),
            "-".repeat(8),
            "-".repeat(8),
            "-".repeat(7),
            "-".repeat(10)
        );

        for el in elements {
            let selector = el.get("selector").and_then(Value::as_str).unwrap_or("?");
            let tag = el.get("tag").and_then(Value::as_str).unwrap_or("?");
            let el_w = el
                .get("rect")
                .and_then(|r| r.get("width"))
                .and_then(Value::as_f64)
                .unwrap_or(0.0);
            let el_h = el
                .get("rect")
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
                "  {selector:<sel_width$}  {tag:>5}  {el_w:>8.1}  {el_h:>8.1}  {visible:>7}  {in_vp:>10}"
            );
        }
        println!();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_geometry_js_inserts_selectors() {
        let selectors = vec!["h1".to_owned(), "p".to_owned()];
        let js = build_geometry_js(&selectors, true);
        assert!(js.contains(r#"["h1","p"]"#));
        assert!(!js.contains("__SELECTORS__"));
    }

    #[test]
    fn build_geometry_js_contains_sentinel() {
        let js = build_geometry_js(&["div".to_owned()], true);
        assert!(js.contains(super::super::js_helpers::JSON_SENTINEL));
    }

    #[test]
    fn build_geometry_js_escapes_special_chars() {
        let selectors = vec!["[data-id=\"foo\"]".to_owned()];
        let js = build_geometry_js(&selectors, true);
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
        let js = build_geometry_js(&["div".to_owned()], true);
        assert!(js.contains("getBoundingClientRect"));
        assert!(js.contains("getComputedStyle"));
        assert!(js.contains("flexDirection"));
        assert!(js.contains("gridTemplateColumns"));
        assert!(js.contains("fontSize"));
    }

    #[test]
    fn build_geometry_js_visible_only_false_uses_false_literal() {
        let js = build_geometry_js(&["div".to_owned()], false);
        assert!(js.contains("var visibleOnly = false;"));
        assert!(!js.contains("__VISIBLE_ONLY__"));
    }

    #[test]
    fn build_geometry_js_visible_only_true_uses_true_literal() {
        let js = build_geometry_js(&["div".to_owned()], true);
        assert!(js.contains("var visibleOnly = true;"));
        assert!(!js.contains("__VISIBLE_ONLY__"));
    }

    #[test]
    fn build_geometry_js_visible_only_includes_filter_guard() {
        let js = build_geometry_js(&["div".to_owned()], true);
        assert!(js.contains("if (visibleOnly && !vis) { continue; }"));
    }

    #[test]
    fn build_set_viewport_js_substitutes_width() {
        let js = build_set_viewport_js(320);
        assert!(js.contains("var w = 320;"), "expected width substitution");
        assert!(!js.contains("__WIDTH__"), "placeholder should be replaced");
    }

    // ── media-query self-check (iter-98 Theme A) ─────────────────────────────

    #[test]
    fn build_media_query_check_js_substitutes_width_and_probes_matchmedia() {
        let js = build_media_query_check_js(390);
        assert!(
            js.contains("var w = 390;"),
            "expected width substitution: {js}"
        );
        assert!(!js.contains("__WIDTH__"), "placeholder should be replaced");
        assert!(
            js.contains("matchMedia('(width: ' + w + 'px)')"),
            "must probe the exact-width media query: {js}"
        );
        assert!(
            js.contains("window.innerWidth"),
            "must read innerWidth: {js}"
        );
    }

    #[test]
    fn media_query_check_to_json_shape() {
        let check = MediaQueryCheck {
            inner_width: Some(1280),
            matches: Some(false),
        };
        let out = check.to_json(390);
        assert_eq!(out["requested"], 390);
        assert_eq!(out["inner_width"], 1280);
        assert_eq!(out["matches"], false);
    }

    #[test]
    fn media_query_check_to_json_defaults_are_null() {
        let check = MediaQueryCheck::default();
        let out = check.to_json(320);
        assert_eq!(out["requested"], 320);
        assert!(out["inner_width"].is_null());
        assert!(out["matches"].is_null());
    }

    #[test]
    fn build_set_viewport_js_uses_important() {
        // The `!important` flag is critical to override site stylesheets that
        // set a width on <html> or <body>.
        let js = build_set_viewport_js(768);
        assert!(js.contains("'important'"), "must use !important");
    }

    #[test]
    fn restore_viewport_css_js_removes_all_properties() {
        // Each property set by SET_VIEWPORT_CSS_JS must have a matching remove.
        assert!(RESTORE_VIEWPORT_CSS_JS.contains("removeProperty('width')"));
        assert!(RESTORE_VIEWPORT_CSS_JS.contains("removeProperty('max-width')"));
        assert!(RESTORE_VIEWPORT_CSS_JS.contains("removeProperty('overflow-x')"));
    }

    #[test]
    fn wait_layout_stable_js_returns_promise() {
        assert!(WAIT_LAYOUT_STABLE_JS.contains("Promise"));
        assert!(WAIT_LAYOUT_STABLE_JS.contains("requestAnimationFrame"));
    }

    #[test]
    fn geometry_js_uses_offset_width_for_vw() {
        // `offsetWidth` must be used (not `innerWidth`) so that the CSS
        // constraint applied by SET_VIEWPORT_CSS_JS is reflected in `vw`.
        assert!(RESPONSIVE_JS_TEMPLATE.contains("documentElement.offsetWidth"));
        assert!(!RESPONSIVE_JS_TEMPLATE.starts_with("window.innerWidth"));
    }

    // ── render_responsive_text ───────────────────────────────────────────────

    #[test]
    fn render_responsive_text_does_not_panic_with_no_breakpoints() {
        render_responsive_text(&serde_json::json!({"breakpoints": []}));
    }

    #[test]
    fn render_responsive_text_does_not_panic_with_full_data() {
        let data = serde_json::json!({
            "breakpoints": [
                {
                    "width": 320,
                    "viewport": {"width": 320, "height": 768},
                    "elements": [
                        {
                            "selector": "h1",
                            "tag": "h1",
                            "rect": {"width": 300.0, "height": 40.0},
                            "visible": true,
                            "in_viewport": true,
                        },
                    ],
                },
                {
                    "width": 1024,
                    "viewport": {"width": 1024, "height": 768},
                    "elements": [],
                },
            ],
        });
        render_responsive_text(&data);
    }

    #[test]
    fn render_responsive_text_does_not_panic_with_missing_breakpoints_key() {
        render_responsive_text(&serde_json::json!({}));
    }
}
