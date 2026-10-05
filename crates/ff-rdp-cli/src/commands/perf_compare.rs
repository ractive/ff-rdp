use std::cell::Cell;

use anyhow::Context;
use ff_rdp_core::{Grip, LongStringActor, WebConsoleActor};
use serde_json::{Value, json};

use crate::cli::args::{Cli, NetworkConditionsArgs};
use crate::error::AppError;
use crate::hints::{HintContext, HintSource};
use crate::output;
use crate::output_pipeline::OutputPipeline;

use super::connect_tab::{ConnectedTab, connect_and_get_target};
use super::perf::{
    compute_cls, compute_fcp, compute_lcp, compute_tbt, compute_ttfb, entry_type_supported,
    is_lcp_approximate, lcp_missing_note, lcp_source, round2,
};
use super::url_validation::validate_content_navigation_url;

/// Validate that the number of labels matches the number of URLs.
///
/// Returns `Ok(())` on success or `Err(AppError::User(...))` on mismatch.
pub(crate) fn validate_labels(urls: &[String], labels: Option<&[String]>) -> Result<(), AppError> {
    if let Some(lbls) = labels
        && lbls.len() != urls.len()
    {
        return Err(AppError::User(format!(
            "--label count ({}) must match URL count ({})",
            lbls.len(),
            urls.len()
        )));
    }
    Ok(())
}

/// Derive the display label for the URL at position `i`.
fn label_for(urls: &[String], labels: Option<&[String]>, i: usize) -> String {
    labels
        .and_then(|lbls| lbls.get(i))
        .cloned()
        .unwrap_or_else(|| urls[i].clone())
}

/// One step of a URL's measurement in `perf compare`, named in its errors.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Step {
    /// Connecting to the tab (and `--cold`'s cache bypass), before the first
    /// URL is loaded.
    Connect,
    /// Everything up to and including sending `navigateTo` (the commit
    /// wait's watcher and resource subscriptions, then the request).
    Navigate,
    /// After `navigateTo` was sent: waiting for the new document to commit
    /// and reach `readyState === 'complete'`.
    ReadyState,
    /// The metrics collection eval.
    Collect,
}

impl Step {
    fn as_str(self) -> &'static str {
        match self {
            Self::Connect => "connect",
            Self::Navigate => "navigate",
            Self::ReadyState => "readystate",
            Self::Collect => "collect",
        }
    }
}

/// The context attached to an error from `step` while measuring `url`: which
/// URL and step it was, and that `--timeout` bounds each step, not the run.
fn step_context(url: &str, step: Step, timeout_ms: u64) -> String {
    format!(
        "perf compare: step `{}` for {url} did not finish (--timeout {timeout_ms}ms bounds \
         each step of each URL, not the whole run)",
        step.as_str()
    )
}

/// Map an error from `step` on `url` into one that names both.
///
/// A timeout (`RdpTimeout` / `Timeout`) keeps its type and exit code and gets
/// [`step_context`] as its hint; a `User` or `Connection` error keeps its type
/// and gets the URL and step prefixed. Other variants (protocol errors, a
/// destroyed actor, navigation failures) pass through unchanged: their
/// structured fields are what callers branch on, and a navigation error
/// already names the URL.
fn step_error(err: impl Into<AppError>, url: &str, step: Step, timeout_ms: u64) -> AppError {
    match err.into() {
        err @ (AppError::RdpTimeout { .. } | AppError::Timeout(_)) => {
            err.with_timeout_hint(step_context(url, step, timeout_ms))
        }
        AppError::User(msg) => AppError::User(format!(
            "perf compare: step `{}` failed for {url}: {msg}",
            step.as_str()
        )),
        AppError::Connection(msg) => AppError::Connection(format!(
            "perf compare: step `{}` failed for {url}: {msg}",
            step.as_str()
        )),
        other => other,
    }
}

/// Navigate to `url` and wait until the new document has committed and
/// reached `readyState === 'complete'`, then pause to let
/// `PerformanceObserver` entries settle.
///
/// This goes through the same commit wait as `navigate`/`reload`
/// ([`super::navigate::wait_for_navigation_commit`]), which re-resolves the
/// console actor for the new document. The former `navigateTo` +
/// `document.readyState` poll answered from the *outgoing* document (already
/// `complete`) and then sent the collection eval to its console actor, which
/// died with that document: the reply never came and the command failed with
/// a bare `RdpTimeout { phase: "recv" }` (feedback 2026-10-05).
fn navigate_and_wait(ctx: &mut ConnectedTab, url: &str, timeout_ms: u64) -> Result<(), AppError> {
    let target_actor = ctx.target().actor.clone();
    let packet = json!({"to": target_actor.as_ref(), "type": "navigateTo", "url": url});
    // Set once `navigateTo` is on the wire: an error before that (watcher
    // setup, the send itself) is step `navigate`, one after it `readystate`.
    let dispatched = Cell::new(false);
    super::navigate::wait_for_navigation_commit(
        ctx,
        timeout_ms,
        url,
        &NetworkConditionsArgs::default(),
        |transport| {
            transport.send(&packet).map_err(AppError::from)?;
            dispatched.set(true);
            Ok(())
        },
    )
    .map_err(|e| {
        let step = if dispatched.get() {
            Step::ReadyState
        } else {
            Step::Navigate
        };
        step_error(e, url, step, timeout_ms)
    })?;

    super::perf::settle_observers();
    Ok(())
}

/// Combined JS script that collects all CWV-relevant entry types plus resource
/// stats in a single eval, mirroring the script used by `run_vitals` / `run_audit`.
///
/// LCP comes from a buffered `PerformanceObserver` read with `takeRecords()`
/// (same as `run_vitals`); only where `supportedEntryTypes` lacks
/// `largest-contentful-paint` does it fall back to `getEntriesByType` and then
/// a DOM-based approximation using the largest visible img/video/svg/canvas.
const COLLECT_SCRIPT: &str = r"(function() {
  var result = {};
  // Same feature-detect + takeRecords() pattern as `perf vitals`.
  var supported =
    (typeof PerformanceObserver !== 'undefined' && PerformanceObserver.supportedEntryTypes) || [];
  var cwvTypes = ['largest-contentful-paint', 'layout-shift', 'longtask', 'paint'];
  cwvTypes.forEach(function(type) {
    result[type] = [];
    if (supported.indexOf(type) < 0) { return; }
    try {
      var obs = new PerformanceObserver(function() {});
      obs.observe({ type: type, buffered: true });
      result[type] = obs.takeRecords().map(function(e) { return e.toJSON(); });
      obs.disconnect();
    } catch(e) {}
  });
  if (!result.paint || result.paint.length === 0) {
    result.paint = performance.getEntriesByType('paint').map(function(e) { return e.toJSON(); });
  }
  // LCP layer 2: direct getEntriesByType query if observer returned nothing
  if (!result['largest-contentful-paint'] || result['largest-contentful-paint'].length === 0) {
    try {
      var direct = performance.getEntriesByType('largest-contentful-paint');
      if (direct && direct.length > 0) {
        result['largest-contentful-paint'] = direct.map(function(e) { return e.toJSON(); });
      }
    } catch(e) {}
  }
  // LCP layer 3: DOM-based approximation, only where the browser cannot report LCP
  if (supported.indexOf('largest-contentful-paint') < 0 &&
      (!result['largest-contentful-paint'] || result['largest-contentful-paint'].length === 0)) {
    try {
      var best = null;
      var bestArea = 0;
      var candidates = Array.prototype.slice.call(
        document.querySelectorAll('img, video, svg, canvas, [style*=background-image]')
      );
      candidates.forEach(function(el) {
        var rect = el.getBoundingClientRect();
        if (rect.width <= 0 || rect.height <= 0) { return; }
        var area = rect.width * rect.height;
        if (area > bestArea) { bestArea = area; best = el; }
      });
      if (best) {
        var src = best.src || best.currentSrc || best.getAttribute('src') || '';
        var loadTime = 0;
        if (src) {
          var res = performance.getEntriesByName(src);
          if (res && res.length > 0) { loadTime = res[0].responseEnd || 0; }
        }
        result['largest-contentful-paint'] = [{
          entryType: 'largest-contentful-paint',
          startTime: loadTime,
          renderTime: loadTime,
          loadTime: loadTime,
          size: bestArea,
          url: src,
          element: null,
          approximate: true
        }];
      }
    } catch(e) {}
  }
  result.navigation = performance.getEntriesByType('navigation').map(function(e) { return e.toJSON(); });
  result.resource = performance.getEntriesByType('resource').map(function(e) { return e.toJSON(); });
  // iter-139 Theme A: same structural unsupported-entry-type signal as
  // `perf vitals`/`perf audit` — `perf compare` is exactly the 'sibling'
  // surface the iteration plan calls out to check (it also derives cls/tbt
  // from layout-shift/longtask, so it had the identical false-good-number
  // exposure even though it doesn't render a `_rating` field).
  result.supported_entry_types = supported;
  return JSON.stringify(result);
})()";

/// Evaluate a JS snippet and return the full string result, resolving LongString grips.
fn eval_to_json_string(
    ctx: &mut ConnectedTab,
    script: &str,
    label: &str,
) -> Result<String, AppError> {
    let console_actor = ctx.target().console_actor.clone();
    let eval_result =
        WebConsoleActor::evaluate_js_async(ctx.transport_mut(), &console_actor, script)
            .map_err(AppError::from)?;

    if let Some(ref exc) = eval_result.exception {
        let msg = exc
            .message
            .as_deref()
            .unwrap_or("evaluation threw an exception");
        return Err(AppError::User(format!("{label}: {msg}")));
    }

    match &eval_result.result {
        Grip::Value(Value::String(s)) => Ok(s.clone()),
        Grip::LongString {
            actor,
            length,
            initial: _,
        } => LongStringActor::full_string(ctx.transport_mut(), actor.as_ref(), *length)
            .map_err(AppError::from),
        other => Err(AppError::User(format!(
            "{label}: expected string result, got: {}",
            other.to_json()
        ))),
    }
}

/// Collect performance data for the current page and return a structured JSON value.
fn collect_page_perf(ctx: &mut ConnectedTab, label: &str) -> Result<Value, AppError> {
    let json_str = eval_to_json_string(ctx, COLLECT_SCRIPT, label)?;

    let all: Value = serde_json::from_str(&json_str)
        .context("perf compare: failed to parse collection JSON")
        .map_err(AppError::from)?;

    // ── vitals ────────────────────────────────────────────────────────────────
    let nav_entries = all.get("navigation").and_then(Value::as_array);
    let nav = nav_entries.and_then(|a| a.first());

    let paint_entries: &[Value] = all
        .get("paint")
        .and_then(Value::as_array)
        .map_or(&[], Vec::as_slice);
    let lcp_entries: &[Value] = all
        .get("largest-contentful-paint")
        .and_then(Value::as_array)
        .map_or(&[], Vec::as_slice);
    let cls_entries: &[Value] = all
        .get("layout-shift")
        .and_then(Value::as_array)
        .map_or(&[], Vec::as_slice);
    let longtask_entries: &[Value] = all
        .get("longtask")
        .and_then(Value::as_array)
        .map_or(&[], Vec::as_slice);

    let ttfb = nav.and_then(compute_ttfb);
    let fcp = compute_fcp(paint_entries);
    let lcp = compute_lcp(lcp_entries);
    let cls = compute_cls(cls_entries);
    let tbt = compute_tbt(longtask_entries, fcp);
    let lcp_approximate = is_lcp_approximate(lcp_entries);
    let cls_supported = entry_type_supported(&all, "layout-shift");
    let tbt_supported = entry_type_supported(&all, "longtask");

    let mut vitals = json!({
        "ttfb_ms": ttfb,
        "fcp_ms": fcp,
        "lcp_ms": lcp,
        // iter-139 Theme A: null (not the computed 0.0) plus a note when
        // Firefox structurally cannot measure the metric — see the identical
        // guard in `perf vitals`/`perf audit`. A silent `0.0` in a
        // side-by-side comparison table would read as "both pages tied at a
        // perfect score" rather than "neither is measurable".
        "cls": if cls_supported { json!(cls) } else { Value::Null },
        "tbt_ms": if tbt_supported { json!(tbt) } else { Value::Null },
    });
    if !cls_supported {
        vitals["cls_note"] = json!(
            "cls not available — Firefox's PerformanceObserver does not support the \
             'layout-shift' entry type, so this cannot be measured (not the same as a measured 0)."
        );
    }
    if !tbt_supported {
        vitals["tbt_note"] = json!(
            "tbt_ms not available — Firefox's PerformanceObserver does not support the \
             'longtask' entry type, so this cannot be measured (not the same as a measured 0)."
        );
    }
    let lcp_supported = entry_type_supported(&all, "largest-contentful-paint");
    vitals["lcp_source"] = json!(lcp_source(lcp, lcp_approximate));
    if lcp_approximate {
        vitals["lcp_approximate"] = json!(true);
        vitals["lcp_note"] = json!(
            "LCP estimated via DOM approximation; this browser's PerformanceObserver \
             does not support the 'largest-contentful-paint' entry type"
        );
    } else if lcp.is_none() {
        vitals["lcp_note"] = json!(lcp_missing_note(lcp_supported));
    }

    // ── navigation timing ────────────────────────────────────────────────────
    let navigation = if let Some(nav_entry) = nav {
        let duration_ms = nav_entry
            .get("duration")
            .and_then(Value::as_f64)
            .map(round2);
        let transfer_size = nav_entry
            .get("transferSize")
            .and_then(Value::as_f64)
            .map(round2);
        let start_time = nav_entry
            .get("startTime")
            .and_then(Value::as_f64)
            .unwrap_or(0.0);
        let dom_interactive_ms = nav_entry
            .get("domInteractive")
            .and_then(Value::as_f64)
            .map(|v| round2(v - start_time));
        let dom_complete_ms = nav_entry
            .get("domComplete")
            .and_then(Value::as_f64)
            .map(|v| round2(v - start_time));
        json!({
            "duration_ms": duration_ms,
            "transfer_size": transfer_size,
            "dom_interactive_ms": dom_interactive_ms,
            "dom_complete_ms": dom_complete_ms,
        })
    } else {
        json!({
            "duration_ms": null,
            "transfer_size": null,
            "dom_interactive_ms": null,
            "dom_complete_ms": null,
        })
    };

    // ── resource stats ────────────────────────────────────────────────────────
    let raw_resources: &[Value] = all
        .get("resource")
        .and_then(Value::as_array)
        .map_or(&[], Vec::as_slice);

    let resource_count = raw_resources.len();
    let total_transfer_size: f64 = raw_resources
        .iter()
        .filter_map(|e| e.get("transferSize").and_then(Value::as_f64))
        .sum();

    let resources = json!({
        "count": resource_count,
        "total_transfer_size": round2(total_transfer_size),
    });

    Ok(json!({
        "vitals": vitals,
        "navigation": navigation,
        "resources": resources,
    }))
}

/// Run `ff-rdp perf compare <url1> <url2> [...]`.
pub fn run(
    cli: &Cli,
    urls: &[String],
    labels: Option<&[String]>,
    cold: bool,
) -> Result<(), AppError> {
    validate_labels(urls, labels)?;

    // Validate the entire list before connecting or navigating to even its
    // first URL. A later unsupported privileged URL must not leave earlier
    // navigations partially executed.
    for url in urls {
        validate_content_navigation_url(url, cli.allow_file_urls, cli.allow_unsafe_urls)?;
    }

    // `validate_labels`' `required = true, num_args = 2..` contract means
    // there is always a first URL; name it in a setup failure.
    let first_url = urls.first().map_or("", String::as_str);
    let mut ctx = connect_and_get_target(cli)
        .map_err(|e| step_error(e, first_url, Step::Connect, cli.timeout))?;

    // `--cold`: one `cacheDisabled` for the whole connection covers every URL
    // this command loads; Firefox restores normal caching on disconnect.
    if cold {
        super::perf::bypass_http_cache(&mut ctx)
            .map_err(|e| step_error(e, first_url, Step::Connect, cli.timeout))?;
    }

    let mut results: Vec<Value> = Vec::with_capacity(urls.len());

    for (i, url) in urls.iter().enumerate() {
        let lbl = label_for(urls, labels, i);

        navigate_and_wait(&mut ctx, url, cli.timeout)?;

        let perf_data = collect_page_perf(&mut ctx, &lbl)
            .map_err(|e| step_error(e, url, Step::Collect, cli.timeout))?;

        results.push(json!({
            "label": lbl,
            "url": url,
            "vitals": perf_data["vitals"],
            "navigation": perf_data["navigation"],
            "resources": perf_data["resources"],
        }));
    }

    let total = results.len();
    let mut meta = json!({});
    crate::connection_meta::merge_into_if_verbose(
        &mut meta,
        &cli.host,
        cli.port,
        None,
        cli.is_verbose(),
    );
    super::perf::mark_cache_bypassed(&mut meta, cold);
    let envelope = output::envelope(&Value::Array(results), total, &meta);

    let hint_ctx = HintContext::new(HintSource::Perf);
    OutputPipeline::from_cli(cli)?.finalize_with_hints(&envelope, Some(&hint_ctx))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &str) -> String {
        v.to_string()
    }

    // ── validate_labels ───────────────────────────────────────────────────────

    #[test]
    fn validate_labels_no_labels_is_ok() {
        let urls = vec![s("https://a.example"), s("https://b.example")];
        assert!(validate_labels(&urls, None).is_ok());
    }

    #[test]
    fn validate_labels_matching_count_is_ok() {
        let urls = vec![s("https://a.example"), s("https://b.example")];
        let labels = vec![s("A"), s("B")];
        assert!(validate_labels(&urls, Some(&labels)).is_ok());
    }

    #[test]
    fn validate_labels_too_few_labels_errors() {
        let urls = vec![s("https://a.example"), s("https://b.example")];
        let labels = vec![s("Only One")];
        let err = validate_labels(&urls, Some(&labels)).unwrap_err();
        assert!(matches!(err, AppError::User(_)));
        let msg = err.to_string();
        assert!(msg.contains('1'), "expected label count in error: {msg}");
        assert!(msg.contains('2'), "expected url count in error: {msg}");
    }

    #[test]
    fn validate_labels_too_many_labels_errors() {
        let urls = vec![s("https://a.example")];
        let labels = vec![s("A"), s("B"), s("C")];
        let err = validate_labels(&urls, Some(&labels)).unwrap_err();
        assert!(matches!(err, AppError::User(_)));
        let msg = err.to_string();
        assert!(msg.contains('3'), "expected label count in error: {msg}");
        assert!(msg.contains('1'), "expected url count in error: {msg}");
    }

    // ── step_error ────────────────────────────────────────────────────────────

    #[test]
    fn step_error_names_url_and_step_on_rdp_timeout() {
        let err = step_error(
            AppError::RdpTimeout {
                phase: "recv".to_owned(),
                after_ms: 1,
                hint: None,
            },
            "https://example.com/",
            Step::Navigate,
            1,
        );
        assert_eq!(err.error_type(), "Timeout");
        let msg = err.to_string();
        assert!(msg.contains("https://example.com/"), "{msg}");
        assert!(msg.contains("step `navigate`"), "{msg}");
        assert!(msg.contains("not the whole run"), "{msg}");
    }

    #[test]
    fn step_error_names_each_step() {
        for (step, name) in [
            (Step::Connect, "`connect`"),
            (Step::Navigate, "`navigate`"),
            (Step::ReadyState, "`readystate`"),
            (Step::Collect, "`collect`"),
        ] {
            let msg = step_error(
                AppError::Timeout("timed out".to_owned()),
                "https://example.org/",
                step,
                1000,
            )
            .to_string();
            assert!(msg.contains(name), "{msg}");
            assert!(msg.contains("https://example.org/"), "{msg}");
        }
    }

    #[test]
    fn step_error_prefixes_user_errors_and_keeps_other_variants() {
        let msg = step_error(
            AppError::User("eval threw".to_owned()),
            "https://a.example/",
            Step::Collect,
            1000,
        )
        .to_string();
        assert!(
            msg.contains("step `collect` failed for https://a.example/"),
            "{msg}"
        );
        assert!(msg.ends_with("eval threw"), "{msg}");

        let err = step_error(
            AppError::RdpActorDestroyed {
                actor: "conn0/tab1".to_owned(),
            },
            "https://a.example/",
            Step::Collect,
            1000,
        );
        assert_eq!(err.error_type(), "actor_destroyed");
    }

    // ── label_for ─────────────────────────────────────────────────────────────

    #[test]
    fn label_for_uses_url_when_no_labels() {
        let urls = vec![s("https://example.com"), s("https://other.com")];
        assert_eq!(label_for(&urls, None, 0), "https://example.com");
        assert_eq!(label_for(&urls, None, 1), "https://other.com");
    }

    #[test]
    fn label_for_uses_provided_label() {
        let urls = vec![s("https://example.com"), s("https://other.com")];
        let labels = vec![s("Home"), s("About")];
        assert_eq!(label_for(&urls, Some(&labels), 0), "Home");
        assert_eq!(label_for(&urls, Some(&labels), 1), "About");
    }

    #[test]
    fn label_for_falls_back_to_url_when_label_out_of_range() {
        // This shouldn't happen in practice (validate_labels catches it) but
        // the function should be safe regardless.
        let urls = vec![s("https://example.com")];
        let labels: Vec<String> = vec![];
        assert_eq!(label_for(&urls, Some(&labels), 0), "https://example.com");
    }
}
