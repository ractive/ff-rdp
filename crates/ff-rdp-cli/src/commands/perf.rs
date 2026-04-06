use anyhow::Context;
use ff_rdp_core::{Grip, LongStringActor, WebConsoleActor};
use serde_json::{Value, json};

use crate::cli::args::Cli;
use crate::error::AppError;
use crate::output;
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

/// Build a JS snippet that uses `PerformanceObserver` with `buffered: true`.
fn script_observer(entry_type: &str) -> String {
    // Firefox's evaluateJSAsync runs expressions in an implicit async context,
    // so bare `await` is valid here.
    format!(
        r"await new Promise(resolve => {{
  try {{
    const entries = [];
    const obs = new PerformanceObserver(list => {{
      entries.push(...list.getEntries().map(e => e.toJSON()));
    }});
    obs.observe({{ type: '{entry_type}', buffered: true }});
    setTimeout(() => {{ obs.disconnect(); resolve(JSON.stringify(entries)); }}, 100);
  }} catch(e) {{ resolve(JSON.stringify([])); }}
}})"
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
fn map_entry(entry_type: &str, entry: Value) -> Value {
    let g = |key: &str| entry.get(key).cloned().unwrap_or(Value::Null);

    match entry_type {
        "resource" => json!({
            "url": g("name"),
            "initiator_type": g("initiatorType"),
            "duration_ms": g("duration"),
            "transfer_size": g("transferSize"),
            "encoded_size": g("encodedBodySize"),
            "decoded_size": g("decodedBodySize"),
            "start_time_ms": g("startTime"),
            "response_end_ms": g("responseEnd"),
            "protocol": g("nextHopProtocol"),
        }),
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

    let script = if OBSERVER_TYPES.contains(&canonical) {
        script_observer(canonical)
    } else {
        script_get_entries(canonical)
    };

    let mut ctx = connect_and_get_target(cli)?;
    let json_str = eval_to_json_string(&mut ctx, &script, &format!("perf --type {canonical}"))?;

    let entries: Vec<Value> = serde_json::from_str(&json_str)
        .with_context(|| format!("perf --type {canonical}: failed to parse JSON"))
        .map_err(AppError::from)?;

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
        .map(|entry| map_entry(canonical, entry))
        .collect();

    let total = results.len();
    let meta = json!({"host": cli.host, "port": cli.port});
    let envelope = output::envelope(&json!(results), total, &meta);

    OutputPipeline::new(cli.jq.clone())
        .finalize(&envelope)
        .map_err(AppError::from)
}

/// Collect all CWV-relevant entry types in a single eval and compute Core Web Vitals.
pub fn run_vitals(cli: &Cli) -> Result<(), AppError> {
    // Firefox's evaluateJSAsync runs expressions in an implicit async context,
    // so bare `await` is valid here.
    let script = r"await new Promise(resolve => {
  const entries = {};
  const observers = [];
  const observerTypes = ['largest-contentful-paint', 'layout-shift', 'longtask', 'paint'];
  observerTypes.forEach(type => {
    try {
      entries[type] = [];
      const obs = new PerformanceObserver(list => {
        entries[type].push(...list.getEntries().map(e => e.toJSON()));
      });
      obs.observe({ type, buffered: true });
      observers.push(obs);
    } catch(e) {}
  });
  entries.navigation = performance.getEntriesByType('navigation').map(e => e.toJSON());
  entries.resource = performance.getEntriesByType('resource').map(e => e.toJSON());
  setTimeout(() => { observers.forEach(o => o.disconnect()); resolve(JSON.stringify(entries)); }, 100);
})";

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

    let results = json!({
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

    let meta = json!({"host": cli.host, "port": cli.port});
    let envelope = output::envelope(&results, 1, &meta);

    OutputPipeline::new(cli.jq.clone())
        .finalize(&envelope)
        .map_err(AppError::from)
}

/// Aggregate resource summary: sizes, request counts by type, slowest resources, domain breakdown.
pub fn run_summary(cli: &Cli) -> Result<(), AppError> {
    let script = script_get_entries("resource");
    let mut ctx = connect_and_get_target(cli)?;
    let json_str = eval_to_json_string(&mut ctx, &script, "perf summary")?;

    let entries: Vec<Value> = serde_json::from_str(&json_str)
        .context("perf summary: failed to parse JSON")
        .map_err(AppError::from)?;

    let mapped: Vec<Value> = entries
        .into_iter()
        .map(|e| map_entry("resource", e))
        .collect();

    let total_transfer_size: f64 = mapped
        .iter()
        .filter_map(|e| e.get("transfer_size").and_then(Value::as_f64))
        .sum();

    let mut by_type: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
    for entry in &mapped {
        let itype = entry
            .get("initiator_type")
            .and_then(Value::as_str)
            .unwrap_or("other")
            .to_string();
        *by_type.entry(itype).or_insert(0) += 1;
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

    let mut domains: std::collections::BTreeMap<String, (usize, f64)> =
        std::collections::BTreeMap::new();
    for entry in &mapped {
        let url = entry.get("url").and_then(Value::as_str).unwrap_or_default();
        let domain = extract_domain(url);
        let size = entry
            .get("transfer_size")
            .and_then(Value::as_f64)
            .unwrap_or(0.0);
        let entry_data = domains.entry(domain).or_insert((0, 0.0));
        entry_data.0 += 1;
        entry_data.1 += size;
    }
    let mut domain_list: Vec<Value> = domains
        .into_iter()
        .map(|(domain, (count, size))| {
            json!({"domain": domain, "requests": count, "transfer_size": round2(size)})
        })
        .collect();
    domain_list.sort_by(|a, b| {
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

    let results = json!({
        "total_resources": mapped.len(),
        "total_transfer_size": round2(total_transfer_size),
        "requests_by_type": by_type,
        "slowest_resources": slowest,
        "domains": domain_list,
    });

    let meta = json!({"host": cli.host, "port": cli.port});
    let envelope = output::envelope(&results, 1, &meta);

    OutputPipeline::new(cli.jq.clone())
        .finalize(&envelope)
        .map_err(AppError::from)
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

    let script = if OBSERVER_TYPES.contains(&canonical) {
        script_observer(canonical)
    } else {
        script_get_entries(canonical)
    };

    let mut ctx = connect_and_get_target(cli)?;
    let json_str = eval_to_json_string(
        &mut ctx,
        &script,
        &format!("perf --type {canonical} --group-by domain"),
    )?;

    let entries: Vec<Value> = serde_json::from_str(&json_str)
        .with_context(|| format!("perf --type {canonical}: failed to parse JSON"))
        .map_err(AppError::from)?;

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
        .map(|entry| map_entry(canonical, entry))
        .collect();

    let mut domains: std::collections::BTreeMap<String, (usize, f64)> =
        std::collections::BTreeMap::new();
    for entry in &mapped {
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

    let mut results: Vec<Value> = domains
        .into_iter()
        .map(|(domain, (count, size))| {
            json!({"domain": domain, "requests": count, "transfer_size": round2(size)})
        })
        .collect();
    results.sort_by(|a, b| {
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

    let total = results.len();
    let meta = json!({"host": cli.host, "port": cli.port});
    let envelope = output::envelope(&json!(results), total, &meta);

    OutputPipeline::new(cli.jq.clone())
        .finalize(&envelope)
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
    round2(sum)
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
}
