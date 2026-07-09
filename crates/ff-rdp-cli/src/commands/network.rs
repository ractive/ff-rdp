use std::collections::HashMap;
use std::io::Write;
use std::time::Duration;

use ff_rdp_core::{
    NetworkEventActor, NetworkResource, ProtocolError, RdpTransport, TabActor, WatcherActor,
    parse_network_resource_updates, parse_network_resources,
};
use serde_json::{Value, json};

use crate::cli::args::Cli;
use crate::error::AppError;
use crate::hints::{HintContext, HintSource};
use crate::output;
use crate::output_controls::{OutputControls, SortDir};
use crate::output_pipeline::OutputPipeline;

use super::connect_tab::{ConnectedTab, connect_and_get_target};
use super::network_events::{
    build_network_entries_with_ids, drain_network_events, drain_network_from_daemon_since,
    merge_updates, performance_api_fallback,
};

/// Floor for the network-drain socket read timeout in daemon mode.
///
/// The global `--timeout` controls individual RDP read timeouts (connection
/// quality); the drain floor is independent and gives slow pages enough time
/// to deliver all buffered events before we give up.
const DAEMON_DRAIN_FLOOR_MS: u64 = 15_000;

/// Build the structured `since_requires_daemon` error (iter-101 Theme D).
fn since_requires_daemon_error() -> AppError {
    AppError::Unsupported {
        error_type: "since_requires_daemon",
        message: "network --since requires the daemon: navigation-scoped \
                  filtering is only available when the persistent daemon is \
                  buffering events.\n\
                  hint: drop --no-daemon so the command routes through the \
                  daemon, or omit --since for a one-shot capture."
            .to_owned(),
    }
}

#[allow(clippy::too_many_arguments)]
pub fn run(
    cli: &Cli,
    filter: Option<&str>,
    method: Option<&str>,
    headers: bool,
    security: bool,
    since_nav: i64,
    since_explicit: bool,
) -> Result<(), AppError> {
    // iter-101 Theme D: `--since` nav-scoping is only implemented against the
    // daemon's navigation-boundary buffer.  When the user forced direct mode
    // with `--no-daemon` there is no boundary bookkeeping, so an
    // explicitly-requested `--since` cannot be honored.  Refuse *before* opening
    // any connection — there is no point connecting just to fail — with a stable
    // `since_requires_daemon` discriminant instead of the pre-101 silent no-op.
    if since_explicit && cli.no_daemon {
        return Err(since_requires_daemon_error());
    }

    let mut ctx = connect_and_get_target(cli)?;
    let via_daemon = ctx.via_daemon;

    // Also refuse when the connection resolved to direct mode despite the daemon
    // being enabled (e.g. daemon auto-start failed and we fell back to a direct
    // connect): the buffer semantics `--since` needs still aren't present.
    if since_explicit && !via_daemon {
        return Err(since_requires_daemon_error());
    }

    let drain_timeout_ms = cli.timeout.max(DAEMON_DRAIN_FLOOR_MS);

    let (all_resources, all_updates, nav_boundary) = if ctx.via_daemon {
        // The daemon has already subscribed to network-event resources and is
        // buffering them.  Drain the buffer without touching watcher state.
        //
        // Temporarily raise the socket read timeout to the drain floor so slow
        // pages don't cause a premature timeout on the drain RPC.
        let restored_timeout = Duration::from_millis(cli.timeout);
        let drain_timeout = Duration::from_millis(drain_timeout_ms);
        let _ = ctx.transport_mut().set_read_timeout(Some(drain_timeout));
        let drain_result = drain_network_from_daemon_since(ctx.transport_mut(), since_nav);
        let _ = ctx.transport_mut().set_read_timeout(Some(restored_timeout));
        drain_result.map_err(|e| {
            // Downcast through the anyhow chain to find a ProtocolError::Timeout
            // or an io::Error with kind WouldBlock/TimedOut — both indicate the
            // socket read deadline fired rather than a real protocol failure.
            if let AppError::Internal(ref inner) = e {
                let mut is_timeout = false;
                for cause in inner.chain() {
                    if let Some(pe) = cause.downcast_ref::<ProtocolError>()
                        && matches!(pe, ProtocolError::Timeout)
                    {
                        is_timeout = true;
                        break;
                    }
                    if let Some(io_err) = cause.downcast_ref::<std::io::Error>()
                        && matches!(
                            io_err.kind(),
                            std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                        )
                    {
                        is_timeout = true;
                        break;
                    }
                }
                if is_timeout {
                    return AppError::Timeout(format!(
                        "network drain timed out — try --timeout {drain_timeout_ms}"
                    ));
                }
            }
            e
        })?
    } else {
        let tab_actor = ctx.target_tab_actor().clone();

        // Get the watcher actor for resource subscriptions.
        let watcher_actor =
            TabActor::get_watcher(ctx.transport_mut(), &tab_actor).map_err(AppError::from)?;

        // Subscribe to network events. The watchResources response from Firefox
        // 149+ includes existing network events as a `resources` field in the
        // ack itself (not as separate resources-available-array events).  We
        // parse the ack for inline resources, then drain for any subsequent
        // events (updates, late-arriving resources).
        WatcherActor::watch_resources(ctx.transport_mut(), &watcher_actor, &["network-event"])
            .map_err(AppError::from)?;

        // Collect resource events until timeout.
        let result = drain_network_events(ctx.transport_mut()).map_err(AppError::from)?;

        // Unwatch to clean up server-side resources.
        let _ = WatcherActor::unwatch_resources(
            ctx.transport_mut(),
            &watcher_actor,
            &["network-event"],
        );

        (result.0, result.1, None)
    };

    // Merge updates into resources by resource_id.
    let update_map = merge_updates(all_updates);

    // Build a map from resource_id → actor for header fetching (watcher entries only).
    let actor_by_resource_id: HashMap<u64, ff_rdp_core::ActorId> = all_resources
        .iter()
        .map(|r| (r.resource_id, r.actor.clone()))
        .collect();

    // Build JSON output combining resource + update data.
    // Entries are annotated with `_resource_id` (stripped before final output)
    // so that header fetching can look up the corresponding NetworkEventActor.
    let apply_filters =
        |entries: Vec<serde_json::Value>, with_resource_id: bool| -> Vec<serde_json::Value> {
            entries
                .into_iter()
                .filter(|entry| {
                    if let Some(f) = filter {
                        let url = entry["url"].as_str().unwrap_or_default();
                        if !url.contains(f) {
                            return false;
                        }
                    }
                    if let Some(m) = method {
                        let entry_method = entry["method"].as_str().unwrap_or_default();
                        if !entry_method.eq_ignore_ascii_case(m) {
                            return false;
                        }
                    }
                    true
                })
                .map(|mut entry| {
                    if !with_resource_id {
                        entry.as_object_mut().map(|o| o.remove("_resource_id"));
                    }
                    entry
                })
                .collect()
        };

    let watcher_entries = build_network_entries_with_ids(&all_resources, &update_map);
    let watcher_was_empty = watcher_entries.is_empty();
    // Keep resource IDs in watcher entries so detail+headers mode can fetch them.
    let filtered_watcher = apply_filters(watcher_entries, true);

    // If the watcher returned nothing (page already loaded before subscribing),
    // try the Performance API as a fallback.  In daemon mode the watcher buffer
    // may be empty because the page loaded before our drain — the Performance
    // API fallback applies equally in that case.
    let (results, used_perf_fallback) = if watcher_was_empty {
        let fallback = performance_api_fallback(&mut ctx);
        let filtered_fallback = apply_filters(fallback, false);
        let used = !filtered_fallback.is_empty();
        (filtered_fallback, used)
    } else {
        // filtered_watcher already has _resource_id present; keep it for the
        // detail+headers path. It will be stripped before final output.
        (filtered_watcher, false)
    };

    // Count plain-HTTP (insecure) requests across the *whole* captured set, not
    // just the shown/limited slice, so `--security` audits can flag mixed
    // content at a glance regardless of --limit.  The scheme comes straight
    // from the request URL, so no per-entry RPC is needed for the count.
    let insecure_requests = if security {
        Some(count_insecure_requests(&results))
    } else {
        None
    };

    // When both the watcher and the Performance API returned nothing, print a
    // hint so the user knows how to get data.
    if results.is_empty() && watcher_was_empty {
        eprintln!(
            "hint: no network events captured. \
             Navigate first or use `--follow` to stream events in real time."
        );
    }

    // Base meta.source on which buffer was actually consulted, not on the
    // post-filter result count.  Otherwise `--filter <no-match>` against a
    // non-empty watcher buffer would omit `meta.source` even though the data
    // source was watcher.
    let mut meta = if used_perf_fallback {
        json!({"source": "performance-api"})
    } else if !watcher_was_empty {
        json!({"source": "watcher"})
    } else {
        json!({})
    };
    // Include the navigation boundary that scoped the result, if any.
    if let Some(ref b) = nav_boundary
        && let Some(m) = meta.as_object_mut()
    {
        m.insert(
            "since".to_string(),
            json!({
                "index": since_nav,
                "url": b.get("url"),
                "sequence": b.get("sequence"),
            }),
        );
    }
    crate::connection_meta::merge_into_if_verbose(
        &mut meta,
        &cli.host,
        cli.port,
        None,
        cli.is_verbose(),
    );

    // Decide whether to show summary or detail mode.
    // Detail mode is used when:
    //   - --detail flag is set
    //   - --jq is set (user wants raw data to process)
    //   - --sort, --limit, --fields are explicitly set (user wants detail controls)
    let use_detail = cli.detail
        || cli.jq.is_some()
        || cli.sort.is_some()
        || cli.limit.is_some()
        || cli.all
        || cli.fields.is_some()
        || headers
        || security;

    let empty_hint = if results.is_empty() && watcher_was_empty {
        let hint = if via_daemon {
            "No network events captured. Events are buffered by the daemon; navigate first with: ff-rdp navigate <url> --with-network, or use --follow to stream events in real time."
        } else {
            "No network events captured. Connect before the page loads, use ff-rdp navigate <url> --with-network, or use --follow to stream events in real time."
        };
        Some(json!(hint))
    } else if results.is_empty() && (filter.is_some() || method.is_some()) {
        Some(json!(
            "No requests matched the current --filter/--method. Remove the filter to see all captured events."
        ))
    } else {
        None
    };

    if use_detail {
        let controls = OutputControls::from_cli(cli, SortDir::Desc);
        let mut detail = results;
        // Default sort by duration_ms desc when no explicit sort is provided.
        if cli.sort.is_none() {
            let dir = controls.sort_dir;
            detail.sort_by(|a, b| {
                let da = a["duration_ms"].as_f64().unwrap_or(0.0);
                let db = b["duration_ms"].as_f64().unwrap_or(0.0);
                let cmp = da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal);
                match dir {
                    SortDir::Asc => cmp,
                    SortDir::Desc => cmp.reverse(),
                }
            });
        } else {
            controls.apply_sort(&mut detail);
        }
        let (limited, total, truncated) = controls.apply_limit(detail, Some(20));
        let shown = limited.len();

        // Fetch request+response headers for each entry when --headers is set
        // and the entry came from the watcher (has _resource_id).  The internal
        // `_resource_id` marker is stripped once at the end, after both the
        // header and the security joins have had a chance to use it.
        let mut limited = limited;
        if headers && used_perf_fallback {
            // Performance-api source has no response headers. Emit a note per
            // entry so callers know why headers are absent; never silently drop.
            const HEADERS_NOTE: &str = "--headers ignored (performance-api source has no \
                response headers; use --with-network to engage watcher)";
            for entry in &mut limited {
                if let Some(obj) = entry.as_object_mut() {
                    obj.entry("note".to_string())
                        .and_modify(|v| {
                            // Append to existing note rather than overwrite.
                            if let Some(existing) = v.as_str() {
                                *v = json!(format!("{existing}; {HEADERS_NOTE}"));
                            } else {
                                *v = json!(HEADERS_NOTE);
                            }
                        })
                        .or_insert_with(|| json!(HEADERS_NOTE));
                }
            }
        } else if headers && !used_perf_fallback {
            for entry in &mut limited {
                if let Some(rid) = entry.get("_resource_id").and_then(Value::as_u64)
                    && let Some(actor) = actor_by_resource_id.get(&rid)
                {
                    let req_hdrs =
                        NetworkEventActor::get_request_headers(ctx.transport_mut(), actor)
                            .ok()
                            .map(|hs| {
                                hs.into_iter()
                                    .map(|h| json!({"name": h.name, "value": h.value}))
                                    .collect::<Vec<_>>()
                            })
                            .unwrap_or_default();

                    let resp_hdrs =
                        NetworkEventActor::get_response_headers(ctx.transport_mut(), actor)
                            .ok()
                            .map(|hs| {
                                hs.into_iter()
                                    .map(|h| json!({"name": h.name, "value": h.value}))
                                    .collect::<Vec<_>>()
                            })
                            .unwrap_or_default();

                    entry["headers"] = json!({"request": req_hdrs, "response": resp_hdrs});
                }
            }
        }

        // Attach per-request TLS/certificate detail when --security is set.
        // HTTPS requests get a `security` object (fetched from the
        // NetworkEventActor we already hold); plain-HTTP requests get
        // `security: null`.  Only the watcher source exposes security info; the
        // performance-api fallback gets a per-entry note instead of silently
        // dropping the flag.
        if security {
            attach_security(
                &mut limited,
                &mut ctx,
                &actor_by_resource_id,
                used_perf_fallback,
            );
        }

        // Strip the internal `_resource_id` marker now that all per-entry joins
        // are done.
        for entry in &mut limited {
            if let Some(obj) = entry.as_object_mut() {
                obj.remove("_resource_id");
            }
        }

        let limited = controls.apply_fields(limited);
        let mut envelope =
            output::envelope_with_truncation(&json!(limited), shown, total, truncated, &meta);
        if let Some(hint) = empty_hint
            && let Some(obj) = envelope.as_object_mut()
        {
            obj.insert("hint".to_string(), hint);
        }
        // Surface the mixed-content count at the top level so `--security`
        // audits can flag insecure requests without scanning every entry.
        if let Some(count) = insecure_requests
            && let Some(obj) = envelope.as_object_mut()
        {
            obj.insert("insecure_requests".to_string(), json!(count));
        }
        let hint_ctx = HintContext::new(HintSource::Network).with_detail(cli.detail);
        return OutputPipeline::from_cli(cli)?
            .finalize_with_hints(&envelope, Some(&hint_ctx))
            .map_err(AppError::from);
    }

    // Summary mode: strip _resource_id from entries before summarizing.
    let results: Vec<_> = results
        .into_iter()
        .map(|mut e| {
            if let Some(obj) = e.as_object_mut() {
                obj.remove("_resource_id");
            }
            e
        })
        .collect();

    // Summary mode (default).
    // The non-timed drain_network_events() stops on idle, so timeout is never reached.
    let summary = build_network_summary(&results, false);

    // Text short-circuit for summary mode.
    if cli.format == "text" && cli.jq.is_none() {
        render_network_summary_text(&summary);
        return Ok(());
    }

    let mut envelope = output::envelope(&summary, results.len(), &meta);
    if let Some(hint) = empty_hint
        && let Some(obj) = envelope.as_object_mut()
    {
        obj.insert("hint".to_string(), hint);
    }
    let hint_ctx = HintContext::new(HintSource::Network).with_detail(cli.detail);
    OutputPipeline::from_cli(cli)?
        .finalize_with_hints(&envelope, Some(&hint_ctx))
        .map_err(AppError::from)
}

/// Count how many entries are plain-HTTP (insecure) requests.
///
/// The classification is purely by URL scheme: a `http://` URL is insecure,
/// everything else (`https://`, `data:`, `blob:`, `about:`, …) is not counted.
/// This mirrors what a mixed-content audit cares about — HTTP subresources on
/// an HTTPS page — without needing a per-request RPC.
fn count_insecure_requests(entries: &[Value]) -> usize {
    entries
        .iter()
        .filter(|e| e["url"].as_str().is_some_and(|u| u.starts_with("http://")))
        .count()
}

/// Render a [`SecurityInfo`] as the JSON `security` object attached to a
/// request entry.
fn security_to_json(si: &ff_rdp_core::SecurityInfo) -> Value {
    let cert = si.cert.as_ref().map(|c| {
        json!({
            "subject": c.subject,
            "issuer": c.issuer,
            "validFrom": c.valid_from,
            "validTo": c.valid_to,
            "sha256Fingerprint": c.sha256_fingerprint,
        })
    });
    json!({
        "state": si.state,
        "protocolVersion": si.protocol_version,
        "cipherSuite": si.cipher_suite,
        "hsts": si.hsts,
        "weaknessReasons": si.weakness_reasons,
        "cert": cert,
    })
}

/// Attach a `security` field to each entry in `limited`.
///
/// HTTPS requests get the fetched [`SecurityInfo`] (or `null` when Firefox has
/// none — e.g. a request whose response the watcher never observed); plain-HTTP
/// requests get `security: null` without any RPC.  When the data came from the
/// performance-api fallback (no NetworkEventActor ids), every entry gets a note
/// explaining why security info is unavailable, matching the `--headers`
/// behaviour.
fn attach_security(
    limited: &mut [Value],
    ctx: &mut ConnectedTab,
    actor_by_resource_id: &HashMap<u64, ff_rdp_core::ActorId>,
    used_perf_fallback: bool,
) {
    const SECURITY_NOTE: &str = "--security ignored (performance-api source has no \
        per-request security info; use --with-network to engage the watcher)";

    for entry in limited.iter_mut() {
        if used_perf_fallback {
            if let Some(obj) = entry.as_object_mut() {
                obj.entry("note".to_string())
                    .and_modify(|v| {
                        if let Some(existing) = v.as_str() {
                            *v = json!(format!("{existing}; {SECURITY_NOTE}"));
                        } else {
                            *v = json!(SECURITY_NOTE);
                        }
                    })
                    .or_insert_with(|| json!(SECURITY_NOTE));
            }
            continue;
        }

        let is_http = entry["url"]
            .as_str()
            .is_some_and(|u| u.starts_with("http://"));
        if is_http {
            // Plain-HTTP request: no TLS, so no security object. Skip the RPC.
            entry["security"] = Value::Null;
            continue;
        }

        // HTTPS (or other secure-ish scheme): fetch security info from the
        // NetworkEventActor we already hold for this request.
        let security_value = entry
            .get("_resource_id")
            .and_then(Value::as_u64)
            .and_then(|rid| actor_by_resource_id.get(&rid))
            .and_then(|actor| {
                NetworkEventActor::get_security_info(ctx.transport_mut(), actor)
                    .ok()
                    .flatten()
            })
            .map_or(Value::Null, |si| security_to_json(&si));
        entry["security"] = security_value;
    }
}

/// Render network summary as human-readable text to `out`.
///
/// Accepts a `Write` sink so callers (and tests) can capture output without
/// spawning subprocesses.  The production path passes `&mut io::stdout()`.
///
/// Null/empty `cause_type` keys are handled as follows:
/// - If ALL keys are null (i.e. the only key is `""`) the "Requests by Cause Type"
///   section is suppressed entirely — immediately post-nav, `cause_type` may not
///   have been set yet, and a bare-number row confuses readers.
/// - For a mix of null + non-null keys, the null key is displayed as `(unknown)`.
fn render_network_summary_text_to(summary: &Value, out: &mut dyn std::io::Write) {
    let total_requests = summary
        .get("total_requests")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let total_bytes = summary
        .get("total_transfer_bytes")
        .and_then(Value::as_f64)
        .unwrap_or(0.0);

    let _ = writeln!(out, "=== Network Summary ===");
    let _ = writeln!(out, "  Total requests:    {total_requests}");
    let _ = writeln!(out, "  Total transferred: {total_bytes:.0} bytes");

    if let Some(by_cause) = summary.get("by_cause_type").and_then(Value::as_object)
        && !by_cause.is_empty()
    {
        // Suppress the section if every key is the null sentinel ("").
        let all_null = by_cause.len() == 1 && by_cause.contains_key("");
        if !all_null {
            let _ = writeln!(out);
            let _ = writeln!(out, "=== Requests by Cause Type ===");
            // For display purposes, map "" → "(unknown)" so readers see a label.
            let display_keys: Vec<String> = by_cause
                .keys()
                .map(|k| {
                    if k.is_empty() {
                        "(unknown)".to_string()
                    } else {
                        k.clone()
                    }
                })
                .collect();
            let max_len = display_keys.iter().map(String::len).max().unwrap_or(4);
            for (raw_key, count) in by_cause {
                let label = if raw_key.is_empty() {
                    "(unknown)"
                } else {
                    raw_key.as_str()
                };
                let n = count.as_u64().unwrap_or(0);
                let _ = writeln!(out, "  {label:<max_len$}  {n:>4}");
            }
        }
    }

    if let Some(slowest) = summary.get("slowest").and_then(Value::as_array)
        && !slowest.is_empty()
    {
        let _ = writeln!(out);
        let _ = writeln!(out, "=== Slowest Requests ===");
        for (i, entry) in slowest.iter().enumerate() {
            let url = entry.get("url").and_then(Value::as_str).unwrap_or("?");
            let dur = entry
                .get("duration_ms")
                .and_then(Value::as_f64)
                .unwrap_or(0.0);
            let status = entry.get("status").and_then(Value::as_u64).unwrap_or(0);
            let size = entry
                .get("transfer_size")
                .and_then(Value::as_f64)
                .unwrap_or(0.0);
            let _ = writeln!(
                out,
                "  {}. {url}  ({dur:.0}ms, {status}, {size:.0}b)",
                i + 1
            );
        }
    }

    if summary
        .get("timeout_reached")
        .and_then(Value::as_bool)
        .unwrap_or(false)
        && let Some(hint) = summary.get("hint").and_then(Value::as_str)
    {
        let _ = writeln!(out);
        let _ = writeln!(out, "{hint}");
    }
}

/// Render network summary as human-readable text to stdout.
///
/// Thin wrapper over `render_network_summary_text_to` for the production path.
fn render_network_summary_text(summary: &Value) {
    render_network_summary_text_to(summary, &mut std::io::stdout());
}

/// Build a summary view of network requests.
///
/// Returns a JSON object with:
/// - `total_requests`: total count
/// - `total_transfer_bytes`: sum of `transfer_size` across all entries
/// - `by_cause_type`: count per `cause_type` field
/// - `slowest`: top-20 slowest requests (url, duration_ms, status, transfer_size)
/// - `timeout_reached`: whether the collection deadline fired while events were still arriving
/// - `hint` (only when `timeout_reached` is true): advice to increase `--network-timeout`
pub fn build_network_summary(
    entries: &[serde_json::Value],
    timeout_reached: bool,
) -> serde_json::Value {
    let total_requests = entries.len();

    let total_transfer_bytes: f64 = entries
        .iter()
        .filter_map(|e| e["transfer_size"].as_f64())
        .sum();

    // Normalise -0.0 → 0.0: IEEE 754 defines -0.0 == 0.0, so this is safe.
    // An empty (or all-null) entries slice sums to 0.0 but floating-point
    // addition can produce negative zero in some edge cases.
    let total_transfer_bytes = if total_transfer_bytes == 0.0 {
        0.0_f64
    } else {
        total_transfer_bytes
    };

    let mut by_cause_type: std::collections::BTreeMap<String, usize> =
        std::collections::BTreeMap::new();
    for entry in entries {
        // Use "" as the null sentinel so we can distinguish "no cause_type" from
        // the literal string "other". The text renderer maps "" → "(unknown)" and
        // suppresses the section entirely when all keys are null.
        let cause = entry["cause_type"].as_str().unwrap_or("").to_string();
        *by_cause_type.entry(cause).or_insert(0) += 1;
    }

    let mut sorted_by_duration: Vec<&serde_json::Value> = entries.iter().collect();
    sorted_by_duration.sort_by(|a, b| {
        let da = a["duration_ms"].as_f64().unwrap_or(0.0);
        let db = b["duration_ms"].as_f64().unwrap_or(0.0);
        db.partial_cmp(&da).unwrap_or(std::cmp::Ordering::Equal)
    });

    let slowest: Vec<serde_json::Value> = sorted_by_duration
        .iter()
        .take(20)
        .map(|e| {
            json!({
                "url": e["url"],
                "duration_ms": e["duration_ms"],
                "status": e["status"],
                "transfer_size": e["transfer_size"],
            })
        })
        .collect();

    let mut summary = json!({
        "total_requests": total_requests,
        "total_transfer_bytes": total_transfer_bytes,
        "by_cause_type": by_cause_type,
        "slowest": slowest,
        "timeout_reached": timeout_reached,
    });
    if timeout_reached {
        summary["hint"] = json!(
            "Network collection was still receiving events when the timeout was reached. \
             Consider increasing --network-timeout for more complete results."
        );
    }
    summary
}

/// Return buffered network events as a JSON array.
///
/// Used by the script runner's `assert_network` step.
/// `drain_timeout_ms` controls how long to drain in direct mode (default: 500ms).
pub fn run_get_events(
    cli: &Cli,
    drain_timeout_ms: Option<u64>,
) -> Result<Vec<serde_json::Value>, crate::error::AppError> {
    use super::network_events::{build_network_entries, drain_network_from_daemon, merge_updates};
    use ff_rdp_core::{TabActor, WatcherActor};
    use std::time::Duration;

    let mut ctx = super::connect_tab::connect_and_get_target(cli)?;

    let entries = if ctx.via_daemon {
        let (resources, updates) = drain_network_from_daemon(ctx.transport_mut())?;
        let update_map = merge_updates(updates);
        build_network_entries(&resources, &update_map)
    } else {
        // Direct mode: subscribe, drain briefly, unsubscribe.
        let drain_ms = drain_timeout_ms.unwrap_or(500);
        let tab_actor = ctx.target_tab_actor().clone();
        let watcher_actor = TabActor::get_watcher(ctx.transport_mut(), &tab_actor)
            .map_err(crate::error::AppError::from)?;
        WatcherActor::watch_resources(ctx.transport_mut(), &watcher_actor, &["network-event"])
            .map_err(crate::error::AppError::from)?;

        let (resources, updates, _) = super::network_events::drain_network_events_timed(
            ctx.transport_mut(),
            Duration::from_millis(drain_ms),
        )
        .map_err(crate::error::AppError::from)?;

        let _ = WatcherActor::unwatch_resources(
            ctx.transport_mut(),
            &watcher_actor,
            &["network-event"],
        );

        let update_map = merge_updates(updates);
        build_network_entries(&resources, &update_map)
    };

    // Convert to plain JSON array.
    let json_entries: Vec<serde_json::Value> = entries
        .iter()
        .map(|e| {
            let url = e
                .get("url")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("");
            let status = e
                .get("status")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0);
            let method_val = e
                .get("method")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("");
            serde_json::json!({
                "url": url,
                "status": status,
                "method": method_val,
            })
        })
        .collect();

    Ok(json_entries)
}

/// Stream network events in real time.
///
/// Subscribes to `network-event` resources via the WatcherActor (direct mode)
/// or daemon stream protocol (daemon mode), then loops reading events and
/// printing each entry as a JSON line (NDJSON) to stdout.
///
/// Both request arrivals (`resources-available-array`) and response completions
/// (`resources-updated-array`) are emitted.  Each request appears first with
/// `event: "request"`, then again with `event: "response"` once the response
/// arrives.
///
/// Exits cleanly when the connection is closed (e.g. Firefox exits).
pub fn run_follow(cli: &Cli, filter: Option<&str>, method: Option<&str>) -> Result<(), AppError> {
    let mut ctx = connect_and_get_target(cli)?;
    if ctx.via_daemon {
        run_follow_daemon(&mut ctx, filter, method, cli.jq.as_deref())
    } else {
        run_follow_direct(&mut ctx, filter, method, cli.jq.as_deref())
    }
}

fn run_follow_direct(
    ctx: &mut ConnectedTab,
    filter: Option<&str>,
    method: Option<&str>,
    jq_filter: Option<&str>,
) -> Result<(), AppError> {
    let tab_actor = ctx.target_tab_actor().clone();
    let watcher_actor =
        TabActor::get_watcher(ctx.transport_mut(), &tab_actor).map_err(AppError::from)?;

    WatcherActor::watch_resources(ctx.transport_mut(), &watcher_actor, &["network-event"])
        .map_err(AppError::from)?;

    let result = network_follow_loop(ctx.transport_mut(), filter, method, jq_filter);

    // Best-effort cleanup — ignore errors since we may be exiting anyway.
    let _ =
        WatcherActor::unwatch_resources(ctx.transport_mut(), &watcher_actor, &["network-event"]);

    result
}

fn run_follow_daemon(
    ctx: &mut ConnectedTab,
    filter: Option<&str>,
    method: Option<&str>,
    jq_filter: Option<&str>,
) -> Result<(), AppError> {
    use crate::daemon::client::{start_daemon_stream, stop_daemon_stream};

    start_daemon_stream(ctx.transport_mut(), "network-event").map_err(AppError::from)?;

    let result = network_follow_loop(ctx.transport_mut(), filter, method, jq_filter);

    // Best-effort cleanup — ignore errors since we may be exiting anyway.
    let _ = stop_daemon_stream(ctx.transport_mut(), "network-event");

    result
}

/// Emit a single NDJSON line for `entry`, applying `jq_filter` if set.
fn emit_ndjson(entry: &Value, jq_filter: Option<&str>) -> Result<(), AppError> {
    if let Some(filter) = jq_filter {
        let values = output::apply_jq_filter(entry, filter).map_err(AppError::from)?;
        for v in values {
            println!(
                "{}",
                serde_json::to_string(&v).map_err(|e| AppError::Internal(e.into()))?
            );
        }
    } else {
        println!(
            "{}",
            serde_json::to_string(entry).map_err(|e| AppError::Internal(e.into()))?
        );
    }
    Ok(())
}

/// Inner loop for `--follow` mode.
///
/// Maintains a map of in-flight requests keyed by `resource_id`.  When a
/// `resources-available-array` message arrives, each resource is emitted with
/// `event: "request"` (after filter/method checks) and stored in `pending`.
/// When a `resources-updated-array` message arrives, matching entries from
/// `pending` are emitted with `event: "response"`.
fn network_follow_loop(
    transport: &mut RdpTransport,
    filter: Option<&str>,
    method: Option<&str>,
    jq_filter: Option<&str>,
) -> Result<(), AppError> {
    // Track in-flight requests so we can correlate updates with their requests.
    // Only resources that pass the filters are stored here.
    let mut pending: HashMap<u64, NetworkResource> = HashMap::new();

    loop {
        match transport.recv() {
            Ok(msg) => {
                let msg_type = msg.get("type").and_then(Value::as_str).unwrap_or_default();
                match msg_type {
                    // Navigation boundary events forwarded by the daemon.
                    "nav-boundary" | "tabNavigated" => {
                        let url = msg
                            .get("url")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .to_owned();
                        let sequence = msg.get("sequence").and_then(Value::as_u64);
                        let mut nav_entry = json!({
                            "event": "navigation",
                            "url": url,
                        });
                        if let Some(seq) = sequence {
                            nav_entry["sequence"] = json!(seq);
                        }
                        emit_ndjson(&nav_entry, jq_filter)?;
                        let _ = std::io::stdout().flush();
                        // Clear pending on navigation — responses from the
                        // previous page will never arrive for those requests.
                        pending.clear();
                    }
                    "resources-available-array" => {
                        let resources = parse_network_resources(&msg);
                        for res in resources {
                            // Apply filters before emitting or tracking.
                            if let Some(f) = filter
                                && !res.url.contains(f)
                            {
                                continue;
                            }
                            if let Some(m) = method
                                && !res.method.eq_ignore_ascii_case(m)
                            {
                                continue;
                            }
                            let entry = json!({
                                "event": "request",
                                "method": res.method,
                                "url": res.url,
                                "is_xhr": res.is_xhr,
                                "cause_type": res.cause_type,
                                "resource_id": res.resource_id,
                            });
                            emit_ndjson(&entry, jq_filter)?;
                            let _ = std::io::stdout().flush();
                            pending.insert(res.resource_id, res);
                        }
                    }
                    "resources-updated-array" => {
                        let updates = parse_network_resource_updates(&msg);
                        for update in updates {
                            // Only emit updates for requests that passed the filters.
                            // Remove from pending so memory doesn't grow without bound.
                            let Some(res) = pending.remove(&update.resource_id) else {
                                continue;
                            };
                            let mut entry = json!({
                                "event": "response",
                                "method": res.method,
                                "url": res.url,
                                "is_xhr": res.is_xhr,
                                "cause_type": res.cause_type,
                                "resource_id": update.resource_id,
                            });
                            if let Some(ref status) = update.status {
                                if let Ok(code) = status.parse::<u16>() {
                                    entry["status"] = json!(code);
                                } else {
                                    entry["status"] = json!(status);
                                }
                            }
                            if let Some(ref mime) = update.mime_type {
                                entry["content_type"] = json!(mime);
                            }
                            if let Some(total) = update.total_time {
                                entry["duration_ms"] = json!(total);
                            }
                            if let Some(size) = update.content_size {
                                entry["size_bytes"] = json!(size);
                            }
                            if let Some(transferred) = update.transferred_size {
                                entry["transfer_size"] = json!(transferred);
                            }
                            emit_ndjson(&entry, jq_filter)?;
                            let _ = std::io::stdout().flush();
                        }
                    }
                    _ => {}
                }
            }
            Err(ProtocolError::Timeout) => {
                // Normal poll timeout — keep waiting for more events.
            }
            Err(ProtocolError::RecvFailed(ref e))
                if e.kind() == std::io::ErrorKind::UnexpectedEof
                    || e.kind() == std::io::ErrorKind::ConnectionReset
                    || e.kind() == std::io::ErrorKind::BrokenPipe =>
            {
                // Connection closed cleanly (Firefox exited, daemon stopped, etc.).
                return Ok(());
            }
            Err(e) => return Err(AppError::from(e)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn render_network_summary_text_does_not_panic_empty() {
        render_network_summary_text(&json!({
            "total_requests": 0,
            "total_transfer_bytes": 0.0,
            "by_cause_type": {},
            "slowest": [],
            "timeout_reached": false,
        }));
    }

    #[test]
    fn render_network_summary_text_does_not_panic_full() {
        let data = json!({
            "total_requests": 3,
            "total_transfer_bytes": 1600.0,
            "by_cause_type": {"script": 2, "img": 1},
            "slowest": [
                {"url": "https://example.com/big.js", "duration_ms": 200.0, "status": 200, "transfer_size": 1000.0},
            ],
            "timeout_reached": false,
        });
        render_network_summary_text(&data);
    }

    #[test]
    fn build_network_summary_empty() {
        let s = build_network_summary(&[], false);
        assert_eq!(s["total_requests"], 0);
        assert_eq!(s["total_transfer_bytes"], 0.0);
        assert!(s["slowest"].as_array().unwrap().is_empty());
        assert_eq!(s["timeout_reached"], false);
        assert!(s.get("hint").is_none());
    }

    #[test]
    fn build_network_summary_total_transfer_bytes_not_negative_zero() {
        // An empty slice sums to 0.0; IEEE 754 can sometimes produce -0.0.
        // Verify the returned value serialises as "0.0" (positive zero) and
        // that the IEEE bit pattern is positive zero, not negative zero.
        let s = build_network_summary(&[], false);
        let v = s["total_transfer_bytes"]
            .as_f64()
            .expect("total_transfer_bytes is f64");
        assert!(v == 0.0, "expected 0.0, got {v}");
        // f64::is_sign_negative distinguishes -0.0 from +0.0.
        assert!(
            !v.is_sign_negative(),
            "total_transfer_bytes should be positive zero, not negative zero"
        );
        // Serialised form must not contain a minus sign.
        let json_str = serde_json::to_string(&s["total_transfer_bytes"]).unwrap();
        assert!(
            !json_str.starts_with('-'),
            "serialised total_transfer_bytes should not start with '-', got {json_str:?}"
        );
    }

    #[test]
    fn build_network_summary_null_transfer_sizes_give_zero_not_negative_zero() {
        // Entries where transfer_size is null contribute nothing to the sum.
        // The result must be positive 0.0, not -0.0.
        let entries = vec![
            json!({"url": "a", "duration_ms": 10.0, "status": 200, "cause_type": "doc"}),
            json!({"url": "b", "duration_ms": 20.0, "status": 200, "cause_type": "doc"}),
        ];
        let s = build_network_summary(&entries, false);
        let v = s["total_transfer_bytes"]
            .as_f64()
            .expect("total_transfer_bytes is f64");
        assert!(v == 0.0, "expected 0.0, got {v}");
        assert!(!v.is_sign_negative(), "should be +0.0, not -0.0");
    }

    #[test]
    fn build_network_summary_counts_and_bytes() {
        let entries = vec![
            json!({"url": "a", "duration_ms": 100.0, "status": 200, "transfer_size": 500.0, "cause_type": "script"}),
            json!({"url": "b", "duration_ms": 50.0, "status": 404, "transfer_size": 100.0, "cause_type": "script"}),
            json!({"url": "c", "duration_ms": 200.0, "status": 200, "transfer_size": 1000.0, "cause_type": "img"}),
        ];
        let s = build_network_summary(&entries, false);
        assert_eq!(s["total_requests"], 3);
        assert_eq!(s["total_transfer_bytes"], 1600.0);
        assert_eq!(s["by_cause_type"]["script"], 2);
        assert_eq!(s["by_cause_type"]["img"], 1);
        // Slowest first: c (200ms), a (100ms), b (50ms)
        let slowest = s["slowest"].as_array().unwrap();
        assert_eq!(slowest[0]["url"], "c");
        assert_eq!(slowest[1]["url"], "a");
        assert_eq!(slowest[2]["url"], "b");
        assert_eq!(s["timeout_reached"], false);
        assert!(s.get("hint").is_none());
    }

    #[test]
    fn build_network_summary_timeout_reached_adds_hint() {
        let entries =
            vec![json!({"url": "a", "duration_ms": 10.0, "status": 200, "cause_type": "doc"})];
        let s = build_network_summary(&entries, true);
        assert_eq!(s["timeout_reached"], true);
        let hint = s["hint"]
            .as_str()
            .expect("hint should be a string when timeout_reached");
        assert!(
            hint.contains("--network-timeout"),
            "hint should mention --network-timeout"
        );
    }

    #[test]
    fn build_network_summary_no_timeout_no_hint() {
        let entries =
            vec![json!({"url": "a", "duration_ms": 10.0, "status": 200, "cause_type": "doc"})];
        let s = build_network_summary(&entries, false);
        assert_eq!(s["timeout_reached"], false);
        assert!(
            s.get("hint").is_none(),
            "hint should not be present when timeout_reached is false"
        );
    }

    /// AC: `pre_fix_repro_network_text_suppresses_null_cause_type_section`
    ///
    /// When ALL entries have null `cause_type`, the "Requests by Cause Type"
    /// section must be absent from the text output.
    #[test]
    fn pre_fix_repro_network_text_suppresses_null_cause_type_section() {
        // All entries have null cause_type — simulates post-nav incomplete state.
        let entries = vec![
            json!({"url": "a", "duration_ms": 10.0, "status": 200, "transfer_size": 100.0}),
            json!({"url": "b", "duration_ms": 20.0, "status": 200, "transfer_size": 200.0}),
            json!({"url": "c", "duration_ms": 30.0, "status": 304, "transfer_size": 0.0}),
        ];
        let summary = build_network_summary(&entries, false);
        let mut buf: Vec<u8> = Vec::new();
        render_network_summary_text_to(&summary, &mut buf);
        let text = String::from_utf8(buf).expect("output is valid UTF-8");

        assert!(
            !text.contains("Requests by Cause Type"),
            "Section 'Requests by Cause Type' must be suppressed when all cause_type values are null.\n\
             Got output:\n{text}"
        );
        // Total requests should still be reported.
        assert!(
            text.contains("Total requests:"),
            "Header should still appear: {text}"
        );
    }

    /// AC: `unit_network_text_null_keyed_row_renders_unknown`
    ///
    /// When cause_type has a mix of null and non-null keys, the null key
    /// must be displayed as "(unknown)" and the section must be present.
    #[test]
    fn unit_network_text_null_keyed_row_renders_unknown() {
        // Mix: some null, some "script"
        let entries = vec![
            json!({"url": "a", "duration_ms": 10.0, "status": 200, "cause_type": "script"}),
            json!({"url": "b", "duration_ms": 20.0, "status": 200, "cause_type": null}),
            json!({"url": "c", "duration_ms": 30.0, "status": 200}), // cause_type absent
        ];
        let summary = build_network_summary(&entries, false);
        let mut buf: Vec<u8> = Vec::new();
        render_network_summary_text_to(&summary, &mut buf);
        let text = String::from_utf8(buf).expect("output is valid UTF-8");

        assert!(
            text.contains("Requests by Cause Type"),
            "Section must appear when there are non-null keys: {text}"
        );
        assert!(
            text.contains("(unknown)"),
            "Null key must render as '(unknown)': {text}"
        );
        assert!(
            text.contains("script"),
            "Non-null key 'script' must appear: {text}"
        );
    }

    /// Verify that null cause_type entries use "" sentinel (not "other") in the summary JSON.
    #[test]
    fn build_network_summary_null_cause_type_uses_empty_sentinel() {
        let entries = vec![json!({"url": "a", "duration_ms": 10.0, "status": 200})];
        let summary = build_network_summary(&entries, false);
        let by_cause = summary["by_cause_type"].as_object().unwrap();
        // The null cause_type must produce an "" key, not "other".
        assert!(
            by_cause.contains_key(""),
            "null cause_type must use \"\" sentinel; got keys: {:?}",
            by_cause.keys().collect::<Vec<_>>()
        );
        assert!(
            !by_cause.contains_key("other"),
            "null cause_type must NOT produce \"other\" key; got: {by_cause:?}"
        );
    }
}
