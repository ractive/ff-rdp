use anyhow::Context;
use ff_rdp_core::{Grip, LongStringActor, WebConsoleActor};
use serde_json::{Value, json};

use crate::cli::args::Cli;
use crate::error::AppError;
use crate::hints::{HintContext, HintSource};
use crate::output;
use crate::output_controls::{OutputControls, SortDir};
use crate::output_pipeline::OutputPipeline;

use super::connect_tab::connect_and_get_target;

/// Observer-backed entry types that cannot be fetched via `getEntriesByType` alone.
const OBSERVER_TYPES: &[&str] = &[
    "largest-contentful-paint",
    "layout-shift",
    "longtask",
    "paint",
];

/// Valid Performance API entry types accepted by the `--type` flag.
const VALID_TYPES: &[&str] = &[
    "resource",
    "navigation",
    "paint",
    "largest-contentful-paint",
    "layout-shift",
    "longtask",
];

/// Map CLI short aliases to canonical Performance API entry type names,
/// validating against the allow-list to prevent JS injection.
fn canonical_type(entry_type: &str) -> Result<&'static str, AppError> {
    let canonical = match entry_type {
        "lcp" => "largest-contentful-paint",
        "cls" => "layout-shift",
        _ => entry_type,
    };
    VALID_TYPES
        .iter()
        .find(|&&t| t == canonical)
        .copied()
        .ok_or_else(|| {
            AppError::User(format!(
                "unknown entry type {entry_type:?}; valid types: resource, navigation, paint, lcp, cls, longtask"
            ))
        })
}

/// Build a JS snippet that uses `getEntriesByType` (works for resource/navigation).
fn script_get_entries(entry_type: &str) -> String {
    format!(r#"JSON.stringify(performance.getEntriesByType("{entry_type}").map(e => e.toJSON()))"#)
}

/// Build a JS snippet that returns both resource entries and the page hostname
/// in a single eval, avoiding a separate roundtrip for third-party detection.
///
/// Safety: `entry_type` must have passed through `canonical_type()` to prevent JS injection.
fn script_get_entries_with_hostname(entry_type: &str) -> String {
    format!(
        r#"JSON.stringify({{entries: performance.getEntriesByType("{entry_type}").map(e => e.toJSON()), hostname: document.location.hostname}})"#
    )
}

/// Build a JS snippet that uses `PerformanceObserver` with `buffered: true`.
///
/// The callback fires synchronously for already-recorded entries when
/// `buffered: true` is set, so we don't need Promises or async/await.
///
/// For `largest-contentful-paint` a three-layer fallback is used:
/// 1. PerformanceObserver with buffered:true
/// 2. `performance.getEntriesByType('largest-contentful-paint')` direct query
/// 3. DOM-based approximation using the largest visible img/video/svg/canvas element
fn script_observer(entry_type: &str) -> String {
    if entry_type == "largest-contentful-paint" {
        // Single-quote raw string — no double quotes inside. The CSS attribute selector
        // intentionally omits quotes around the attribute value: [style*=background-image]
        // is valid CSS and avoids the double-quote restriction.
        return r"(function() {
  try {
    var entries = [];
    var obs = new PerformanceObserver(function(list) {
      entries = entries.concat(list.getEntries().map(function(e) { return e.toJSON(); }));
    });
    obs.observe({ type: 'largest-contentful-paint', buffered: true });
    obs.disconnect();
    if (entries.length > 0) { return JSON.stringify(entries); }
  } catch(e) {}
  // Layer 2: direct getEntriesByType query
  try {
    var direct = performance.getEntriesByType('largest-contentful-paint');
    if (direct && direct.length > 0) {
      return JSON.stringify(direct.map(function(e) { return e.toJSON(); }));
    }
  } catch(e) {}
  // Layer 3: DOM-based approximation — find largest visible img/video/svg/canvas
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
      if (area > bestArea) {
        bestArea = area;
        best = el;
      }
    });
    if (best) {
      var src = best.src || best.currentSrc || best.getAttribute('src') || '';
      var loadTime = 0;
      if (src) {
        var res = performance.getEntriesByName(src);
        if (res && res.length > 0) { loadTime = res[0].responseEnd || 0; }
      }
      return JSON.stringify([{
        entryType: 'largest-contentful-paint',
        startTime: loadTime,
        renderTime: loadTime,
        loadTime: loadTime,
        size: bestArea,
        url: src,
        element: null,
        approximate: true
      }]);
    }
  } catch(e) {}
  return JSON.stringify([]);
})()"
            .to_string();
    }

    format!(
        r"(function() {{
  try {{
    var entries = [];
    var obs = new PerformanceObserver(function(list) {{
      entries = entries.concat(list.getEntries().map(function(e) {{ return e.toJSON(); }}));
    }});
    obs.observe({{ type: '{entry_type}', buffered: true }});
    obs.disconnect();
    return JSON.stringify(entries);
  }} catch(e) {{ return JSON.stringify([]); }}
}})()"
    )
}

/// Evaluate a JS snippet and return the JSON string result, handling LongString grips.
fn eval_to_json_string(
    ctx: &mut crate::commands::connect_tab::ConnectedTab,
    script: &str,
    label: &str,
) -> Result<String, AppError> {
    let console_actor = ctx.target.console_actor.clone();
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

/// Map a raw performance entry JSON object to the canonical output shape for its type.
///
/// `nav_domain` is the navigation document's domain, used to detect third-party resources.
fn map_entry(entry_type: &str, entry: Value, nav_domain: Option<&str>) -> Value {
    let g = |key: &str| entry.get(key).cloned().unwrap_or(Value::Null);

    match entry_type {
        "resource" => {
            let transfer_size = entry
                .get("transferSize")
                .and_then(Value::as_f64)
                .unwrap_or(0.0);
            let decoded_size = entry
                .get("decodedBodySize")
                .and_then(Value::as_f64)
                .unwrap_or(0.0);
            // Heuristic: cross-origin resources without Timing-Allow-Origin have both
            // transferSize and decodedBodySize blocked (both 0), so from_cache will be
            // false even if the resource was served from cache.
            let from_cache = transfer_size == 0.0 && decoded_size > 0.0;
            let url_str = entry
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let resource_type = classify_resource_type(
                url_str,
                entry
                    .get("initiatorType")
                    .and_then(Value::as_str)
                    .unwrap_or_default(),
            );
            let third_party = nav_domain.is_some_and(|nav| {
                let res_domain = extract_domain(url_str);
                res_domain != "unknown" && res_domain != nav
            });
            json!({
                "url": g("name"),
                "initiator_type": g("initiatorType"),
                "duration_ms": g("duration"),
                "transfer_size": g("transferSize"),
                "encoded_size": g("encodedBodySize"),
                "decoded_size": g("decodedBodySize"),
                "start_time_ms": g("startTime"),
                "response_end_ms": g("responseEnd"),
                "protocol": g("nextHopProtocol"),
                "from_cache": from_cache,
                "resource_type": resource_type,
                "third_party": third_party,
            })
        }
        "navigation" => {
            let dns_ms = sub_f64(&entry, "domainLookupEnd", "domainLookupStart");
            let tls_ms = {
                let secure = entry
                    .get("secureConnectionStart")
                    .and_then(Value::as_f64)
                    .unwrap_or(0.0);
                if secure > 0.0 {
                    let connect_end = entry
                        .get("connectEnd")
                        .and_then(Value::as_f64)
                        .unwrap_or(0.0);
                    Some(round2(connect_end - secure))
                } else {
                    None // HTTP connection: no TLS
                }
            };
            let ttfb_ms = compute_ttfb(&entry);
            let download_ms = sub_f64(&entry, "responseEnd", "responseStart");
            let start_time = entry
                .get("startTime")
                .and_then(Value::as_f64)
                .unwrap_or(0.0);
            let dom_interactive_ms = entry
                .get("domInteractive")
                .and_then(Value::as_f64)
                .map(|v| round2(v - start_time));
            let dom_complete_ms = entry
                .get("domComplete")
                .and_then(Value::as_f64)
                .map(|v| round2(v - start_time));
            json!({
                "url": g("name"),
                "start_time_ms": g("startTime"),
                "duration_ms": g("duration"),
                "dns_ms": dns_ms,
                "tls_ms": tls_ms,
                "ttfb_ms": ttfb_ms,
                "download_ms": download_ms,
                "dom_interactive_ms": dom_interactive_ms,
                "dom_complete_ms": dom_complete_ms,
                "transfer_size": g("transferSize"),
                "protocol": g("nextHopProtocol"),
            })
        }
        "paint" => json!({
            "name": g("name"),
            "start_time_ms": g("startTime"),
        }),
        "largest-contentful-paint" => json!({
            "element": g("element"),
            "url": g("url"),
            "start_time_ms": g("startTime"),
            "render_time_ms": g("renderTime"),
            "load_time_ms": g("loadTime"),
            "size": g("size"),
        }),
        "layout-shift" => json!({
            "value": g("value"),
            "had_recent_input": g("hadRecentInput"),
            "start_time_ms": g("startTime"),
            "sources": g("sources"),
        }),
        "longtask" => json!({
            "name": g("name"),
            "start_time_ms": g("startTime"),
            "duration_ms": g("duration"),
        }),
        // Passthrough for unknown types
        _ => entry,
    }
}

fn sub_f64(entry: &Value, a: &str, b: &str) -> Option<f64> {
    let va = entry.get(a)?.as_f64()?;
    let vb = entry.get(b)?.as_f64()?;
    Some(round2(va - vb))
}

/// Query Performance API entries for a given type and emit the standard JSON envelope.
pub fn run(cli: &Cli, entry_type: &str, filter: Option<&str>) -> Result<(), AppError> {
    let canonical = canonical_type(entry_type)?;

    // For resource type, use a combined script that returns both entries and
    // the page hostname in a single eval (avoids a second roundtrip).
    let use_combined = canonical == "resource";
    let script = if use_combined {
        script_get_entries_with_hostname(canonical)
    } else if OBSERVER_TYPES.contains(&canonical) {
        script_observer(canonical)
    } else {
        script_get_entries(canonical)
    };

    let mut ctx = connect_and_get_target(cli)?;
    let json_str = eval_to_json_string(&mut ctx, &script, &format!("perf --type {canonical}"))?;

    let (entries, nav_domain): (Vec<Value>, Option<String>) = if use_combined {
        let combined: Value = serde_json::from_str(&json_str)
            .with_context(|| format!("perf --type {canonical}: failed to parse JSON"))
            .map_err(AppError::from)?;
        let entries = combined
            .get("entries")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let hostname = combined
            .get("hostname")
            .and_then(Value::as_str)
            .map(str::to_string);
        (entries, hostname)
    } else {
        let entries: Vec<Value> = serde_json::from_str(&json_str)
            .with_context(|| format!("perf --type {canonical}: failed to parse JSON"))
            .map_err(AppError::from)?;
        (entries, None)
    };

    let has_url = matches!(canonical, "resource" | "navigation");

    let results: Vec<Value> = entries
        .into_iter()
        .filter(|entry| {
            if let Some(f) = filter {
                if !has_url {
                    return true;
                }
                let url = entry
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                if !url.contains(f) {
                    return false;
                }
            }
            true
        })
        .map(|entry| map_entry(canonical, entry, nav_domain.as_deref()))
        .collect();

    // Apply output controls for resource entries: default sort duration_ms desc,
    // default limit 20.  Other entry types (paint, lcp, etc.) are short lists
    // that do not benefit from limiting, so we apply limit=20 only for resource.
    let default_limit = if canonical == "resource" {
        Some(20)
    } else {
        None
    };

    let controls = OutputControls::from_cli(cli, SortDir::Desc);
    let mut results = results;
    if cli.sort.is_none() && canonical == "resource" {
        let dir = controls.sort_dir;
        results.sort_by(|a, b| {
            let da = a["duration_ms"].as_f64().unwrap_or(0.0);
            let db = b["duration_ms"].as_f64().unwrap_or(0.0);
            let cmp = da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal);
            match dir {
                SortDir::Asc => cmp,
                SortDir::Desc => cmp.reverse(),
            }
        });
    } else {
        controls.apply_sort(&mut results);
    }
    let (limited, total, truncated) = controls.apply_limit(results, default_limit);
    let shown = limited.len();
    let limited = controls.apply_fields(limited);

    let meta = json!({"host": cli.host, "port": cli.port});
    let envelope =
        output::envelope_with_truncation(&json!(limited), shown, total, truncated, &meta);

    let hint_ctx = HintContext::new(HintSource::Perf);
    OutputPipeline::from_cli(cli)?
        .finalize_with_hints(&envelope, Some(&hint_ctx))
        .map_err(AppError::from)
}

/// Collect all CWV-relevant entry types in a single eval and compute Core Web Vitals.
pub fn run_vitals(cli: &Cli) -> Result<(), AppError> {
    // Use synchronous PerformanceObserver with `buffered: true`.  The
    // callback fires synchronously for already-recorded entries, so we
    // don't need Promises or async/await (which `evaluateJSAsync` doesn't
    // auto-resolve).
    let script = r"(function() {
  var entries = {};
  var types = ['largest-contentful-paint', 'layout-shift', 'longtask', 'paint'];
  types.forEach(function(type) {
    try {
      entries[type] = [];
      var obs = new PerformanceObserver(function(list) {
        entries[type] = entries[type].concat(list.getEntries().map(function(e) { return e.toJSON(); }));
      });
      obs.observe({ type: type, buffered: true });
      obs.disconnect();
    } catch(e) {}
  });
  // Fallback: if PerformanceObserver returned no paint entries, try getEntriesByType
  if (!entries.paint || entries.paint.length === 0) {
    entries.paint = performance.getEntriesByType('paint').map(function(e) { return e.toJSON(); });
  }
  // LCP layer 2: direct getEntriesByType query if observer returned nothing
  if (!entries['largest-contentful-paint'] || entries['largest-contentful-paint'].length === 0) {
    try {
      var direct = performance.getEntriesByType('largest-contentful-paint');
      if (direct && direct.length > 0) {
        entries['largest-contentful-paint'] = direct.map(function(e) { return e.toJSON(); });
      }
    } catch(e) {}
  }
  // LCP layer 3: DOM-based approximation if still empty
  if (!entries['largest-contentful-paint'] || entries['largest-contentful-paint'].length === 0) {
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
        entries['largest-contentful-paint'] = [{
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
  entries.navigation = performance.getEntriesByType('navigation').map(function(e) { return e.toJSON(); });
  entries.resource = performance.getEntriesByType('resource').map(function(e) { return e.toJSON(); });
  return JSON.stringify(entries);
})()";

    let mut ctx = connect_and_get_target(cli)?;
    let json_str = eval_to_json_string(&mut ctx, script, "perf vitals")?;

    let all: Value = serde_json::from_str(&json_str)
        .context("perf vitals: failed to parse collection JSON")
        .map_err(AppError::from)?;

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

    let mut results = json!({
        "lcp_ms": lcp,
        "lcp_rating": lcp.map(|v| rate(v, 2500.0, 4000.0)),
        "cls": cls,
        "cls_rating": rate(cls, 0.1, 0.25),
        "tbt_ms": tbt,
        "tbt_rating": rate(tbt, 200.0, 600.0),
        "fcp_ms": fcp,
        "fcp_rating": fcp.map(|v| rate(v, 1800.0, 3000.0)),
        "ttfb_ms": ttfb,
        "ttfb_rating": ttfb.map(|v| rate(v, 800.0, 1800.0)),
    });
    if lcp_approximate {
        results["lcp_approximate"] = json!(true);
        results["lcp_note"] = json!(
            "LCP estimated via DOM approximation; not available from PerformanceObserver in headless Firefox"
        );
    } else if lcp.is_none() {
        results["lcp_note"] = json!("LCP not available in headless Firefox");
    }

    // Apply --fields filtering to the single-object result before wrapping it.
    let controls = OutputControls::from_cli(cli, SortDir::Asc);
    let results = controls.apply_fields_object(results);

    let meta = json!({"host": cli.host, "port": cli.port});
    let envelope = output::envelope(&results, 1, &meta);

    let hint_ctx = HintContext::new(HintSource::PerfVitals);
    OutputPipeline::from_cli(cli)?
        .finalize_with_hints(&envelope, Some(&hint_ctx))
        .map_err(AppError::from)
}

/// Aggregate mapped performance entries by domain, returning a Vec sorted by transfer_size descending.
fn aggregate_by_domain(mapped: &[Value]) -> Vec<Value> {
    let mut domains: std::collections::BTreeMap<String, (usize, f64)> =
        std::collections::BTreeMap::new();
    for entry in mapped {
        let url = entry.get("url").and_then(Value::as_str).unwrap_or_default();
        let domain = extract_domain(url);
        let size = entry
            .get("transfer_size")
            .and_then(Value::as_f64)
            .unwrap_or(0.0);
        let d = domains.entry(domain).or_insert((0, 0.0));
        d.0 += 1;
        d.1 += size;
    }
    let mut list: Vec<Value> = domains
        .into_iter()
        .map(|(domain, (count, size))| {
            json!({"domain": domain, "requests": count, "transfer_size": round2(size)})
        })
        .collect();
    list.sort_by(|a, b| {
        let sa = a
            .get("transfer_size")
            .and_then(Value::as_f64)
            .unwrap_or(0.0);
        let sb = b
            .get("transfer_size")
            .and_then(Value::as_f64)
            .unwrap_or(0.0);
        sb.partial_cmp(&sa).unwrap_or(std::cmp::Ordering::Equal)
    });
    list
}

/// Aggregate resource summary: sizes, request counts by type, slowest resources, domain breakdown.
pub fn run_summary(cli: &Cli) -> Result<(), AppError> {
    let script = script_get_entries_with_hostname("resource");
    let mut ctx = connect_and_get_target(cli)?;
    let json_str = eval_to_json_string(&mut ctx, &script, "perf summary")?;

    let combined: Value = serde_json::from_str(&json_str)
        .context("perf summary: failed to parse JSON")
        .map_err(AppError::from)?;
    let entries: Vec<Value> = combined
        .get("entries")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let nav_domain = combined
        .get("hostname")
        .and_then(Value::as_str)
        .map(str::to_string);

    let mapped: Vec<Value> = entries
        .into_iter()
        .map(|e| map_entry("resource", e, nav_domain.as_deref()))
        .collect();

    let total_transfer_size: f64 = mapped
        .iter()
        .filter_map(|e| e.get("transfer_size").and_then(Value::as_f64))
        .sum();

    let mut by_type: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
    for entry in &mapped {
        let rtype = entry
            .get("resource_type")
            .and_then(Value::as_str)
            .unwrap_or("other")
            .to_string();
        *by_type.entry(rtype).or_insert(0) += 1;
    }

    let mut by_duration: Vec<(&Value, f64)> = mapped
        .iter()
        .map(|e| {
            (
                e,
                e.get("duration_ms").and_then(Value::as_f64).unwrap_or(0.0),
            )
        })
        .collect();
    by_duration.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    let slowest: Vec<Value> = by_duration
        .iter()
        .take(5)
        .map(|(e, _)| {
            json!({
                "url": e.get("url"),
                "duration_ms": e.get("duration_ms"),
                "transfer_size": e.get("transfer_size"),
            })
        })
        .collect();

    let domain_list = aggregate_by_domain(&mapped);

    let results = json!({
        "total_resources": mapped.len(),
        "total_transfer_size": round2(total_transfer_size),
        "requests_by_type": by_type,
        "slowest_resources": slowest,
        "domains": domain_list,
    });

    // Text short-circuit: render a human-readable summary table instead of JSON.
    if cli.format == "text" && cli.jq.is_none() {
        render_summary_text(&results);
        return Ok(());
    }

    let meta = json!({"host": cli.host, "port": cli.port});
    let envelope = output::envelope(&results, 1, &meta);

    let hint_ctx = HintContext::new(HintSource::PerfSummary);
    OutputPipeline::from_cli(cli)?
        .finalize_with_hints(&envelope, Some(&hint_ctx))
        .map_err(AppError::from)
}

/// Render a `perf summary` result as human-readable text to stdout.
fn render_summary_text(results: &Value) {
    let total_resources = results
        .get("total_resources")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let total_transfer = results
        .get("total_transfer_size")
        .and_then(Value::as_f64)
        .unwrap_or(0.0);

    println!("=== Resource Summary ===");
    println!("  Total requests:    {total_resources}");
    println!("  Total transferred: {total_transfer:.0} bytes");

    if let Some(by_type) = results.get("requests_by_type").and_then(Value::as_object) {
        println!();
        println!("=== Requests by Type ===");
        let max_type_len = by_type.keys().map(String::len).max().unwrap_or(4);
        for (rtype, count) in by_type {
            let n = count.as_u64().unwrap_or(0);
            println!("  {rtype:<max_type_len$}  {n:>4}");
        }
    }

    if let Some(domains) = results.get("domains").and_then(Value::as_array)
        && !domains.is_empty()
    {
        println!();
        println!("=== Domains (by transfer size) ===");
        let max_domain_len = domains
            .iter()
            .filter_map(|d| d.get("domain").and_then(Value::as_str))
            .map(str::len)
            .max()
            .unwrap_or(6)
            .max(6);
        println!(
            "  {:<max_domain_len$}  {:>8}  {:>14}",
            "domain", "requests", "transfer_bytes"
        );
        println!(
            "  {}  {}  {}",
            "-".repeat(max_domain_len),
            "-".repeat(8),
            "-".repeat(14)
        );
        for d in domains {
            let domain = d.get("domain").and_then(Value::as_str).unwrap_or("?");
            let req = d.get("requests").and_then(Value::as_u64).unwrap_or(0);
            let size = d
                .get("transfer_size")
                .and_then(Value::as_f64)
                .unwrap_or(0.0);
            println!("  {domain:<max_domain_len$}  {req:>8}  {size:>14.0}");
        }
    }

    if let Some(slowest) = results.get("slowest_resources").and_then(Value::as_array)
        && !slowest.is_empty()
    {
        println!();
        println!("=== Top 5 Slowest Resources ===");
        for (i, entry) in slowest.iter().enumerate() {
            let url = entry.get("url").and_then(Value::as_str).unwrap_or("?");
            let dur = entry
                .get("duration_ms")
                .and_then(Value::as_f64)
                .unwrap_or(0.0);
            let size = entry
                .get("transfer_size")
                .and_then(Value::as_f64)
                .unwrap_or(0.0);
            println!("  {}. {url}  ({dur:.0}ms, {size:.0}b)", i + 1);
        }
    }
}

/// Collect all performance data into a single structured audit report.
pub fn run_audit(cli: &Cli) -> Result<(), AppError> {
    let script = r#"(function() {
  var result = {};

  // CWV via PerformanceObserver with buffered: true
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

  // Fallback: if PerformanceObserver returned no paint entries, try getEntriesByType
  if (!result.paint || result.paint.length === 0) {
    result.paint = performance.getEntriesByType('paint').map(function(e) { return e.toJSON(); });
  }

  // LCP layer 2: direct getEntriesByType query if observer returned nothing
  if (!result['largest-contentful-paint'] || result['largest-contentful-paint'].length === 0) {
    try {
      var lcpDirect = performance.getEntriesByType('largest-contentful-paint');
      if (lcpDirect && lcpDirect.length > 0) {
        result['largest-contentful-paint'] = lcpDirect.map(function(e) { return e.toJSON(); });
      }
    } catch(e) {}
  }
  // LCP layer 3: DOM-based approximation if still empty
  if (!result['largest-contentful-paint'] || result['largest-contentful-paint'].length === 0) {
    try {
      var lcpBest = null;
      var lcpBestArea = 0;
      var lcpCandidates = Array.prototype.slice.call(
        document.querySelectorAll('img, video, svg, canvas, [style*=background-image]')
      );
      lcpCandidates.forEach(function(el) {
        var rect = el.getBoundingClientRect();
        if (rect.width <= 0 || rect.height <= 0) { return; }
        var area = rect.width * rect.height;
        if (area > lcpBestArea) { lcpBestArea = area; lcpBest = el; }
      });
      if (lcpBest) {
        var lcpSrc = lcpBest.src || lcpBest.currentSrc || lcpBest.getAttribute('src') || '';
        var lcpLoad = 0;
        if (lcpSrc) {
          var lcpRes = performance.getEntriesByName(lcpSrc);
          if (lcpRes && lcpRes.length > 0) { lcpLoad = lcpRes[0].responseEnd || 0; }
        }
        result['largest-contentful-paint'] = [{
          entryType: 'largest-contentful-paint',
          startTime: lcpLoad,
          renderTime: lcpLoad,
          loadTime: lcpLoad,
          size: lcpBestArea,
          url: lcpSrc,
          element: null,
          approximate: true
        }];
      }
    } catch(e) {}
  }

  result.navigation = performance.getEntriesByType('navigation').map(function(e) { return e.toJSON(); });
  result.resource = performance.getEntriesByType('resource').map(function(e) { return e.toJSON(); });

  // DOM stats
  result.dom = {
    node_count: document.querySelectorAll('*').length,
    document_size: document.documentElement.outerHTML.length,
    inline_script_count: document.querySelectorAll('script:not([src])').length,
    render_blocking_count: (function() {
      var count = 0;
      document.querySelectorAll('link[rel="stylesheet"], script:not([async]):not([defer]):not([type="module"])').forEach(function(el) {
        if (el.tagName === 'LINK' || (el.tagName === 'SCRIPT' && !el.src.startsWith('data:'))) count++;
      });
      return count;
    })(),
    images_without_lazy: document.querySelectorAll('img:not([loading="lazy"])').length
  };

  result.hostname = document.location.hostname;

  return JSON.stringify(result);
})()"#;

    let mut ctx = connect_and_get_target(cli)?;
    let json_str = eval_to_json_string(&mut ctx, script, "perf audit")?;

    let all: Value = serde_json::from_str(&json_str)
        .context("perf audit: failed to parse collection JSON")
        .map_err(AppError::from)?;

    let nav_domain = all
        .get("hostname")
        .and_then(Value::as_str)
        .map(str::to_string);

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

    let mut vitals = json!({
        "ttfb_ms": ttfb,
        "ttfb_rating": ttfb.map(|v| rate(v, 800.0, 1800.0)),
        "fcp_ms": fcp,
        "fcp_rating": fcp.map(|v| rate(v, 1800.0, 3000.0)),
        "lcp_ms": lcp,
        "lcp_rating": lcp.map(|v| rate(v, 2500.0, 4000.0)),
        "cls": cls,
        "cls_rating": rate(cls, 0.1, 0.25),
        "tbt_ms": tbt,
        "tbt_rating": rate(tbt, 200.0, 600.0),
    });
    if lcp_approximate {
        vitals["lcp_approximate"] = json!(true);
        vitals["lcp_note"] = json!(
            "LCP estimated via DOM approximation; not available from PerformanceObserver in headless Firefox"
        );
    } else if lcp.is_none() {
        vitals["lcp_note"] = json!("LCP not available in headless Firefox");
    }

    // ── navigation entry ──────────────────────────────────────────────────────
    let navigation = nav.cloned().map(|e| map_entry("navigation", e, None));

    // ── resource breakdown ────────────────────────────────────────────────────
    let raw_resources: Vec<Value> = all
        .get("resource")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    let mapped_resources: Vec<Value> = raw_resources
        .into_iter()
        .map(|e| map_entry("resource", e, nav_domain.as_deref()))
        .collect();

    let total_count = mapped_resources.len();
    let total_transfer_size: f64 = mapped_resources
        .iter()
        .filter_map(|e| e.get("transfer_size").and_then(Value::as_f64))
        .sum();

    let resource_summary = json!({
        "count": total_count,
        "transfer_size": round2(total_transfer_size),
    });

    // ── resource_by_type ──────────────────────────────────────────────────────
    let mut by_type: std::collections::BTreeMap<&str, (usize, f64)> =
        std::collections::BTreeMap::new();
    for entry in &mapped_resources {
        let rtype = entry
            .get("resource_type")
            .and_then(Value::as_str)
            .unwrap_or("other");
        let size = entry
            .get("transfer_size")
            .and_then(Value::as_f64)
            .unwrap_or(0.0);
        let d = by_type.entry(rtype).or_insert((0, 0.0));
        d.0 += 1;
        d.1 += size;
    }
    let resource_by_type: Vec<Value> = by_type
        .into_iter()
        .map(|(rtype, (count, size))| {
            json!({"type": rtype, "count": count, "transfer_size": round2(size)})
        })
        .collect();

    // ── resource_by_domain (top 10) ───────────────────────────────────────────
    let domain_list = aggregate_by_domain(&mapped_resources);
    let resource_by_domain: Vec<Value> = domain_list.into_iter().take(10).collect();

    // ── third_party_summary ───────────────────────────────────────────────────
    let third_party_resources: Vec<&Value> = mapped_resources
        .iter()
        .filter(|e| {
            e.get("third_party")
                .and_then(Value::as_bool)
                .unwrap_or(false)
        })
        .collect();
    let third_party_count = third_party_resources.len();
    let third_party_transfer_size: f64 = third_party_resources
        .iter()
        .filter_map(|e| e.get("transfer_size").and_then(Value::as_f64))
        .sum();
    let third_party_summary = json!({
        "count": third_party_count,
        "transfer_size": round2(third_party_transfer_size),
    });

    // ── slowest_resources (top 5 by duration_ms) ─────────────────────────────
    let mut by_duration: Vec<(&Value, f64)> = mapped_resources
        .iter()
        .map(|e| {
            (
                e,
                e.get("duration_ms").and_then(Value::as_f64).unwrap_or(0.0),
            )
        })
        .collect();
    by_duration.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    let slowest_resources: Vec<Value> = by_duration
        .iter()
        .take(5)
        .map(|(e, _)| {
            json!({
                "url": e.get("url"),
                "duration_ms": e.get("duration_ms"),
                "transfer_size": e.get("transfer_size"),
                "resource_type": e.get("resource_type"),
            })
        })
        .collect();

    // ── dom_stats ─────────────────────────────────────────────────────────────
    let dom_stats = all.get("dom").cloned().unwrap_or(Value::Null);

    let results = json!({
        "vitals": vitals,
        "navigation": navigation,
        "resource_summary": resource_summary,
        "resource_by_type": resource_by_type,
        "resource_by_domain": resource_by_domain,
        "third_party_summary": third_party_summary,
        "slowest_resources": slowest_resources,
        "dom_stats": dom_stats,
    });

    // ── text output short-circuit ─────────────────────────────────────────────
    if cli.format == "text" && cli.jq.is_none() {
        render_audit_text(&results);
        return Ok(());
    }

    let meta = json!({"host": cli.host, "port": cli.port});
    let envelope = output::envelope(&results, 1, &meta);

    let hint_ctx = HintContext::new(HintSource::PerfAudit);
    OutputPipeline::from_cli(cli)?
        .finalize_with_hints(&envelope, Some(&hint_ctx))
        .map_err(AppError::from)
}

/// Render an audit result as human-readable text to stdout.
///
/// Does not panic on missing fields — all accesses use safe fallbacks.
fn render_audit_text(results: &Value) {
    let vitals = &results["vitals"];

    println!("=== Core Web Vitals ===");
    let metrics = [
        ("FCP", "fcp_ms", "fcp_rating", "ms"),
        ("LCP", "lcp_ms", "lcp_rating", "ms"),
        ("CLS", "cls", "cls_rating", ""),
        ("TBT", "tbt_ms", "tbt_rating", "ms"),
        ("TTFB", "ttfb_ms", "ttfb_rating", "ms"),
    ];
    for (label, val_key, rating_key, unit) in &metrics {
        let val = vitals.get(val_key).and_then(Value::as_f64);
        let rating = vitals
            .get(rating_key)
            .and_then(Value::as_str)
            .unwrap_or("n/a");
        match val {
            Some(v) => println!("  {label:5}  {v:>10.2}{unit:2}  [{rating}]"),
            None => println!("  {label:5}  {:>12}  [{rating}]", "n/a"),
        }
    }
    if let Some(note) = vitals.get("lcp_note").and_then(Value::as_str) {
        println!("  note: {note}");
    }

    println!();
    println!("=== Flagged Issues ===");
    let mut flagged = false;
    for (label, val_key, rating_key, unit) in &metrics {
        let rating = vitals
            .get(rating_key)
            .and_then(Value::as_str)
            .unwrap_or("n/a");
        if rating == "needs-improvement" || rating == "poor" {
            let val = vitals.get(val_key).and_then(Value::as_f64).unwrap_or(0.0);
            println!("  {label}: {val:.2}{unit} ({rating})");
            flagged = true;
        }
    }
    if !flagged {
        println!("  None");
    }

    println!();
    println!("=== Resources ===");
    let res_summary = &results["resource_summary"];
    let total_count = res_summary
        .get("count")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let total_size = res_summary
        .get("transfer_size")
        .and_then(Value::as_f64)
        .unwrap_or(0.0);
    println!("  Total: {total_count} requests, {total_size:.0} bytes transferred");

    if let Some(by_type) = results.get("resource_by_type").and_then(Value::as_array) {
        println!("  By type:");
        for entry in by_type {
            let rtype = entry.get("type").and_then(Value::as_str).unwrap_or("?");
            let count = entry.get("count").and_then(Value::as_u64).unwrap_or(0);
            let size = entry
                .get("transfer_size")
                .and_then(Value::as_f64)
                .unwrap_or(0.0);
            println!("    {rtype:12}  {count:>4} requests  {size:>10.0} bytes");
        }
    }

    if let Some(slowest) = results.get("slowest_resources").and_then(Value::as_array) {
        println!("  Top 5 slowest:");
        for (i, entry) in slowest.iter().enumerate() {
            let url = entry.get("url").and_then(Value::as_str).unwrap_or("?");
            let dur = entry
                .get("duration_ms")
                .and_then(Value::as_f64)
                .unwrap_or(0.0);
            let size = entry
                .get("transfer_size")
                .and_then(Value::as_f64)
                .unwrap_or(0.0);
            println!("    {}. {url}  ({dur:.0}ms, {size:.0}b)", i + 1);
        }
    }

    println!();
    println!("=== DOM Stats ===");
    let dom = &results["dom_stats"];
    let node_count = dom.get("node_count").and_then(Value::as_u64).unwrap_or(0);
    let doc_size = dom
        .get("document_size")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let inline_scripts = dom
        .get("inline_script_count")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let render_blocking = dom
        .get("render_blocking_count")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let no_lazy = dom
        .get("images_without_lazy")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    println!("  DOM nodes:              {node_count}");
    println!("  Document size (bytes):  {doc_size}");
    println!("  Inline scripts:         {inline_scripts}");
    println!("  Render-blocking:        {render_blocking}");
    println!("  Images without lazy:    {no_lazy}");
}

/// Group performance entries by domain, showing count and total transfer size per domain.
pub fn run_group_by_domain(
    cli: &Cli,
    entry_type: &str,
    filter: Option<&str>,
) -> Result<(), AppError> {
    let canonical = canonical_type(entry_type)?;

    if !matches!(canonical, "resource" | "navigation") {
        return Err(AppError::User(format!(
            "--group-by domain only works with resource or navigation types, not {canonical:?}"
        )));
    }

    // Both "resource" and "navigation" are getEntriesByType-compatible; the guard
    // above already rejects observer-only types, so no OBSERVER_TYPES check needed.
    let use_combined = canonical == "resource";
    let script = if use_combined {
        script_get_entries_with_hostname(canonical)
    } else {
        script_get_entries(canonical)
    };

    let mut ctx = connect_and_get_target(cli)?;
    let json_str = eval_to_json_string(
        &mut ctx,
        &script,
        &format!("perf --type {canonical} --group-by domain"),
    )?;

    let (entries, nav_domain): (Vec<Value>, Option<String>) = if use_combined {
        let combined: Value = serde_json::from_str(&json_str)
            .with_context(|| format!("perf --type {canonical}: failed to parse JSON"))
            .map_err(AppError::from)?;
        let entries = combined
            .get("entries")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let hostname = combined
            .get("hostname")
            .and_then(Value::as_str)
            .map(str::to_string);
        (entries, hostname)
    } else {
        let entries: Vec<Value> = serde_json::from_str(&json_str)
            .with_context(|| format!("perf --type {canonical}: failed to parse JSON"))
            .map_err(AppError::from)?;
        (entries, None)
    };

    let mapped: Vec<Value> = entries
        .into_iter()
        .filter(|entry| {
            if let Some(f) = filter {
                let url = entry
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                url.contains(f)
            } else {
                true
            }
        })
        .map(|entry| map_entry(canonical, entry, nav_domain.as_deref()))
        .collect();

    let results = aggregate_by_domain(&mapped);
    let total = results.len();
    let meta = json!({"host": cli.host, "port": cli.port});
    let envelope = output::envelope(&json!(results), total, &meta);

    let hint_ctx = HintContext::new(HintSource::Perf);
    OutputPipeline::from_cli(cli)?
        .finalize_with_hints(&envelope, Some(&hint_ctx))
        .map_err(AppError::from)
}

// ── CWV computation helpers ──────────────────────────────────────────────────

pub(crate) fn compute_ttfb(nav: &Value) -> Option<f64> {
    let response_start = nav.get("responseStart")?.as_f64()?;
    let activation_start = nav
        .get("activationStart")
        .and_then(Value::as_f64)
        .unwrap_or(0.0);
    Some(round2((response_start - activation_start).max(0.0)))
}

pub(crate) fn compute_fcp(paint_entries: &[Value]) -> Option<f64> {
    paint_entries
        .iter()
        .find(|e| e.get("name").and_then(Value::as_str) == Some("first-contentful-paint"))
        .and_then(|e| e.get("startTime"))
        .and_then(Value::as_f64)
        .map(round2)
}

pub(crate) fn compute_lcp(lcp_entries: &[Value]) -> Option<f64> {
    lcp_entries
        .last()
        .and_then(|e| e.get("startTime"))
        .and_then(Value::as_f64)
        .map(round2)
}

/// Returns true if the last LCP entry was produced by the DOM-based approximation
/// (i.e. has `approximate: true`).
pub(crate) fn is_lcp_approximate(lcp_entries: &[Value]) -> bool {
    lcp_entries
        .last()
        .and_then(|e| e.get("approximate"))
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

pub(crate) fn compute_cls(layout_shifts: &[Value]) -> f64 {
    // Session window algorithm: group shifts (excluding hadRecentInput=true) into
    // windows with max 1s gap and max 5s total duration. Return the max window sum.
    let mut shifts: Vec<(f64, f64)> = layout_shifts
        .iter()
        .filter(|e| {
            !e.get("hadRecentInput")
                .and_then(Value::as_bool)
                .unwrap_or(false)
        })
        .filter_map(|e| {
            let start = e.get("startTime")?.as_f64()?;
            let value = e.get("value")?.as_f64()?;
            Some((start, value))
        })
        .collect();
    shifts.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));

    if shifts.is_empty() {
        return 0.0;
    }

    let mut max_sum = 0.0_f64;
    let mut window_sum = 0.0_f64;
    let mut window_start = shifts[0].0;
    let mut prev_time = shifts[0].0;

    for &(time, value) in &shifts {
        // Start a new window if gap > 1s or window duration > 5s
        if time - prev_time > 1000.0 || time - window_start > 5000.0 {
            max_sum = max_sum.max(window_sum);
            window_sum = 0.0;
            window_start = time;
        }
        window_sum += value;
        prev_time = time;
    }
    max_sum = max_sum.max(window_sum);

    round2(max_sum)
}

pub(crate) fn compute_tbt(longtasks: &[Value], fcp_ms: Option<f64>) -> f64 {
    // TBT is defined as blocking time between FCP and TTI; without FCP it is meaningless
    let Some(fcp) = fcp_ms else {
        return 0.0;
    };
    let sum: f64 = longtasks
        .iter()
        .filter_map(|e| {
            let start = e.get("startTime")?.as_f64()?;
            let duration = e.get("duration")?.as_f64()?;
            let end = start + duration;
            if end <= fcp {
                return None;
            }
            // For tasks straddling FCP, only count the portion after FCP
            let effective_start = start.max(fcp);
            let effective_duration = end - effective_start;
            if effective_duration > 50.0 {
                Some(effective_duration - 50.0)
            } else {
                None
            }
        })
        .sum();
    let result = round2(sum);
    // Normalize -0.0 to 0.0: IEEE 754 treats -0.0 == 0.0, so this comparison catches it
    if result == 0.0 { 0.0 } else { result }
}

/// Classify a resource URL into a high-level type based on file extension,
/// falling back to the initiator type hint from the Performance API.
fn classify_resource_type(url: &str, initiator_type: &str) -> &'static str {
    // Try to extract the extension from the URL path (before query string)
    let path = url.split('?').next().unwrap_or(url);
    let path = path.split('#').next().unwrap_or(path);
    if let Some(dot_pos) = path.rfind('.') {
        let ext = &path[dot_pos + 1..];
        match ext {
            "js" | "mjs" | "cjs" => return "js",
            "css" => return "css",
            "png" | "jpg" | "jpeg" | "gif" | "svg" | "webp" | "avif" | "ico" | "bmp" => {
                return "image";
            }
            "woff" | "woff2" | "ttf" | "otf" | "eot" => return "font",
            "html" | "htm" => return "document",
            "json" | "xml" => return "data",
            _ => {}
        }
    }
    // Fall back to initiator type
    match initiator_type {
        "script" => "js",
        "css" | "link" => "css",
        "img" | "image" => "image",
        "font" => "font",
        "xmlhttprequest" | "fetch" => "xhr",
        "navigation" | "iframe" => "document",
        _ => "other",
    }
}

/// Extract the domain (host) from a URL string. Returns "unknown" for unparseable URLs.
fn extract_domain(url: &str) -> String {
    url::Url::parse(url).map_or_else(
        |_| "unknown".to_string(),
        |u| u.host_str().unwrap_or("unknown").to_string(),
    )
}

pub(crate) fn round2(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

pub(crate) fn rate(value: f64, good: f64, poor: f64) -> &'static str {
    if value <= good {
        "good"
    } else if value <= poor {
        "needs-improvement"
    } else {
        "poor"
    }
}
// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn approx_eq(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    fn approx_eq_opt(a: Option<f64>, b: Option<f64>) -> bool {
        match (a, b) {
            (Some(av), Some(bv)) => approx_eq(av, bv),
            (None, None) => true,
            _ => false,
        }
    }

    // ── compute_ttfb ─────────────────────────────────────────────────────────

    #[test]
    fn ttfb_basic() {
        let nav = json!({ "responseStart": 400.0, "activationStart": 0.0 });
        assert!(approx_eq_opt(compute_ttfb(&nav), Some(400.0)));
    }

    #[test]
    fn ttfb_subtracts_activation_start() {
        let nav = json!({ "responseStart": 500.0, "activationStart": 100.0 });
        assert!(approx_eq_opt(compute_ttfb(&nav), Some(400.0)));
    }

    #[test]
    fn ttfb_missing_response_start() {
        let nav = json!({ "activationStart": 0.0 });
        assert_eq!(compute_ttfb(&nav), None);
    }

    #[test]
    fn ttfb_missing_activation_start_defaults_zero() {
        let nav = json!({ "responseStart": 300.0 });
        assert!(approx_eq_opt(compute_ttfb(&nav), Some(300.0)));
    }

    // ── compute_fcp ──────────────────────────────────────────────────────────

    #[test]
    fn fcp_finds_first_contentful_paint() {
        let entries = vec![
            json!({ "name": "first-paint", "startTime": 500.0 }),
            json!({ "name": "first-contentful-paint", "startTime": 980.0 }),
        ];
        assert!(approx_eq_opt(compute_fcp(&entries), Some(980.0)));
    }

    #[test]
    fn fcp_returns_none_when_absent() {
        let entries = vec![json!({ "name": "first-paint", "startTime": 200.0 })];
        assert_eq!(compute_fcp(&entries), None);
    }

    #[test]
    fn fcp_empty_array() {
        assert_eq!(compute_fcp(&[]), None);
    }

    // ── compute_lcp ──────────────────────────────────────────────────────────

    #[test]
    fn lcp_picks_last_entry() {
        let entries = vec![
            json!({ "startTime": 1000.0 }),
            json!({ "startTime": 1850.0 }),
            json!({ "startTime": 1600.0 }),
        ];
        assert!(approx_eq_opt(compute_lcp(&entries), Some(1600.0)));
    }

    #[test]
    fn lcp_single_entry() {
        let entries = vec![json!({ "startTime": 2100.5 })];
        assert!(approx_eq_opt(compute_lcp(&entries), Some(2100.5)));
    }

    #[test]
    fn lcp_empty_array() {
        assert_eq!(compute_lcp(&[]), None);
    }

    // ── compute_cls ──────────────────────────────────────────────────────────

    #[test]
    fn cls_zero_for_empty() {
        assert!(approx_eq(compute_cls(&[]), 0.0));
    }

    #[test]
    fn cls_excludes_had_recent_input() {
        let entries = vec![
            json!({ "startTime": 100.0, "value": 0.3, "hadRecentInput": true }),
            json!({ "startTime": 200.0, "value": 0.05, "hadRecentInput": false }),
        ];
        assert!(approx_eq(compute_cls(&entries), 0.05));
    }

    #[test]
    fn cls_gap_over_1s_creates_new_window() {
        let entries = vec![
            json!({ "startTime": 0.0, "value": 0.1 }),
            json!({ "startTime": 1500.0, "value": 0.2 }), // 1.5s gap → new window
        ];
        // First window sum = 0.1, second = 0.2; max = 0.2
        assert!(approx_eq(compute_cls(&entries), 0.2));
    }

    #[test]
    fn cls_window_over_5s_creates_new_window() {
        // Entries spaced 500ms apart so the gap-check never fires (≤1s).
        // The 3rd entry starts at 5500ms which is > 5000ms from window_start=0 → new window.
        let entries = vec![
            json!({ "startTime": 0.0,    "value": 0.1 }),
            json!({ "startTime": 500.0,  "value": 0.05 }), // gap 500ms ≤ 1000ms, same window
            json!({ "startTime": 5500.0, "value": 0.3 }),  // window_dur 5500 > 5000 → new window
        ];
        // Window 1: 0.1 + 0.05 = 0.15; Window 2: 0.3; max = 0.3
        assert!(approx_eq(compute_cls(&entries), 0.3));
    }

    #[test]
    fn cls_same_window_accumulates() {
        let entries = vec![
            json!({ "startTime": 0.0, "value": 0.05 }),
            json!({ "startTime": 100.0, "value": 0.07 }),
        ];
        assert!(approx_eq(compute_cls(&entries), round2(0.12)));
    }

    // ── compute_tbt ──────────────────────────────────────────────────────────

    #[test]
    fn tbt_counts_blocking_time_after_fcp() {
        let tasks = vec![
            json!({ "startTime": 200.0, "duration": 150.0 }), // ends 350, duration > 50 → +100
            json!({ "startTime": 500.0, "duration": 80.0 }),  // ends 580, duration > 50 → +30
        ];
        assert!(approx_eq(compute_tbt(&tasks, Some(100.0)), 130.0));
    }

    #[test]
    fn tbt_ignores_tasks_shorter_than_50ms() {
        let tasks = vec![json!({ "startTime": 200.0, "duration": 40.0 })];
        assert!(approx_eq(compute_tbt(&tasks, Some(0.0)), 0.0));
    }

    #[test]
    fn tbt_task_straddling_fcp_counts_only_portion_after() {
        // Task: start=100, duration=200 → ends at 300. FCP=250.
        // Effective portion after FCP: 300-250=50ms, which is not > 50ms → TBT=0
        let tasks = vec![json!({ "startTime": 100.0, "duration": 200.0 })];
        assert!(approx_eq(compute_tbt(&tasks, Some(250.0)), 0.0));

        // Task: start=100, duration=300 → ends at 400. FCP=250.
        // Effective portion after FCP: 400-250=150ms, blocking=150-50=100ms
        let tasks2 = vec![json!({ "startTime": 100.0, "duration": 300.0 })];
        assert!(approx_eq(compute_tbt(&tasks2, Some(250.0)), 100.0));
    }

    #[test]
    fn tbt_task_ending_before_fcp_excluded() {
        // Task ends at 200ms, FCP is 300ms → not counted
        let tasks = vec![json!({ "startTime": 100.0, "duration": 100.0 })];
        assert!(approx_eq(compute_tbt(&tasks, Some(300.0)), 0.0));
    }

    #[test]
    fn tbt_empty_array() {
        assert!(approx_eq(compute_tbt(&[], None), 0.0));
    }

    // ── rate ─────────────────────────────────────────────────────────────────

    #[test]
    fn rate_good_boundary() {
        assert_eq!(rate(2500.0, 2500.0, 4000.0), "good");
    }

    #[test]
    fn rate_needs_improvement() {
        assert_eq!(rate(3000.0, 2500.0, 4000.0), "needs-improvement");
    }

    #[test]
    fn rate_poor_boundary() {
        assert_eq!(rate(4001.0, 2500.0, 4000.0), "poor");
    }

    #[test]
    fn rate_cls_thresholds() {
        assert_eq!(rate(0.05, 0.1, 0.25), "good");
        assert_eq!(rate(0.15, 0.1, 0.25), "needs-improvement");
        assert_eq!(rate(0.30, 0.1, 0.25), "poor");
    }

    // ── eval_to_json_string error path ───────────────────────────────────────

    /// When Firefox returns a Promise grip (an Object with class "Promise") the
    /// error message must name the grip type so the caller knows what went wrong
    /// rather than seeing a generic "expected string result, got: ..." message
    /// without any hint that `await` was missing from the script.
    #[test]
    fn eval_result_promise_grip_error_message_names_promise() {
        // Simulate the grip that Firefox returns when a bare `new Promise(…)` is
        // evaluated without `await`.
        let promise_grip = Grip::Object {
            actor: "conn0/obj1".into(),
            class: "Promise".to_owned(),
            preview: None,
        };

        // The error arm in eval_to_json_string formats: "{label}: expected string
        // result, got: {other.to_json()}". Verify that to_json() for a Promise grip
        // produces output that clearly identifies it as a Promise object.
        let json_repr = promise_grip.to_json();
        let repr_str = json_repr.to_string();

        assert!(
            repr_str.contains("Promise"),
            "error representation should mention 'Promise', got: {repr_str}"
        );
        assert!(
            repr_str.contains("object"),
            "error representation should mention 'object', got: {repr_str}"
        );
    }

    // ── extract_domain ───────────────────────────────────────────────────────

    #[test]
    fn extract_domain_basic() {
        assert_eq!(extract_domain("https://example.com/path"), "example.com");
        assert_eq!(
            extract_domain("https://cdn.example.com/file.js"),
            "cdn.example.com"
        );
        assert_eq!(extract_domain("not-a-url"), "unknown");
        assert_eq!(extract_domain(""), "unknown");
    }

    // ── round2 ───────────────────────────────────────────────────────────────

    #[test]
    fn round2_two_decimals() {
        // 1.235 in f64 is 1.23500000000000009... so it rounds up to 1.24
        assert!(approx_eq(round2(1.235), 1.24));
        // 1.234 truncates cleanly to 1.23
        assert!(approx_eq(round2(1.234), 1.23));
        // Whole numbers stay the same
        assert!(approx_eq(round2(100.0), 100.0));
        // Zero stays zero
        assert!(approx_eq(round2(0.0), 0.0));
    }

    // ── render_summary_text ──────────────────────────────────────────────────

    #[test]
    fn render_summary_text_does_not_panic_with_empty_data() {
        // Should complete without panicking when all fields are absent.
        render_summary_text(&serde_json::Value::Object(serde_json::Map::new()));
    }

    #[test]
    fn render_summary_text_does_not_panic_with_full_data() {
        let data = json!({
            "total_resources": 10,
            "total_transfer_size": 12345.0,
            "requests_by_type": {"js": 4, "css": 2, "image": 4},
            "domains": [
                {"domain": "example.com", "requests": 8, "transfer_size": 10000.0},
                {"domain": "cdn.example.com", "requests": 2, "transfer_size": 2345.0},
            ],
            "slowest_resources": [
                {"url": "https://example.com/app.js", "duration_ms": 300.0, "transfer_size": 50000.0},
            ],
        });
        render_summary_text(&data);
    }
}
