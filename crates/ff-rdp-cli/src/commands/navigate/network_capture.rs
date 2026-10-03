use std::collections::HashMap;
use std::time::{Duration, Instant};

use ff_rdp_core::{ActorId, TabActor, WatcherActor};
use serde_json::json;

use crate::cli::args::Cli;
use crate::commands::connect_tab::connect_and_get_target;
use crate::commands::network::{attach_headers, attach_security};
use crate::commands::network_events::{
    build_network_entries_with_ids, drain_network_events_timed, merge_updates,
};
use crate::commands::url_validation::validate_content_navigation_url;
use crate::error::AppError;
use crate::hints::{HintContext, HintSource};
use crate::output;
use crate::output_controls::{OutputControls, SortDir};
use crate::output_pipeline::OutputPipeline;

use super::consent::detect_and_accept_on;
use super::readiness::{
    CommitInfo, check_real_tab_url_for_neterror, eval_document_ready_state, eval_location_href,
    refresh_console_actor, run_wait_for_predicates,
};
use super::status::{StatusUnknown, extract_document_status};
use super::{WaitAfterNav, restore_timeout, wait_after_navigate};

/// How long `--with-network --auto-consent` keeps draining after the consent
/// click, so requests unblocked by the dismissal are part of the same capture.
///
/// Short by design: [`drain_network_events_timed`] returns as soon as the
/// stream goes quiet, so this is a ceiling for a page that keeps loading, not a
/// fixed wait.
pub(crate) const CONSENT_POST_DRAIN_MS: u64 = 8_000;

/// Navigate to `url` and capture all network requests made during navigation.
///
/// The flow on a single TCP connection is:
/// 1. Connect and resolve the target tab.
/// 2. Get the WatcherActor via `TabActor::get_watcher`.
/// 3. Subscribe to `"network-event"` resources via `WatcherActor::watch_resources`.
/// 4. Navigate with `WindowGlobalTarget::navigate_to`.
/// 5. Drain `resources-available-array` / `resources-updated-array` events
///    (timeout-bounded, same pattern as the `network` command).
/// 6. Merge updates into resources by `resource_id`, apply the output
///    controls, and — with `--headers`/`--security` — fetch per-request detail
///    for the shown entries while their NetworkEventActors are still alive.
/// 7. Unwatch resources to clean up server-side state.
/// 8. Optionally wait for a condition (--wait-text / --wait-selector).
/// 9. Emit combined JSON output.
///
/// iter-159: `auto_consent` is honoured here too.  `--with-network` and
/// `--auto-consent` used to be mutually exclusive at the clap level, so on any
/// consent-walled site — the exact case where you want both — you had to choose
/// between dismissing the banner and capturing the network. The consent step now
/// runs while capture is still in effect, on the **same** connection (see
/// [`detect_and_accept_on`]), and a short follow-up drain after the click
/// collects the requests the dismissal unblocks.
#[allow(clippy::too_many_arguments)]
pub(crate) fn run_with_network(
    cli: &Cli,
    url: &str,
    wait_opts: &WaitAfterNav<'_>,
    network_timeout_ms: u64,
    detail: NetworkDetail,
    auto_consent: bool,
    conditions: &crate::cli::args::NetworkConditionsArgs,
    page_args: &crate::cli::args::PageViewArgs,
) -> Result<(), AppError> {
    validate_content_navigation_url(url, cli.allow_file_urls, cli.allow_unsafe_urls)?;
    let mut ctx = connect_and_get_target(cli)?;
    let target_actor = ctx.target().actor.clone();

    let tab_actor = ctx.target_tab_actor().clone();

    // Get watcher actor for resource subscriptions.
    let watcher_actor =
        TabActor::get_watcher(ctx.transport_mut(), &tab_actor).map_err(AppError::from)?;

    // Engage the watcher's frame-target stream before subscribing to resources.
    // Per the Firefox WatcherActor contract (kb/rdp/actors/watcher.md), the
    // server delivers nothing until BOTH `watchTargets("frame")` and
    // `watchResources([...])` have been issued — without this the
    // `network-event` stream stays empty on `navigate --with-network`.
    WatcherActor::watch_targets(ctx.transport_mut(), &watcher_actor, "frame")
        .map_err(AppError::from)?;

    // Subscribe to network events before navigating so we capture everything.
    WatcherActor::watch_resources(ctx.transport_mut(), &watcher_actor, &["network-event"])
        .map_err(AppError::from)?;

    // `--throttle`/`--block`: set on this watcher before `navigateTo`.
    let conditions_applied =
        crate::commands::network_conditions::apply(&mut ctx, &watcher_actor, conditions)?;

    // iter-138 Theme G: wall-clock start, so the envelope's `elapsed_ms`
    // matches what plain `navigate` reports rather than being absent.
    let nav_start = Instant::now();

    // Send the navigateTo request without reading its response.  The normal
    // `WindowGlobalTarget::navigate_to` uses `actor_request` which loops
    // reading messages until it finds one from the target actor — silently
    // discarding any `resources-available-array` events from the watcher that
    // arrive in between.  By sending raw, we let `drain_network_events`
    // collect those events (it skips non-network message types harmlessly).
    ctx.transport_mut()
        .send(&json!({
            "to": target_actor.as_ref(),
            "type": "navigateTo",
            "url": url,
        }))
        .map_err(AppError::from)?;

    // Drain resource events for the total_timeout wall-clock duration,
    // using short 500ms poll intervals internally.  This captures events
    // that arrive in bursts with gaps — the navigateTo ack is harmlessly
    // skipped by the drain since it is not a network resource message type.
    let drain_result = drain_network_events_timed(
        ctx.transport_mut(),
        Duration::from_millis(network_timeout_ms),
    );

    // Restore original timeout before any further RDP round-trips (unwatch).
    restore_timeout(ctx.transport_mut(), cli.timeout);

    let (mut all_resources, mut all_updates, mut timeout_reached) =
        drain_result.map_err(AppError::from)?;

    // iter-159: dismiss the consent overlay while the resource subscription is
    // still live, then drain again briefly so the requests the click triggers
    // land in this invocation's capture.
    let consent = if auto_consent {
        let c = detect_and_accept_on(&mut ctx);
        let post = drain_network_events_timed(
            ctx.transport_mut(),
            Duration::from_millis(CONSENT_POST_DRAIN_MS),
        );
        restore_timeout(ctx.transport_mut(), cli.timeout);
        match post {
            Ok((r, u, t)) => {
                all_resources.extend(r);
                all_updates.extend(u);
                timeout_reached = timeout_reached || t;
            }
            Err(e) => {
                eprintln!("warning: --auto-consent: post-consent network drain failed: {e}"); // stderr-ok: (b) warn-and-continue — the primary capture already succeeded
            }
        }
        let mut seen = std::collections::HashSet::new();
        all_resources.retain(|r| seen.insert(r.resource_id));
        Some(c)
    } else {
        None
    };

    // iter-138 Theme G/A: capture the main document's status candidates before
    // `merge_updates` consumes `all_updates` by value below. Resolution waits
    // until the committed URL has been evaluated (iter-166).
    let doc_tracker = extract_document_status(&all_resources, &all_updates);

    // Merge updates into resources by resource_id.
    let update_map = merge_updates(all_updates);

    // Build the network entries array (no URL/method filtering here). Entries
    // carry an internal `_resource_id` until `apply_network_controls` strips
    // it, so `--headers`/`--security` can find each request's actor.
    let network_entries = build_network_entries_with_ids(&all_resources, &update_map);

    // `--headers`/`--security` read per-request detail from the
    // NetworkEventActors, which die when this connection unwatches
    // `network-event` (below) or closes — a later `network --headers` has
    // nothing to ask. So the output controls run here, while the actors are
    // alive, and the detail is fetched for the entries that will be shown.
    let network_entries = {
        let actor_by_resource_id: HashMap<u64, ActorId> = all_resources
            .iter()
            .map(|r| (r.resource_id, r.actor.clone()))
            .collect();
        apply_network_controls(
            cli,
            &network_entries,
            timeout_reached,
            detail,
            |shown: &mut [serde_json::Value]| {
                if detail.headers {
                    attach_headers(shown, ctx.transport_mut(), &actor_by_resource_id);
                }
                if detail.security {
                    attach_security(shown, &mut ctx, &actor_by_resource_id, false);
                }
            },
        )?
    };

    // Unwatch to clean up server-side resources — unless `--throttle`/`--block`
    // are in force: unwatching `network-event` would tear them down before the
    // wait conditions and `--with-page` below. The connection's end cleans up.
    if conditions_applied.is_none() {
        let _ = WatcherActor::unwatch_resources(
            ctx.transport_mut(),
            &watcher_actor,
            &["network-event"],
        );
    }

    // Pair the `watchTargets("frame")` prelude with `unwatchTargets` so the
    // server-side frame-target subscription is cleared (oneway, best-effort).
    let _ = WatcherActor::unwatch_targets(ctx.transport_mut(), &watcher_actor, Some("frame"), None);

    // NOTE: wait_after_navigate is called *after* draining network events
    // and unwatching resources, so network data is already fully collected
    // before we begin waiting.

    // iter-138 Theme G: the network drain already waited for events to
    // settle, so a direct eval here is exactly as truthful as plain
    // `navigate`'s post-commit reads — no separate commit-wait is needed,
    // but `committed_url`/`ready_state`/`status` are no longer dropped: the
    // envelope reports the same navigation fields plain `navigate` does.
    // Neterror detection still runs via listTabs below.
    //
    // Theme K: refresh consoleActor before evaluating —
    // `ctx.target().console_actor` is still bound to the pre-navigation docshell here,
    // and evaluating against it would fail with `noSuchActor` on any real
    // cross-document navigation.
    refresh_console_actor(&mut ctx);

    let commit_info: Option<CommitInfo> = {
        let console_actor = ctx.target().console_actor.clone();
        let committed_url = eval_location_href(ctx.transport_mut(), &console_actor);
        let ready_state = eval_document_ready_state(ctx.transport_mut(), &console_actor);
        let elapsed_ms = u64::try_from(nav_start.elapsed().as_millis()).unwrap_or(u64::MAX);
        let (http_status, status_reason) = doc_tracker.resolve(url, &committed_url);
        Some(CommitInfo {
            committed_url,
            ready_state,
            elapsed_ms,
            http_status,
            status_reason,
        })
    };

    // Detect about:neterror in the --with-network path.
    if let Some(err) = check_real_tab_url_for_neterror(&mut ctx, url) {
        return Err(err);
    }

    let wait_result = wait_after_navigate(&mut ctx, wait_opts)?;
    let wait_for_result = run_wait_for_predicates(&mut ctx, wait_opts)?;

    let mut result = json!({
        "navigated": url,
        "network": network_entries,
    });
    if let Some(ref ci) = commit_info
        && let Some(obj) = result.as_object_mut()
    {
        obj.insert("committed_url".to_string(), json!(ci.committed_url));
        obj.insert("ready_state".to_string(), json!(ci.ready_state));
        obj.insert("elapsed_ms".to_string(), json!(ci.elapsed_ms));
        obj.insert("status".to_string(), json!(ci.http_status));
        obj.insert(
            "status_reason".to_string(),
            json!(ci.status_reason.map(StatusUnknown::as_str)),
        );
    }
    if let Some(w) = wait_result
        && let Some(obj) = result.as_object_mut()
    {
        obj.insert("wait".to_string(), w);
    }
    if let Some(wf) = wait_for_result
        && let Some(obj) = result.as_object_mut()
    {
        obj.insert("wait_for".to_string(), wf);
    }
    if let Some(c) = consent
        && let Some(obj) = result.as_object_mut()
    {
        obj.insert("consent".to_string(), c);
    }
    crate::commands::network_conditions::insert_echo(&mut result, conditions_applied.as_ref());
    if page_args.with_page {
        crate::commands::page_view::attach(
            cli,
            &mut ctx,
            &mut result,
            Some(cli.timeout),
            page_args,
        )?;
    }
    let mut meta = json!({});
    let page_text = crate::commands::page_view::lift_meta(cli, &mut result, &mut meta);
    crate::connection_meta::merge_into_if_verbose(
        &mut meta,
        &cli.host,
        cli.port,
        None,
        cli.is_verbose(),
    );
    let envelope = output::envelope(&result, 1, &meta);

    let hint_ctx = HintContext::new(HintSource::Navigate);
    OutputPipeline::from_cli(cli)?.finalize_with_hints(&envelope, Some(&hint_ctx))?;
    crate::commands::page_view::render_text_section(page_text.as_ref());
    Ok(())
}

/// Apply output controls (sort, limit, fields) to network entries from navigate.
///
/// Iteration 126: this always returns the ONE canonical network object built by
/// [`crate::commands::network::build_canonical_network`] —
/// `{entries, shown, total, truncated, total_requests, total_transfer_bytes,
/// by_cause_type, slowest, timeout_reached, ...}` — on every path (busy or
/// quiet page, detail or summary mode, `--all` or default). Previously this
/// flipped between a bare array (quiet/`--all`), a truncation object (busy), and
/// a summary object (non-detail), so `.results.network.entries` and
/// `.results.network.total_requests` threw `cannot index array` half the time.
///
/// In detail mode (`--detail`/`--jq`/`--sort`/`--limit`/`--fields`/`--all`, and
/// `--headers`/`--security`, which imply it exactly as on `network`) the
/// `entries` list is sorted, capped at 20 (unless `--all`), and field-projected;
/// in summary mode `entries` carries the full unsorted capture. Summary fields
/// (`total_requests`, …) always reflect the full capture regardless of the view.
///
/// `enrich` runs on the shown entries after the cap and before `--fields`
/// projection, while they still carry the internal `_resource_id` marker — the
/// hook `--headers`/`--security` use to join each entry to its
/// `NetworkEventActor`. The marker is stripped from every entry afterwards.
/// With `--security` the object also carries `insecure_requests`, counted over
/// the full capture as on `network --security`.
///
/// `timeout_reached` is forwarded to [`crate::commands::network::build_network_summary`]
/// so the object carries the hint field when the collection deadline fired while
/// events were still arriving.
pub(crate) fn apply_network_controls(
    cli: &Cli,
    network_entries: &[serde_json::Value],
    timeout_reached: bool,
    detail: NetworkDetail,
    enrich: impl FnOnce(&mut [serde_json::Value]),
) -> Result<serde_json::Value, AppError> {
    let use_detail = cli.detail
        || cli.jq.is_some()
        || cli.sort.is_some()
        || cli.limit.is_some()
        || cli.all
        || cli.fields.is_some()
        || detail.headers
        || detail.security;

    let mut canonical = if use_detail {
        let controls = OutputControls::from_cli(cli, SortDir::Desc);
        let added: Vec<&str> = [(detail.headers, "headers"), (detail.security, "security")]
            .into_iter()
            .filter_map(|(on, key)| on.then_some(key))
            .collect();
        crate::commands::network::validate_controls_for_view(&controls, network_entries, &added)?;
        let mut sorted = network_entries.to_vec();
        if cli.sort.is_none() {
            let dir = controls.sort_dir;
            sorted.sort_by(|a, b| {
                let da = a["duration_ms"].as_f64().unwrap_or(0.0);
                let db = b["duration_ms"].as_f64().unwrap_or(0.0);
                let cmp = da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal);
                match dir {
                    SortDir::Asc => cmp,
                    SortDir::Desc => cmp.reverse(),
                }
            });
        } else {
            controls.apply_sort(&mut sorted)?;
        }
        let (mut limited, total, truncated) = controls.apply_limit(sorted, Some(20));
        enrich(&mut limited);
        strip_resource_ids(&mut limited);
        let limited = controls.apply_fields(limited);
        let shown = limited.len();
        // Summary fields are computed from the FULL capture (`network_entries`),
        // never the truncated/field-projected `limited` view.
        crate::commands::network::build_canonical_network(
            limited,
            shown,
            total,
            truncated,
            network_entries,
            timeout_reached,
        )
    } else {
        // Summary mode: `entries` carries the full unsorted capture so consumers
        // can still reach `.entries` without flipping to detail mode.
        let mut entries = network_entries.to_vec();
        strip_resource_ids(&mut entries);
        let total = entries.len();
        crate::commands::network::build_canonical_network(
            entries,
            total,
            total,
            false,
            network_entries,
            timeout_reached,
        )
    };
    if detail.security
        && let Some(obj) = canonical.as_object_mut()
    {
        obj.insert(
            "insecure_requests".to_string(),
            json!(crate::commands::network::count_insecure_requests(
                network_entries
            )),
        );
    }
    Ok(canonical)
}

/// Which per-request detail `navigate --with-network` fetches before its
/// connection closes.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct NetworkDetail {
    /// `--headers`: request + response headers per shown entry.
    pub(crate) headers: bool,
    /// `--security`: TLS/certificate detail per shown entry.
    pub(crate) security: bool,
}

/// Remove the internal `_resource_id` join marker from every entry.
fn strip_resource_ids(entries: &mut [serde_json::Value]) {
    for entry in entries {
        if let Some(obj) = entry.as_object_mut() {
            obj.remove("_resource_id");
        }
    }
}
