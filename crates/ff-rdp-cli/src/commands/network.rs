use std::collections::HashMap;
use std::io::Write;

use ff_rdp_core::{
    NetworkEventActor, NetworkResource, ProtocolError, RdpTransport, TabActor, WatcherActor,
    parse_network_resource_updates, parse_network_resources,
};
use serde_json::{Value, json};

use crate::cli::args::{Cli, NetworkSource};
use crate::error::AppError;
use crate::hints::{HintContext, HintSource};
use crate::output;
use crate::output_controls::{OutputControls, SortDir};
use crate::output_pipeline::OutputPipeline;

use super::connect_tab::{ConnectedTab, connect_and_get_target};
use super::network_events::{
    build_network_entries_with_ids, drain_network_events, merge_updates, performance_api_fallback,
};

/// Hint for an empty one-shot capture: a fresh connection only sees requests
/// made while it is open, so teach the three ways to capture the traffic the
/// caller actually wanted.
pub(crate) const EMPTY_CAPTURE_HINT: &str = "No network events captured: a one-shot `network` only sees requests made while it is connected. \
     Capture a page load with `ff-rdp navigate <url> --with-network`; capture traffic caused by later commands by starting \
     `ff-rdp network --follow > net.ndjson &` before them; or read what already loaded with `ff-rdp network --source performance-api`.";

/// [`EMPTY_CAPTURE_HINT`]'s variant for `--headers`/`--security`: per-request
/// detail lives on `NetworkEventActor`s that die with the connection that
/// observed the request, so after the fact neither `network` nor
/// `--source performance-api` can produce it — only a capture that fetches it
/// before closing can.
pub(crate) const EMPTY_CAPTURE_HEADERS_HINT: &str = "No network events captured: a one-shot `network` only sees requests made while it is connected, \
     and request/response headers and TLS detail can only be read on the connection that observed the request. \
     Capture a page load with its headers in one command: `ff-rdp navigate <url> --with-network --headers` (add `--security` for TLS detail); \
     for a request a click triggers, `ff-rdp click <selector> --wait-for-network <pattern> --headers`.";

/// The empty-capture hint for this invocation: the headers variant when the
/// caller asked for per-request detail no after-the-fact source can supply.
fn empty_capture_hint(wants_per_request_detail: bool) -> &'static str {
    if wants_per_request_detail {
        EMPTY_CAPTURE_HEADERS_HINT
    } else {
        EMPTY_CAPTURE_HINT
    }
}

pub fn run(
    cli: &Cli,
    filter: Option<&str>,
    method: Option<&str>,
    headers: bool,
    security: bool,
    source: NetworkSource,
) -> Result<(), AppError> {
    let mut ctx = connect_and_get_target(cli)?;
    let (all_resources, all_updates) = if source == NetworkSource::PerformanceApi {
        // iter-137 Theme C: `--source performance-api` must not touch the
        // watcher at all — its cost must not depend on a source the user
        // explicitly opted out of.
        (Vec::new(), Vec::new())
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

        // No `unwatchResources` here: unwatching destroys the network-event
        // actors, and `--headers`/`--security` still have to query them
        // below. The subscription ends with this command's connection.
        (result.0, result.1)
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
    // Keep resource IDs in watcher entries so detail+headers mode can fetch them.
    let filtered_watcher = apply_filters(watcher_entries, true);

    // Pick the source.  Exactly one of two, and always the one that was asked
    // for — there is no implicit substitution any more (iter-159 Theme D).
    //
    // An empty watcher capture is reported as zero watcher rows, never swapped
    // for Performance API rows with `method`/`status`/`content_type` null;
    // `--source performance-api` is the explicit opt-in.
    let is_perf_source = source == NetworkSource::PerformanceApi;
    let results = if is_perf_source {
        apply_filters(performance_api_fallback(&mut ctx), false)
    } else {
        // filtered_watcher already has _resource_id present; keep it for the
        // detail+headers path. It will be stripped before final output.
        filtered_watcher
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

    // `meta.source` names where we looked, not what we found: a zero-row
    // watcher result still reports `watcher`, because the alternative — going
    // quiet, or answering from somewhere else — is exactly the behaviour
    // iter-159 removed.
    let mut meta = if is_perf_source {
        json!({"source": "performance-api"})
    } else {
        json!({"source": "watcher"})
    };
    crate::connection_meta::merge_into_if_verbose(
        &mut meta,
        &cli.host,
        cli.port,
        None,
        cli.is_verbose(),
    );

    let use_detail = use_detail_mode(cli, headers, security);

    let empty_capture_hint = empty_capture_hint(headers || security);
    if results.is_empty() {
        // stderr-ok: (b) hint — stdout still carries the (empty) JSON result
        // envelope, whose top-level `hint` says the same thing.
        eprintln!("hint: {empty_capture_hint}");
    }

    let empty_hint = if results.is_empty() && filter.is_none() && method.is_none() {
        Some(json!(empty_capture_hint))
    } else if results.is_empty() {
        Some(json!(
            "No requests matched the current --filter/--method. Remove the filter to see all captured events."
        ))
    } else {
        None
    };

    if use_detail {
        let controls = OutputControls::from_cli(cli, SortDir::Desc);
        // Iteration 126: keep the FULL entry list so the detail envelope can
        // carry the same summary fields (total_requests, total_transfer_bytes,
        // slowest, …) as summary mode. iter-160 Theme F removed `--jq` from
        // `use_detail_mode`, so `--jq` users are no longer forced in here — but
        // this stays, and is now what makes the migration free: a caller who
        // used to reach detail implicitly and now passes `--detail` gets a
        // strict superset of the old envelope, not a trade.
        // build_network_summary only reads url/status/duration_ms/transfer_size/
        // cause_type, so the internal `_resource_id` marker on these entries is
        // harmless here.
        let summary_source = results.clone();
        let mut detail = results;
        // The keys the per-entry joins below attach, so `--fields` may name them.
        let added: Vec<&str> = if is_perf_source {
            if headers || security {
                vec!["note"]
            } else {
                vec![]
            }
        } else {
            [(headers, "headers"), (security, "security")]
                .into_iter()
                .filter_map(|(on, key)| on.then_some(key))
                .collect()
        };
        validate_controls_for_view(&controls, &detail, &added)?;
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
            controls.apply_sort(&mut detail)?;
        }
        let (limited, total, truncated) = controls.apply_limit(detail, Some(20));
        let shown = limited.len();

        // Fetch request+response headers for each entry when --headers is set
        // and the entry came from the watcher (has _resource_id).  The internal
        // `_resource_id` marker is stripped once at the end, after both the
        // header and the security joins have had a chance to use it.
        let mut limited = limited;
        if headers && is_perf_source {
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
        } else if !is_perf_source {
            // iter-128 Theme B: `content_type` is documented (`--help`) as
            // always available on watcher rows, but the `mimeType` field only
            // lands via the `network-event-update:response-content` push —
            // which can arrive after our idle-based drain cutoff on busy
            // pages, leaving `content_type: null` despite a live watcher
            // actor being available. Backfill it from the `Content-Type`
            // response header for any entry where it's still null. When
            // `--headers` is also set, reuse that single response-headers
            // fetch instead of issuing a second RPC per entry.
            for entry in &mut limited {
                let Some(rid) = entry.get("_resource_id").and_then(Value::as_u64) else {
                    continue;
                };
                let Some(actor) = actor_by_resource_id.get(&rid) else {
                    continue;
                };
                let needs_content_type = entry.get("content_type").is_none_or(Value::is_null);

                if headers {
                    let (req_hdrs, resp_hdrs) = fetch_headers(ctx.transport_mut(), actor);
                    if needs_content_type && let Some(ct) = content_type_from_headers(&resp_hdrs) {
                        entry["content_type"] = json!(ct);
                    }
                    entry["headers"] = json!({"request": req_hdrs, "response": resp_hdrs});
                } else if needs_content_type
                    && let Ok(hs) =
                        NetworkEventActor::get_response_headers(ctx.transport_mut(), actor)
                {
                    let resp_hdrs: Vec<Value> = hs
                        .into_iter()
                        .map(|h| json!({"name": h.name, "value": h.value}))
                        .collect();
                    if let Some(ct) = content_type_from_headers(&resp_hdrs) {
                        entry["content_type"] = json!(ct);
                    }
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
                is_perf_source,
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
        // Iteration 126: carry the summary fields alongside `results` in the
        // detail envelope so `--jq` consumers get total_requests etc. Summary
        // counts are computed from the full capture, never the truncated view.
        // `timeout_reached` is always false here: the non-timed drain used by
        // the standalone `network` command stops on idle (see summary mode).
        merge_summary_fields(&mut envelope, &summary_source, false);
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
        return OutputPipeline::from_cli(cli)?.finalize_with_hints(&envelope, Some(&hint_ctx));
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
    OutputPipeline::from_cli(cli)?.finalize_with_hints(&envelope, Some(&hint_ctx))
}

/// Count how many entries are plain-HTTP (insecure) requests.
///
/// The classification is purely by URL scheme: a `http://` URL is insecure,
/// everything else (`https://`, `data:`, `blob:`, `about:`, …) is not counted.
/// This mirrors what a mixed-content audit cares about — HTTP subresources on
/// an HTTPS page — without needing a per-request RPC.
/// Whether `network` returns the entries **array** (detail mode) rather than
/// the summary **object**.
///
/// iter-160 Theme F: `cli.jq.is_some()` used to be in this disjunction, so
/// `ff-rdp network --jq '.results | type'` answered `"array"` while plain
/// `ff-rdp network` produced an object — the filter changed the document it was
/// filtering, on the one command in the tree where that happened (`console`,
/// `a11y`, `perf`, `sources` and `cookies` are single-shape). The global help
/// at `args.rs` already promises "use --jq to filter the envelope", so the doc
/// was right and the code was wrong; changing the doc instead would have
/// ratified the exception.
///
/// `--sort` / `--limit` / `--fields` deliberately stay: they are list-shaped
/// controls whose meaning on a summary object is undefined. Only `--jq`, which
/// is shape-agnostic by construction, comes out. `--detail` is the explicit way
/// in, and iter-126 already made detail mode carry the full summary fields, so
/// callers migrating off the old implicit switch get a strict superset.
fn use_detail_mode(cli: &Cli, headers: bool, security: bool) -> bool {
    cli.detail
        || cli.sort.is_some()
        || cli.limit.is_some()
        || cli.all
        || cli.fields.is_some()
        || headers
        || security
}

pub(crate) fn count_insecure_requests(entries: &[Value]) -> usize {
    entries
        .iter()
        .filter(|e| e["url"].as_str().is_some_and(|u| u.starts_with("http://")))
        .count()
}

/// Extract the `Content-Type` header value (parameters like `charset`
/// stripped) from a fetched `{name, value}` response-header list.
///
/// Matches the header name case-insensitively (HTTP header names are
/// case-insensitive per RFC 7230). Returns `None` when no `Content-Type`
/// header is present in the list.
fn content_type_from_headers(headers: &[Value]) -> Option<String> {
    headers.iter().find_map(|h| {
        let name = h.get("name")?.as_str()?;
        if !name.eq_ignore_ascii_case("content-type") {
            return None;
        }
        let value = h.get("value")?.as_str()?;
        let bare = value.split(';').next().unwrap_or(value).trim();
        if bare.is_empty() {
            None
        } else {
            Some(bare.to_owned())
        }
    })
}

/// Validate `--sort`/`--fields` against the entries as the user will see them.
///
/// Watcher entries carry the internal `_resource_id` join key until the
/// per-request joins are done, and `--headers`/`--security` add their keys only
/// after the cap. Validating the raw entries would accept `--fields
/// _resource_id` (then strip it, printing `{}` rows), list `_resource_id` as an
/// available name, and reject `--fields headers`. So: drop the join key, and
/// for `--fields` count `added` (the keys the joins will attach) as available.
pub(crate) fn validate_controls_for_view(
    controls: &OutputControls,
    entries: &[Value],
    added: &[&str],
) -> Result<(), AppError> {
    if controls.sort_field.is_none() && controls.fields.is_none() {
        return Ok(());
    }
    let mut view: Vec<Value> = entries
        .iter()
        .map(|e| {
            let mut e = e.clone();
            if let Some(obj) = e.as_object_mut() {
                obj.remove("_resource_id");
            }
            e
        })
        .collect();
    controls.validate_sort(&view)?;
    if let Some(obj) = view.first_mut().and_then(Value::as_object_mut) {
        for key in added {
            obj.insert((*key).to_owned(), Value::Null);
        }
    }
    controls.validate_fields(&view)
}

/// Fetch one request's headers from its `NetworkEventActor` as the
/// `(request, response)` pair of `{name, value}` lists `--headers` emits. A
/// failed fetch yields an empty list for that side, so the entry's `headers`
/// object never changes shape with the outcome.
///
/// Must run on the connection that observed the request, before it unwatches
/// `network-event`: the actor dies with either.
fn fetch_headers(
    transport: &mut RdpTransport,
    actor: &ff_rdp_core::ActorId,
) -> (Vec<Value>, Vec<Value>) {
    fn to_json(hs: Vec<ff_rdp_core::Header>) -> Vec<Value> {
        hs.into_iter()
            .map(|h| json!({"name": h.name, "value": h.value}))
            .collect()
    }
    let req = NetworkEventActor::get_request_headers(transport, actor)
        .map(to_json)
        .unwrap_or_default();
    let resp = NetworkEventActor::get_response_headers(transport, actor)
        .map(to_json)
        .unwrap_or_default();
    (req, resp)
}

/// Attach `headers: {request, response}` — the `network --headers` shape — to
/// every entry whose internal `_resource_id` maps to a live
/// `NetworkEventActor`, backfilling a null `content_type` from the response's
/// `Content-Type` on the way.
///
/// `navigate --with-network --headers` and `click --wait-for-network
/// --headers` call this before their connection unwatches: the actors die with
/// the subscription, which is why a later `network --headers` sees nothing.
pub(crate) fn attach_headers(
    entries: &mut [Value],
    transport: &mut RdpTransport,
    actor_by_resource_id: &HashMap<u64, ff_rdp_core::ActorId>,
) {
    for entry in entries.iter_mut() {
        let Some(actor) = entry
            .get("_resource_id")
            .and_then(Value::as_u64)
            .and_then(|rid| actor_by_resource_id.get(&rid))
        else {
            continue;
        };
        attach_headers_to(entry, transport, actor);
    }
}

/// [`attach_headers`] for one entry whose `NetworkEventActor` is already known
/// (`click --wait-for-network --headers`).
pub(crate) fn attach_headers_to(
    entry: &mut Value,
    transport: &mut RdpTransport,
    actor: &ff_rdp_core::ActorId,
) {
    let (req, resp) = fetch_headers(transport, actor);
    if entry.get("content_type").is_none_or(Value::is_null)
        && let Some(ct) = content_type_from_headers(&resp)
    {
        entry["content_type"] = json!(ct);
    }
    entry["headers"] = json!({"request": req, "response": resp});
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
pub(crate) fn attach_security(
    limited: &mut [Value],
    ctx: &mut ConnectedTab,
    actor_by_resource_id: &HashMap<u64, ff_rdp_core::ActorId>,
    is_perf_source: bool,
) {
    const SECURITY_NOTE: &str = "--security ignored (performance-api source has no \
        per-request security info; use --with-network to engage the watcher)";

    for entry in limited.iter_mut() {
        if is_perf_source {
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
        // iter-128 Theme C: middle-ellipsize the url so a single ~900-char
        // tracking/CMP URL can't blow this line out to thousands of columns
        // wide (dogfood-62 #2). The 80-char cap plus the fixed "  N. " /
        // "  (…ms, …, …b)" prefix+suffix keeps the whole line comfortably
        // under 120 columns.
        for (i, entry) in slowest.iter().enumerate() {
            let raw_url = entry.get("url").and_then(Value::as_str).unwrap_or("?");
            let url = crate::output::middle_ellipsis(raw_url, 80);
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
        // iter-141 Theme F: text mode must not silently show "the 20
        // slowest" as if it were "every request" — say so explicitly when
        // `slowest_truncated` is set.
        if summary
            .get("slowest_truncated")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            let total = summary
                .get("total_requests")
                .and_then(Value::as_u64)
                .unwrap_or(0);
            let _ = writeln!(
                out,
                "  (showing {} of {total} requests — use --all for the complete list)",
                slowest.len()
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
/// - `slowest_truncated`: `true` when `total_requests` exceeds `slowest.len()`
///   — i.e. `slowest` is a top-20 sample, not the full request list (iter-141
///   Theme F). Previously `slowest` silently capped at 20 with no marker
///   distinguishing "these are all N requests" from "these are the 20
///   slowest of N"; `--all`/`--detail` switch to the entry-level `truncated`
///   flag on the full list, but nothing said the summary's own `slowest`
///   was *also* incomplete.
/// - `timeout_reached`: whether the collection deadline fired while events were still arriving
/// - `hint`: an always-present, nullable member (iter-128 Theme A) — advice to
///   increase `--network-timeout` when `timeout_reached` is true, `null`
///   otherwise. Previously this key was omitted entirely unless
///   `timeout_reached`, so a quiet capture and a timed-out one had different
///   key sets and `.hint` threw on the quiet path under `--jq`.
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
    // iter-141 Theme F: explicit marker for the silent 20-cap on `slowest`.
    let slowest_truncated = total_requests > slowest.len();

    // iter-128 Theme A: `hint` is always present — `null` when there is
    // nothing to hint — so the key set never varies with capture content.
    let hint = if timeout_reached {
        json!(
            "Network collection was still receiving events when the timeout was reached. \
             Consider increasing --network-timeout for more complete results."
        )
    } else {
        Value::Null
    };

    json!({
        "total_requests": total_requests,
        "total_transfer_bytes": total_transfer_bytes,
        "by_cause_type": by_cause_type,
        "slowest": slowest,
        "slowest_truncated": slowest_truncated,
        "timeout_reached": timeout_reached,
        "hint": hint,
    })
}

/// Merge the summary fields from [`build_network_summary`] into a target object.
///
/// Copies `total_requests`, `total_transfer_bytes`, `by_cause_type`, `slowest`
/// and `timeout_reached` (plus the `hint` field when `timeout_reached` is true)
/// onto `target` without disturbing keys `target` already holds. Existing keys
/// on `target` win, so entry-level fields such as `entries`/`shown`/`total` are
/// never clobbered by the summary.
///
/// `entries` is the FULL, unlimited entry list — summary counts (e.g.
/// `total_requests`) always reflect the whole capture, not the truncated view.
pub(crate) fn merge_summary_fields(
    target: &mut serde_json::Value,
    entries: &[serde_json::Value],
    timeout_reached: bool,
) {
    let summary = build_network_summary(entries, timeout_reached);
    if let (Some(dst), Some(src)) = (target.as_object_mut(), summary.as_object()) {
        for (k, v) in src {
            // Do not overwrite entry-level keys the caller already set.
            dst.entry(k.clone()).or_insert_with(|| v.clone());
        }
    }
}

/// Build the ONE canonical network object shape returned on every path.
///
/// Iteration 126: `navigate --with-network` (and the standalone `network`
/// detail view) previously flipped between a bare array (quiet/`--all` pages),
/// a `{entries, shown, total, truncated, hint}` object (busy pages), and a
/// summary object (non-detail) — so `.results.network.entries` threw
/// `cannot index array` half the time and the documented summary fields were
/// unreachable via `--jq`. This builder returns a single object on every path:
///
/// ```json
/// {
///   "entries": [ ... ],          // limited + field-projected view
///   "shown": N,                  // entries.len()
///   "total": N,                  // total BEFORE the default 20-entry limit
///   "truncated": bool,           // total > shown
///   "total_requests": N,         // summary fields (from the FULL capture)
///   "total_transfer_bytes": N,
///   "by_cause_type": { ... },
///   "slowest": [ ... ],
///   "slowest_truncated": bool,   // true when total_requests > slowest.len()
///   "timeout_reached": bool,
///   "hint": null | "..."         // iter-128 Theme A: always present; null
///                                 // unless truncated or timeout_reached
/// }
/// ```
///
/// The summary counts are computed from `all_entries` (the full capture) while
/// `entries` carries the possibly-truncated, field-projected view, so
/// `--fields url` can never strip `total_requests` and `--limit`/the default cap
/// never distorts the totals.
pub(crate) fn build_canonical_network(
    entries: Vec<serde_json::Value>,
    shown: usize,
    total: usize,
    truncated: bool,
    all_entries: &[serde_json::Value],
    timeout_reached: bool,
) -> serde_json::Value {
    // Move `entries` into the object explicitly via Value::Array so the Vec is
    // consumed rather than borrowed (avoids a needless clone).
    let mut obj = json!({
        "entries": Value::Array(entries),
        "shown": shown,
        "total": total,
        "truncated": truncated,
    });
    // merge_summary_fields always inserts `hint` (null unless timeout_reached
    // — see build_network_summary, iter-128 Theme A) since `obj` doesn't
    // already carry the key.
    merge_summary_fields(&mut obj, all_entries, timeout_reached);
    // The summary's own timeout hint wins if it already claimed the key
    // (non-null); otherwise overwrite the null placeholder with the
    // truncation hint. `hint` is always present after this point, so use
    // `insert` (unconditional) gated on the hint currently being null,
    // rather than `entry().or_insert()` which would never fire now that the
    // key always exists.
    if truncated && let Some(map) = obj.as_object_mut() {
        let hint_is_null = map.get("hint").is_none_or(Value::is_null);
        if hint_is_null {
            map.insert(
                "hint".to_string(),
                json!(format!(
                    "showing {shown} of {total}, use --all for complete list"
                )),
            );
        }
    }
    obj
}

/// Direct-mode default drain window, in ms, when the caller passes no timeout.
/// Public so the runner can report the window it actually got without
/// hard-coding a second copy that could drift (iter-179).
pub const DEFAULT_DRAIN_MS: u64 = 500;

/// Drain network events as a JSON array (iter-179).
///
/// Used by the script runner's `assert_network` step. `drain_timeout_ms`
/// controls how long to drain (default [`DEFAULT_DRAIN_MS`]).
///
/// # The subscription window
///
/// There is no standing subscription. This function arms the
/// watcher itself, drains for `drain_timeout_ms`, and unwatches. Firefox's
/// `watchResources` delivers events that occur *while watching*; it does not
/// replay history. **A request that completed before this call was made is
/// therefore invisible, and the drain returns zero events rather than a partial
/// buffer.**
///
/// That is the whole explanation for iteration 179's
/// `assert_network … diagnostics.events_in_buffer: 0`: the playbook's `click`
/// step fired a single POST, the following `assert_network` step opened a fresh
/// connection and armed the watcher, and on a loaded machine that arming lost
/// the race with the response. With exactly one request in flight, losing the
/// race produces **zero**, never a partial count — which is why the zero looked
/// like a broken subscription and was not one.
///
/// Iteration 181 removed that race from the script runner's default path by
/// arming one subscription for the whole playbook
/// ([`crate::commands::network_watch::PlaybookNetworkWatch`]). Everything above
/// still describes **this** function, which the runner now reaches only when
/// that arming failed — and its `assert_network` diagnostics say so, with
/// `subscription: "step"`.
pub fn run_get_events(
    cli: &Cli,
    drain_timeout_ms: Option<u64>,
) -> Result<Vec<serde_json::Value>, crate::error::AppError> {
    use super::network_events::{build_network_entries, merge_updates};
    use ff_rdp_core::{TabActor, WatcherActor};
    use std::time::Duration;

    let mut ctx = super::connect_tab::connect_and_get_target(cli)?;

    let entries = {
        // Subscribe, drain briefly, unsubscribe.
        let drain_ms = drain_timeout_ms.unwrap_or(DEFAULT_DRAIN_MS);
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
/// Subscribes to `network-event` resources via the WatcherActor, then loops
/// reading events and
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
    run_follow_on(&mut ctx, filter, method, cli.jq.as_deref())
}

fn run_follow_on(
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
                    // Top-level navigation of the watched tab.
                    "tabNavigated" => {
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
                // Connection closed cleanly (Firefox exited, etc.).
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

    // ── iter-160 Theme F: --jq is a view, not a shape switch ───────────────

    fn cli_with(args: &[&str]) -> Cli {
        use clap::Parser as _;
        let mut full = vec!["ff-rdp", "network"];
        full.extend_from_slice(args);
        Cli::try_parse_from(full).expect("args must parse")
    }

    /// AC `unit_160_use_detail_excludes_jq`.
    #[test]
    fn unit_160_use_detail_excludes_jq() {
        // `--jq` alone must leave the shape alone: a filter that changes the
        // document it filters makes every jq expression conditional on which
        // command it is aimed at.
        assert!(
            !use_detail_mode(&cli_with(&["--jq", ".results"]), false, false),
            "--jq must not force detail mode"
        );
        // Plain `network` is unchanged too.
        assert!(!use_detail_mode(&cli_with(&[]), false, false));

        // Every explicit way in still works.
        for args in [
            vec!["--detail"],
            vec!["--all"],
            vec!["--sort", "duration_ms"],
            vec!["--limit", "5"],
            vec!["--fields", "url"],
        ] {
            assert!(
                use_detail_mode(&cli_with(&args), false, false),
                "{args:?} must still reach detail mode"
            );
        }
        // --headers / --security are command-local flags, not global Cli ones.
        assert!(use_detail_mode(&cli_with(&[]), true, false), "--headers");
        assert!(use_detail_mode(&cli_with(&[]), false, true), "--security");
    }

    /// `--detail --jq` is the migration path and must still be detail.
    #[test]
    fn unit_160_detail_plus_jq_is_still_detail() {
        assert!(use_detail_mode(
            &cli_with(&["--detail", "--jq", ".results"]),
            false,
            false
        ));
    }

    #[test]
    fn content_type_from_headers_finds_case_insensitively_and_strips_params() {
        let headers = vec![
            json!({"name": "Content-Length", "value": "1234"}),
            json!({"name": "content-type", "value": "text/html; charset=utf-8"}),
        ];
        assert_eq!(
            content_type_from_headers(&headers).as_deref(),
            Some("text/html")
        );
    }

    #[test]
    fn content_type_from_headers_no_match_returns_none() {
        let headers = vec![json!({"name": "Content-Length", "value": "1234"})];
        assert_eq!(content_type_from_headers(&headers), None);
    }

    #[test]
    fn content_type_from_headers_bare_value_untouched() {
        let headers = vec![json!({"name": "Content-Type", "value": "application/json"})];
        assert_eq!(
            content_type_from_headers(&headers).as_deref(),
            Some("application/json")
        );
    }

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
        // iter-128 Theme A: `hint` is always present (null when nothing to
        // hint), never omitted — the key set must not vary with content.
        assert_eq!(s["hint"], Value::Null, "hint must be null, not absent");
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
        // iter-128 Theme A: hint is always present, null when not timed out.
        assert_eq!(s["hint"], Value::Null, "hint must be null, not absent");
        // iter-141 Theme F: 3 requests, all shown in `slowest` — not truncated.
        assert_eq!(s["slowest_truncated"], false);
    }

    // ── iter-141 Theme F: `slowest_truncated` ────────────────────────────

    /// AC `e2e_network_truncation_flag`: more than 20 requests means
    /// `slowest` only carries the top 20 — `slowest_truncated` must say so
    /// explicitly rather than leaving a caller to infer it by comparing
    /// `total_requests` to `slowest.len()` themselves.
    #[test]
    fn build_network_summary_slowest_truncated_when_over_20_requests() {
        let entries: Vec<Value> = (0..25)
            .map(|i| {
                json!({"url": format!("https://example.com/{i}"), "duration_ms": f64::from(i), "status": 200, "cause_type": "script"})
            })
            .collect();
        let s = build_network_summary(&entries, false);
        assert_eq!(s["total_requests"], 25);
        assert_eq!(s["slowest"].as_array().unwrap().len(), 20);
        assert_eq!(
            s["slowest_truncated"], true,
            "25 requests > 20-slot `slowest` must be flagged truncated"
        );
    }

    /// Exactly 20 requests: `slowest` carries all of them — not truncated.
    #[test]
    fn build_network_summary_slowest_not_truncated_at_exactly_20() {
        let entries: Vec<Value> = (0..20)
            .map(|i| {
                json!({"url": format!("https://example.com/{i}"), "duration_ms": f64::from(i), "status": 200, "cause_type": "script"})
            })
            .collect();
        let s = build_network_summary(&entries, false);
        assert_eq!(s["slowest"].as_array().unwrap().len(), 20);
        assert_eq!(s["slowest_truncated"], false);
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
        // iter-128 Theme A: hint is present-but-null, not absent, when
        // timeout_reached is false — flipped from the pre-128 absence
        // assertion this test name still describes ("no hint").
        assert_eq!(
            s["hint"],
            Value::Null,
            "hint should be null (present, not absent) when timeout_reached is false"
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

    // -----------------------------------------------------------------------
    // iter-126: canonical network object shape
    // -----------------------------------------------------------------------

    fn sample_entries(n: usize) -> Vec<Value> {
        (0..n)
            .map(|i| {
                // Test indices are tiny; cast losslessly via u16 for a stable
                // increasing duration without triggering cast_precision_loss.
                let duration_ms = f64::from(u16::try_from(i).unwrap_or(u16::MAX)) * 10.0;
                json!({
                    "url": format!("https://example.com/{i}"),
                    "duration_ms": duration_ms,
                    "status": 200,
                    "transfer_size": 100.0,
                    "cause_type": "script",
                })
            })
            .collect()
    }

    /// AC: `unit_canonical_network_hint_null_when_quiet` — `hint` is JSON
    /// `null` (present, never omitted) when `!truncated && !timeout_reached`
    /// and the capture is non-empty, on BOTH the navigate builder
    /// (`build_canonical_network`) and the standalone `network` detail
    /// envelope's builder (`merge_summary_fields`, which
    /// `build_canonical_network` itself wraps).
    #[test]
    fn unit_canonical_network_hint_null_when_quiet() {
        // navigate builder: quiet page, nothing truncated, no timeout.
        let entries = sample_entries(3);
        let obj = build_canonical_network(entries.clone(), 3, 3, false, &entries, false);
        assert_eq!(
            obj["hint"],
            Value::Null,
            "navigate builder: hint must be null (present, not absent) when quiet"
        );

        // standalone `network` detail envelope builder: same merge path,
        // applied directly to a hand-built envelope the way run()'s detail
        // branch does.
        let mut env = json!({"results": entries.clone(), "total": entries.len()});
        merge_summary_fields(&mut env, &entries, false);
        assert_eq!(
            env["hint"],
            Value::Null,
            "network detail envelope builder: hint must be null (present, not absent) when quiet"
        );
    }

    #[test]
    fn build_canonical_network_carries_entries_and_summary() {
        let entries = sample_entries(3);
        let obj = build_canonical_network(entries.clone(), 3, 3, false, &entries, false);
        assert!(obj.is_object(), "canonical shape must be an object");
        // Entry-level keys.
        assert!(obj["entries"].is_array());
        assert_eq!(obj["entries"].as_array().unwrap().len(), 3);
        assert_eq!(obj["shown"], 3);
        assert_eq!(obj["total"], 3);
        assert_eq!(obj["truncated"], false);
        // Summary keys ride alongside.
        assert_eq!(obj["total_requests"], 3);
        assert_eq!(obj["total_transfer_bytes"], 300.0);
        assert!(obj["by_cause_type"].is_object());
        assert!(obj["slowest"].is_array());
        assert_eq!(obj["timeout_reached"], false);
    }

    #[test]
    fn build_canonical_network_empty_keeps_all_keys() {
        // A zero-request page still carries entries:[] and total_requests:0 —
        // keys present, not omitted (plan Task A, third bullet).
        let obj = build_canonical_network(vec![], 0, 0, false, &[], false);
        assert!(obj.is_object());
        assert!(obj["entries"].is_array());
        assert_eq!(obj["entries"].as_array().unwrap().len(), 0);
        assert_eq!(obj["shown"], 0);
        assert_eq!(obj["total"], 0);
        assert_eq!(obj["truncated"], false);
        assert_eq!(obj["total_requests"], 0);
        assert_eq!(obj["total_transfer_bytes"], 0.0);
    }

    #[test]
    fn build_canonical_network_truncated_summary_reflects_full_capture() {
        // The `entries` view is truncated to 2, but summary counts must reflect
        // the FULL 5-entry capture, and a truncation hint is added.
        let all = sample_entries(5);
        let limited: Vec<Value> = all.iter().take(2).cloned().collect();
        let obj = build_canonical_network(limited, 2, 5, true, &all, false);
        assert_eq!(obj["shown"], 2);
        assert_eq!(obj["total"], 5);
        assert_eq!(obj["truncated"], true);
        // total_requests reflects the full capture, never the truncated view.
        assert_eq!(obj["total_requests"], 5);
        assert_eq!(obj["total_transfer_bytes"], 500.0);
        let hint = obj["hint"].as_str().expect("truncation hint present");
        assert!(hint.contains("--all"), "hint should mention --all: {hint}");
    }

    #[test]
    fn build_canonical_network_timeout_hint_wins_over_truncation() {
        // When both timeout and truncation could add a hint, the timeout hint
        // (from build_network_summary) is set first and must not be overwritten.
        let all = sample_entries(5);
        let limited: Vec<Value> = all.iter().take(2).cloned().collect();
        let obj = build_canonical_network(limited, 2, 5, true, &all, true);
        assert_eq!(obj["timeout_reached"], true);
        let hint = obj["hint"].as_str().expect("hint present");
        assert!(
            hint.contains("--network-timeout"),
            "timeout hint must win over truncation hint: {hint}"
        );
    }

    #[test]
    fn merge_summary_fields_does_not_clobber_entry_keys() {
        let all = sample_entries(2);
        let mut target = json!({
            "entries": [{"url": "kept"}],
            "shown": 1,
            "total": 2,
            "truncated": true,
        });
        merge_summary_fields(&mut target, &all, false);
        // Entry keys survive; summary keys are added.
        assert_eq!(target["entries"][0]["url"], "kept");
        assert_eq!(target["shown"], 1);
        assert_eq!(target["total"], 2);
        assert_eq!(target["truncated"], true);
        assert_eq!(target["total_requests"], 2);
        assert!(target["slowest"].is_array());
    }

    #[test]
    fn network_and_navigate_summary_fields_agree_field_for_field() {
        // Parity assertion (iter-125 precedent): the summary fields carried by
        // the standalone `network` detail envelope and by the `navigate
        // --with-network` canonical object must be byte-identical for the same
        // capture. Both go through merge_summary_fields / build_network_summary,
        // so extract each side's summary key set and compare.
        let entries = sample_entries(4);

        // navigate side: the canonical object embeds summary fields directly.
        let nav = build_canonical_network(entries.clone(), 4, 4, false, &entries, false);

        // network side: summary fields are merged onto the envelope.
        let mut net_env = json!({ "results": [], "total": 4 });
        merge_summary_fields(&mut net_env, &entries, false);

        for key in [
            "total_requests",
            "total_transfer_bytes",
            "by_cause_type",
            "slowest",
            "timeout_reached",
        ] {
            assert_eq!(
                nav[key], net_env[key],
                "summary field `{key}` must agree between navigate and network shapes"
            );
        }
    }
}
