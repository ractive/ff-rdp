use std::time::Duration;

use anyhow::Context;
use ff_rdp_core::{ActorId, Grip, LongStringActor, WindowGlobalTarget};
use serde_json::{Value, json};

use crate::cli::args::Cli;
use crate::error::AppError;
use crate::output;
use crate::output_pipeline::OutputPipeline;

use super::connect_tab::{ConnectedTab, connect_and_get_target};
use super::eval_helpers::eval_or_user_error;
use super::perf::{compute_cls, compute_fcp, compute_lcp, compute_tbt, compute_ttfb, round2};
use super::polling::{POLL_INTERVAL_MS, poll_js_condition};
use super::url_validation::validate_url;

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

/// Navigate to `url`, wait for `document.readyState === 'complete'`, then sleep
/// 200 ms to let `PerformanceObserver` entries settle.
fn navigate_and_wait(
    ctx: &mut ConnectedTab,
    target_actor: &ActorId,
    url: &str,
    timeout_ms: u64,
) -> Result<(), AppError> {
    WindowGlobalTarget::navigate_to(ctx.transport_mut(), target_actor, url)
        .map_err(AppError::from)?;

    // Poll readyState until complete.
    let console_actor = ctx.target.console_actor.clone();
    let poll_result = poll_js_condition(
        ctx.transport_mut(),
        &console_actor,
        "document.readyState === 'complete'",
        timeout_ms,
        POLL_INTERVAL_MS,
    )?;

    if !poll_result.matched {
        return Err(AppError::User(format!(
            "perf compare: page did not reach readyState=complete within {timeout_ms}ms for {url}"
        )));
    }

    // Give PerformanceObserver entries a moment to settle.
    std::thread::sleep(Duration::from_millis(200));
    Ok(())
}

/// Combined JS script that collects all CWV-relevant entry types plus resource
/// stats in a single eval, mirroring the script used by `run_vitals` / `run_audit`.
const COLLECT_SCRIPT: &str = r"(function() {
  var result = {};
  var cwvTypes = ['largest-contentful-paint', 'layout-shift', 'longtask', 'paint'];
  cwvTypes.forEach(function(type) {
    try {
      result[type] = [];
      var obs = new PerformanceObserver(function(list) {
        result[type] = result[type].concat(list.getEntries().map(function(e) { return e.toJSON(); }));
      });
      obs.observe({ type: type, buffered: true });
      obs.disconnect();
    } catch(e) {}
  });
  if (!result.paint || result.paint.length === 0) {
    result.paint = performance.getEntriesByType('paint').map(function(e) { return e.toJSON(); });
  }
  result.navigation = performance.getEntriesByType('navigation').map(function(e) { return e.toJSON(); });
  result.resource = performance.getEntriesByType('resource').map(function(e) { return e.toJSON(); });
  return JSON.stringify(result);
})()";

/// Evaluate a JS snippet and return the full string result, resolving LongString grips.
fn eval_to_json_string(
    ctx: &mut ConnectedTab,
    script: &str,
    label: &str,
) -> Result<String, AppError> {
    let console_actor = ctx.target.console_actor.clone();
    let eval_result = eval_or_user_error(ctx.transport_mut(), &console_actor, script, label)?;

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

    let vitals = json!({
        "ttfb_ms": ttfb,
        "fcp_ms": fcp,
        "lcp_ms": lcp,
        "cls": cls,
        "tbt_ms": tbt,
    });

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
pub fn run(cli: &Cli, urls: &[String], labels: Option<&[String]>) -> Result<(), AppError> {
    validate_labels(urls, labels)?;

    // Validate all URLs before connecting.
    if !cli.allow_unsafe_urls {
        for url in urls {
            validate_url(url)?;
        }
    }

    let mut ctx = connect_and_get_target(cli)?;
    let target_actor = ctx.target.actor.clone();

    let mut results: Vec<Value> = Vec::with_capacity(urls.len());

    for (i, url) in urls.iter().enumerate() {
        let lbl = label_for(urls, labels, i);

        navigate_and_wait(&mut ctx, &target_actor, url, cli.timeout)?;

        let perf_data = collect_page_perf(&mut ctx, &lbl)?;

        results.push(json!({
            "label": lbl,
            "url": url,
            "vitals": perf_data["vitals"],
            "navigation": perf_data["navigation"],
            "resources": perf_data["resources"],
        }));
    }

    let total = results.len();
    let meta = json!({"host": cli.host, "port": cli.port});
    let envelope = output::envelope(&Value::Array(results), total, &meta);

    OutputPipeline::from_cli(cli)?
        .finalize(&envelope)
        .map_err(AppError::from)
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
