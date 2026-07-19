use ff_rdp_core::WebConsoleActor;
use serde_json::{Value, json};

use crate::cli::args::Cli;
use crate::error::AppError;
use crate::hints::{HintContext, HintSource};
use crate::output;
use crate::output_controls::{OutputControls, SortDir};
use crate::output_pipeline::OutputPipeline;

use super::connect_tab::connect_and_get_target;
use super::js_helpers::resolve_result;

pub fn run(cli: &Cli, selector: Option<&str>, fail_only: bool) -> Result<(), AppError> {
    let mut ctx = connect_and_get_target(cli)?;
    let console_actor = ctx.target.console_actor.clone();

    let sel = selector.unwrap_or("*");
    let js = CONTRAST_JS_TEMPLATE.replace("__SELECTOR__", &super::js_helpers::escape_selector(sel));

    let eval_result = WebConsoleActor::evaluate_js_async(ctx.transport_mut(), &console_actor, &js)
        .map_err(AppError::from)?;

    if let Some(ref exc) = eval_result.exception {
        let msg = exc.message.as_deref().unwrap_or("contrast check failed");
        return Err(AppError::User(format!("contrast check failed: {msg}")));
    }

    let mut result = resolve_result(&mut ctx, &eval_result.result)?;

    let checks = match result.get_mut("checks").and_then(Value::as_array_mut) {
        Some(arr) => std::mem::take(arr),
        None => Vec::new(),
    };

    // Apply fail_only filter: use aa_large for large text, aa_normal otherwise.
    let mut filtered = apply_fail_only_filter(checks, fail_only);

    // `summary.total` is the number of elements the in-page JS sampled and
    // checked — NOT the number of results this command returns. Under
    // `--fail-only` those differ: `sampled` counts every examined element while
    // the top-level `total` counts only returned failures (iter-127).
    let sampled = result
        .get("summary")
        .and_then(|s| s.get("total"))
        .and_then(Value::as_u64)
        .and_then(|v| usize::try_from(v).ok())
        .unwrap_or(0);

    let summary = result.get("summary").cloned().unwrap_or(json!({}));

    let mut meta = json!({
        "summary": summary,
    });
    crate::connection_meta::merge_into_if_verbose(
        &mut meta,
        &cli.host,
        cli.port,
        None,
        cli.is_verbose(),
    );

    // Apply output controls (sort, limit, fields).
    let controls = OutputControls::from_cli(cli, SortDir::Desc);
    controls.apply_sort(&mut filtered);
    let (limited, total, truncated) = controls.apply_limit(filtered, None);
    let shown = limited.len();
    let limited = controls.apply_fields(limited);

    // `total` counts what this command returns: the post-filter, pre-limit
    // population (failures under `--fail-only`, all checks otherwise). A
    // `--limit` truncates `results` but `total` still reports the full count.
    // The separate `sampled` field (below) carries the "elements examined"
    // signal that `summary.total` used to smuggle into `total` (iter-127).
    let mut envelope =
        output::envelope_with_truncation(&Value::Array(limited), shown, total, truncated, &meta);
    if let Some(obj) = envelope.as_object_mut() {
        obj.insert("sampled".to_string(), json!(sampled));
    }

    let hint_ctx = HintContext::new(HintSource::A11yContrast).with_fail_only(fail_only);
    OutputPipeline::from_cli(cli)?
        .finalize_with_hints(&envelope, Some(&hint_ctx))
        .map_err(AppError::from)
}

/// Keep only checks that fail WCAG AA when `fail_only` is set, otherwise return
/// the checks unchanged.
///
/// A check fails AA when its level-appropriate flag (`aa_large` for large text,
/// `aa_normal` otherwise) is `false`. The count of returned entries is what the
/// envelope's top-level `total` reports — distinct from the JS `summary.total`
/// sample size surfaced as `sampled` (iter-127).
fn apply_fail_only_filter(checks: Vec<Value>, fail_only: bool) -> Vec<Value> {
    if !fail_only {
        return checks;
    }
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

  for (var i = 0; i < elements.length && i < 1000; i++) {
    var el = elements[i];
    var text = el.textContent && el.textContent.trim();
    if (!text) continue;

    var cs;
    try {
      cs = window.getComputedStyle(el);
      if (cs.display === 'none' || cs.visibility === 'hidden' || cs.opacity === '0') continue;
    } catch(e) { continue; }

    // Theme J (iter-84): check both leaf elements AND elements with direct
    // text nodes.  The previous code skipped container elements without direct
    // text, but real-world contrast violations (e.g. WAI bad demo) often live
    // in `<td>` / `<li>` elements whose text is wrapped in inline elements.
    // Strategy: process any element that has text content AND whose computed
    // color is not fully transparent.
    var hasText = false;
    if (el.children.length === 0) {
      // Leaf element — always has direct text if textContent is non-empty.
      hasText = true;
    } else {
      // Check for direct text node children first.
      for (var j = 0; j < el.childNodes.length; j++) {
        if (el.childNodes[j].nodeType === 3 && el.childNodes[j].textContent.trim()) {
          hasText = true;
          break;
        }
      }
      // Also include elements where ALL children are inline (span, a, b, etc.)
      // so we don't miss styled-container contrast issues.
      if (!hasText) {
        var INLINE_TAGS = {'A': 1, 'ABBR': 1, 'B': 1, 'BDI': 1, 'BDO': 1, 'BR': 1,
          'CITE': 1, 'CODE': 1, 'DATA': 1, 'DFN': 1, 'EM': 1, 'I': 1, 'KBD': 1,
          'MARK': 1, 'Q': 1, 'RP': 1, 'RT': 1, 'RUBY': 1, 'S': 1, 'SAMP': 1,
          'SMALL': 1, 'SPAN': 1, 'STRONG': 1, 'SUB': 1, 'SUP': 1, 'TIME': 1,
          'U': 1, 'VAR': 1, 'WBR': 1, 'FONT': 1};
        var allInline = el.children.length > 0;
        for (var k = 0; k < el.children.length; k++) {
          if (!INLINE_TAGS[el.children[k].tagName]) { allInline = false; break; }
        }
        if (allInline) hasText = true;
      }
    }
    if (!hasText) continue;

    var fg = parseColor(cs.color);
    if (!fg) continue;
    // Skip fully transparent foreground colors.
    if (fg.a !== undefined && fg.a < 0.1) continue;
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
    summary: {total: checks.length, aa_pass: aaPass, aa_fail: aaFail, capped: elements.length > 1000}
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
    fn contrast_js_checks_text_nodes() {
        // Theme J (iter-84): the JS checks direct text nodes and also
        // containers with only inline children (span, a, b, etc.) so that
        // real-world contrast violations in table cells and paragraphs are found.
        assert!(CONTRAST_JS_TEMPLATE.contains("nodeType === 3"));
        assert!(CONTRAST_JS_TEMPLATE.contains("hasText"));
    }

    #[test]
    fn contrast_js_caps_element_count() {
        // Guard against hanging on massive pages.
        assert!(CONTRAST_JS_TEMPLATE.contains("i < 1000"));
    }

    // -- iter-127: honest `total` vs `sampled` ------------------------------

    /// Build a single contrast check with the given AA-normal result.
    fn passing_check(aa_normal: bool) -> Value {
        json!({
            "selector": "p",
            "is_large_text": false,
            "aa_normal": aa_normal,
            "aa_large": aa_normal,
        })
    }

    /// Assemble the envelope exactly as `run` does after evaluation: apply the
    /// fail-only filter, run the output controls, then build the truncation
    /// envelope and inject the `sampled` field. Returns the top-level envelope.
    ///
    /// This mirrors `a11y_contrast::run`'s post-eval body so the count contract
    /// can be pinned without a live Firefox instance.
    fn assemble_envelope(checks: Vec<Value>, sampled: usize, fail_only: bool) -> Value {
        let filtered = apply_fail_only_filter(checks, fail_only);
        // No sort/limit/fields controls in this harness — mirror the default
        // (no --limit) path where `total == filtered.len()`.
        let total = filtered.len();
        let shown = filtered.len();
        let meta = json!({});
        let mut envelope =
            output::envelope_with_truncation(&Value::Array(filtered), shown, total, false, &meta);
        if let Some(obj) = envelope.as_object_mut() {
            obj.insert("sampled".to_string(), json!(sampled));
        }
        envelope
    }

    #[test]
    fn fail_only_all_passing_reports_zero_total_and_sample_size() {
        // sampled = 4, all pass AA -> 0 failures.
        let checks = vec![
            passing_check(true),
            passing_check(true),
            passing_check(true),
            passing_check(true),
        ];
        let env = assemble_envelope(checks, 4, true);
        assert_eq!(
            env["total"], 0,
            "--fail-only with zero failures must report total == 0, not the sample size"
        );
        assert_eq!(
            env["results"].as_array().map(Vec::len),
            Some(0),
            "results must be empty when nothing fails AA"
        );
        assert_eq!(
            env["sampled"], 4,
            "sampled must carry the number of examined elements"
        );
    }

    #[test]
    fn fail_only_reports_failure_count_not_sample_size() {
        // sampled = 500, of which 447 fail AA.
        let mut checks: Vec<Value> = Vec::with_capacity(500);
        for _ in 0..447 {
            checks.push(passing_check(false)); // aa_normal == false -> fails
        }
        for _ in 0..53 {
            checks.push(passing_check(true)); // passes AA
        }
        let env = assemble_envelope(checks, 500, true);
        assert_eq!(
            env["total"], 447,
            "--fail-only must report the failure count (447), not the sample size (500)"
        );
        assert_eq!(
            env["results"].as_array().map(Vec::len),
            Some(447),
            "results length must equal the failure count"
        );
        assert_eq!(env["sampled"], 500, "sampled must equal the examined count");
    }

    #[test]
    fn without_fail_only_total_equals_sampled() {
        // Without --fail-only, `total` counts every check and equals `sampled`.
        let checks = vec![
            passing_check(true),
            passing_check(false),
            passing_check(true),
        ];
        let env = assemble_envelope(checks, 3, false);
        assert_eq!(
            env["total"], 3,
            "total counts all checks without --fail-only"
        );
        assert_eq!(
            env["total"], env["sampled"],
            "total must equal sampled when not filtering"
        );
    }

    #[test]
    fn fail_only_filter_uses_large_threshold_for_large_text() {
        // Large text uses aa_large; a large-text check that passes aa_large but
        // fails aa_normal must NOT be counted as a failure.
        let large_pass = json!({
            "selector": "h1",
            "is_large_text": true,
            "aa_normal": false,
            "aa_large": true,
        });
        let filtered = apply_fail_only_filter(vec![large_pass], true);
        assert!(
            filtered.is_empty(),
            "large text passing aa_large is not an AA failure"
        );
    }
}
