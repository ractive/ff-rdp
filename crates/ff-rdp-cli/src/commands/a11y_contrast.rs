use serde_json::{Value, json};

use crate::cli::args::Cli;
use crate::error::AppError;
use crate::output;
use crate::output_controls::{OutputControls, SortDir};
use crate::output_pipeline::OutputPipeline;

use super::connect_tab::connect_and_get_target;
use super::eval_helpers::eval_or_user_error;
use super::js_helpers::resolve_result;

pub fn run(cli: &Cli, selector: Option<&str>, fail_only: bool) -> Result<(), AppError> {
    let mut ctx = connect_and_get_target(cli)?;
    let console_actor = ctx.target.console_actor.clone();

    let sel = selector.unwrap_or("*");
    let js = CONTRAST_JS_TEMPLATE.replace("__SELECTOR__", &super::js_helpers::escape_selector(sel));

    let eval_result = eval_or_user_error(
        ctx.transport_mut(),
        &console_actor,
        &js,
        "contrast check failed",
    )?;

    let mut result = resolve_result(&mut ctx, &eval_result.result)?;

    let checks = match result.get_mut("checks").and_then(Value::as_array_mut) {
        Some(arr) => std::mem::take(arr),
        None => Vec::new(),
    };

    // Apply fail_only filter: use aa_large for large text, aa_normal otherwise.
    let mut filtered: Vec<Value> = if fail_only {
        checks
            .into_iter()
            .filter(|c| {
                let is_large = c
                    .get("is_large_text")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                let key = if is_large { "aa_large" } else { "aa_normal" };
                c.get(key).and_then(Value::as_bool) == Some(false)
            })
            .collect()
    } else {
        checks
    };

    let total_count = result
        .get("summary")
        .and_then(|s| s.get("total"))
        .and_then(Value::as_u64)
        .and_then(|v| usize::try_from(v).ok())
        .unwrap_or(0);

    let summary = result.get("summary").cloned().unwrap_or(json!({}));

    let meta = json!({
        "host": cli.host,
        "port": cli.port,
        "summary": summary,
    });

    // Apply output controls (sort, limit, fields).
    let controls = OutputControls::from_cli(cli, SortDir::Desc);
    controls.apply_sort(&mut filtered);
    let (limited, total, truncated) = controls.apply_limit(filtered, None);
    let shown = limited.len();
    let limited = controls.apply_fields(limited);

    let envelope = output::envelope_with_truncation(
        &Value::Array(limited),
        shown,
        total_count.max(total),
        truncated,
        &meta,
    );

    OutputPipeline::from_cli(cli)?
        .finalize(&envelope)
        .map_err(AppError::from)
}

/// JS template for WCAG contrast ratio checking.
///
/// Walks DOM elements matching `__SELECTOR__`, computes foreground/background
/// luminance, and returns per-element contrast ratios with AA/AAA pass/fail flags.
/// `__SELECTOR__` is replaced before evaluation.
const CONTRAST_JS_TEMPLATE: &str = r#"(function() {
  function luminance(r, g, b) {
    var a = [r, g, b].map(function(v) {
      v /= 255;
      return v <= 0.03928 ? v / 12.92 : Math.pow((v + 0.055) / 1.055, 2.4);
    });
    return 0.2126 * a[0] + 0.7152 * a[1] + 0.0722 * a[2];
  }

  function parseColor(str) {
    var m = str.match(/rgba?\((\d+),\s*(\d+),\s*(\d+)(?:,\s*([\d.]+))?\)/);
    if (m) return {r: +m[1], g: +m[2], b: +m[3], a: m[4] !== undefined ? +m[4] : 1};
    return null;
  }

  function getEffectiveBg(el) {
    var cur = el;
    while (cur) {
      var cs = window.getComputedStyle(cur);
      var bg = parseColor(cs.backgroundColor);
      if (bg && bg.a > 0) return bg;
      cur = cur.parentElement;
    }
    return {r: 255, g: 255, b: 255, a: 1};
  }

  function contrastRatio(l1, l2) {
    var lighter = Math.max(l1, l2);
    var darker = Math.min(l1, l2);
    return (lighter + 0.05) / (darker + 0.05);
  }

  function toHex(c) {
    return '#' + [c.r, c.g, c.b].map(function(v) {
      return ('0' + v.toString(16)).slice(-2);
    }).join('');
  }

  var selector = "__SELECTOR__";
  var elements = document.querySelectorAll(selector);
  var checks = [];
  var aaPass = 0, aaFail = 0;

  for (var i = 0; i < elements.length && i < 500; i++) {
    var el = elements[i];
    var text = el.textContent && el.textContent.trim();
    if (!text) continue;

    try {
      var cs = window.getComputedStyle(el);
      if (cs.display === 'none' || cs.visibility === 'hidden') continue;
    } catch(e) { continue; }

    // Only check leaf text nodes or elements with direct text.
    if (el.children.length > 0) {
      var hasDirectText = false;
      for (var j = 0; j < el.childNodes.length; j++) {
        if (el.childNodes[j].nodeType === 3 && el.childNodes[j].textContent.trim()) {
          hasDirectText = true;
          break;
        }
      }
      if (!hasDirectText) continue;
    }

    var fg = parseColor(cs.color);
    if (!fg) continue;
    var bg = getEffectiveBg(el);

    var fgL = luminance(fg.r, fg.g, fg.b);
    var bgL = luminance(bg.r, bg.g, bg.b);
    var ratio = contrastRatio(fgL, bgL);
    ratio = Math.round(ratio * 100) / 100;

    var fontSize = parseFloat(cs.fontSize);
    var fontWeight = parseInt(cs.fontWeight, 10) || 400;
    var isLarge = fontSize >= 24 || (fontSize >= 18.66 && fontWeight >= 700);

    var aaNormal = ratio >= 4.5;
    var aaLarge = ratio >= 3;
    var aaaNormal = ratio >= 7;
    var aaaLarge = ratio >= 4.5;

    var aaResult = isLarge ? aaLarge : aaNormal;
    if (aaResult) aaPass++; else aaFail++;

    // Build a simple CSS selector for this element.
    var sel = el.tagName.toLowerCase();
    if (el.id) sel += '#' + el.id;
    else if (el.className && typeof el.className === 'string') {
      sel += '.' + el.className.trim().split(/\s+/).slice(0, 2).join('.');
    }

    checks.push({
      selector: sel,
      text: text.length > 80 ? text.slice(0, 80) + '...' : text,
      foreground: toHex(fg),
      background: toHex(bg),
      ratio: ratio,
      font_size: cs.fontSize,
      is_large_text: isLarge,
      aa_normal: aaNormal,
      aa_large: aaLarge,
      aaa_normal: aaaNormal,
      aaa_large: aaaLarge
    });
  }

  return '__FF_RDP_JSON__' + JSON.stringify({
    checks: checks,
    summary: {total: checks.length, aa_pass: aaPass, aa_fail: aaFail, capped: elements.length >= 500}
  });
})()"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contrast_js_template_has_selector_placeholder() {
        assert!(CONTRAST_JS_TEMPLATE.contains("__SELECTOR__"));
    }

    #[test]
    fn contrast_js_template_has_sentinel() {
        assert!(CONTRAST_JS_TEMPLATE.contains("__FF_RDP_JSON__"));
    }

    #[test]
    fn contrast_js_computes_luminance() {
        assert!(CONTRAST_JS_TEMPLATE.contains("luminance"));
        assert!(CONTRAST_JS_TEMPLATE.contains("0.2126"));
    }

    #[test]
    fn contrast_js_has_wcag_thresholds() {
        assert!(CONTRAST_JS_TEMPLATE.contains("4.5"));
        assert!(CONTRAST_JS_TEMPLATE.contains("ratio >= 3"));
        assert!(CONTRAST_JS_TEMPLATE.contains("ratio >= 7"));
    }

    #[test]
    fn contrast_js_checks_direct_text_only() {
        // Ensures we don't check containers with only child-element text.
        assert!(CONTRAST_JS_TEMPLATE.contains("hasDirectText"));
        assert!(CONTRAST_JS_TEMPLATE.contains("nodeType === 3"));
    }

    #[test]
    fn contrast_js_caps_element_count() {
        // Guard against hanging on massive pages.
        assert!(CONTRAST_JS_TEMPLATE.contains("i < 500"));
    }
}
