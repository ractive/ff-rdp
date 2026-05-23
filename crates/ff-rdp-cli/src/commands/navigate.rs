use std::time::{Duration, Instant};

use ff_rdp_core::{
    NavCause, RdpTransport, Resource, ResourceCommand, ResourceType, RootActor, TabActor,
    WatcherActor, WindowGlobalTarget, parse_network_resource_updates, parse_network_resources,
};
use serde_json::json;

use crate::cli::args::Cli;
use crate::error::AppError;
use crate::hints::{HintContext, HintSource};
use crate::output;
use crate::output_controls::{OutputControls, SortDir};
use crate::output_pipeline::OutputPipeline;

use super::connect_tab::connect_and_get_target;
use super::js_helpers::{
    WaitForPredicate, escape_selector, poll_js_condition, wait_for_predicates,
};
use super::network_events::{
    build_network_entries, drain_network_events_timed, drain_network_from_daemon, merge_updates,
    serialize_network_resources_for_buffer,
};
use super::url_validation::validate_url;

/// Restore the socket read timeout to the value established at connect time.
///
/// Called after `drain_network_events` completes so that subsequent RDP
/// round-trips (e.g. unwatch, wait condition polling) use the original timeout.
/// Failures are logged and swallowed — the drain has already completed.
fn restore_timeout(transport: &mut RdpTransport, original_timeout_ms: u64) {
    if let Err(e) = transport.set_read_timeout(Some(Duration::from_millis(original_timeout_ms))) {
        eprintln!("warning: failed to restore socket read timeout: {e:#}");
    }
}

/// The readiness level to wait for before declaring navigation complete.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, clap::ValueEnum)]
#[clap(rename_all = "lowercase")]
pub enum WaitLevel {
    /// Return as soon as `dom-loading` fires (URL committed).
    Loading,
    /// Return as soon as `dom-interactive` fires (DOM parsed, scripts may still be running).
    Interactive,
    /// Return as soon as `dom-complete` fires (all resources loaded) — default.
    #[default]
    Complete,
}

/// Options controlling an optional wait condition after navigation.
///
/// # False positive risk
///
/// If the *previous* page already satisfies the wait condition (same selector
/// present, or same text visible) before the new page begins loading, the poll
/// loop may observe a truthy result on the old DOM and return immediately —
/// before the navigation has actually completed.  Callers should be aware of
/// this when reusing the same selector or text across navigations.
// Field names intentionally carry the `wait_` prefix to match the CLI flags
// they correspond to (--wait-text, --wait-selector, --wait-timeout).
#[allow(clippy::struct_field_names)]
pub struct WaitAfterNav<'a> {
    /// Wait until this text appears anywhere on the page body.
    pub wait_text: Option<&'a str>,
    /// Wait until an element matching this CSS selector exists in the DOM.
    pub wait_selector: Option<&'a str>,
    /// Timeout in milliseconds for the wait condition (default: 5000).
    pub wait_timeout: u64,
    /// Skip the default commit-wait and return immediately after navigate is dispatched.
    pub no_wait: bool,
    /// Additional wait-for predicates to evaluate after the document commits.
    /// Each element is a raw predicate string: `selector:<css>`, `text:<substr>`, etc.
    pub wait_for: &'a [String],
    /// Readiness level to wait for (default: `Complete`).
    pub wait_level: WaitLevel,
}

impl WaitAfterNav<'_> {
    fn has_condition(&self) -> bool {
        self.wait_text.is_some() || self.wait_selector.is_some()
    }
}

/// The result of waiting for a navigation to commit.
#[derive(Debug)]
struct CommitInfo {
    /// The URL observed after the navigation committed.
    committed_url: String,
    /// The `document.readyState` observed when the commit condition was met.
    ready_state: String,
    /// Wall-clock milliseconds elapsed from navigate dispatch to commit.
    elapsed_ms: u64,
}

/// Wait for a document-event on the bus (level determined by `wait_level`),
/// pumping the transport until the condition is met or the timeout elapses.
///
/// - [`WaitLevel::Loading`]     — resolves on `dom-loading`.
/// - [`WaitLevel::Interactive`] — resolves on `dom-interactive` (or earlier
///   `dom-loading` for neterror detection).
/// - [`WaitLevel::Complete`]    — resolves on `dom-complete` (default).
///
/// Always returns `Err(AppError::Navigation { … })` on `about:neterror`
/// regardless of `wait_level`.
///
/// Returns a [`CommitInfo`] describing the outcome.  Returns
/// `Err(AppError::Timeout)` when the target event does not arrive within
/// `timeout_ms`.
///
/// The caller must have already subscribed to [`ResourceType::DocumentEvent`]
/// via `bus` before calling this function.  The subscription is left open so
/// that the caller can unsubscribe at its own discretion.
fn wait_for_doc_complete(
    transport: &mut RdpTransport,
    bus: &mut ResourceCommand,
    rx: &std::sync::mpsc::Receiver<std::sync::Arc<Resource>>,
    timeout_ms: u64,
    wait_level: WaitLevel,
) -> Result<CommitInfo, AppError> {
    let deadline = Instant::now() + Duration::from_millis(timeout_ms);

    // Use a short socket read timeout so we can check the deadline
    // even when the server is quiet.
    let poll_interval = Duration::from_millis(100);
    transport
        .set_read_timeout(Some(poll_interval))
        .map_err(|e| AppError::from(anyhow::anyhow!("set_read_timeout: {e:#}")))?;

    let started = Instant::now();
    let mut commit_url: Option<String> = None;
    // Track whether we've seen dom-interactive so Loading/Interactive can return early.
    let mut interactive_url: Option<String> = None;

    loop {
        // Check deadline first so we do not drain another batch of events
        // when the timeout has already expired.  This bounds the overrun to
        // at most one `poll_interval` (100 ms).
        if Instant::now() >= deadline {
            let level_name = match wait_level {
                WaitLevel::Loading => "dom-loading",
                WaitLevel::Interactive => "dom-interactive",
                WaitLevel::Complete => "dom-complete",
            };
            return Err(AppError::Timeout(format!(
                "navigate: page did not fire {level_name} within the timeout — \
                 use --no-wait to skip or increase --timeout"
            )));
        }

        // Drain the channel — may have been filled by a previous recv batch.
        while let Ok(arc) = rx.try_recv() {
            if let Resource::DocumentEvent(v) = arc.as_ref() {
                let name = v.get("name").and_then(|n| n.as_str()).unwrap_or("");
                let url = v
                    .get("url")
                    .and_then(|u| u.as_str())
                    .unwrap_or("")
                    .to_owned();

                match name {
                    "dom-loading" => {
                        // Always detect neterror early — Firefox loads about:neterror
                        // as a document and we will see dom-loading with the
                        // neterror URL before dom-complete fires.
                        if is_neterror_url(&url) {
                            let nav_cause = classify_neterror(&url).map_or(
                                NavCause::Unknown("unknown".to_owned()),
                                NavCause::from_e_param,
                            );
                            return Err(AppError::Navigation {
                                cause: nav_cause,
                                url,
                            });
                        }
                        commit_url = Some(url.clone());
                        // --wait loading: resolve immediately on dom-loading.
                        if wait_level == WaitLevel::Loading {
                            let elapsed_ms =
                                u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
                            return Ok(CommitInfo {
                                committed_url: url,
                                ready_state: "loading".to_owned(),
                                elapsed_ms,
                            });
                        }
                    }
                    "dom-interactive" => {
                        // Record the interactive URL. If we haven't seen dom-loading
                        // yet, treat this as both loading and interactive.
                        let eff_url = if url.is_empty() {
                            commit_url.clone().unwrap_or_default()
                        } else {
                            url.clone()
                        };
                        if commit_url.is_none() {
                            commit_url = Some(eff_url.clone());
                        }
                        interactive_url = Some(eff_url.clone());
                        // --wait interactive: resolve on dom-interactive.
                        if wait_level == WaitLevel::Interactive && commit_url.is_some() {
                            let elapsed_ms =
                                u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
                            return Ok(CommitInfo {
                                committed_url: eff_url,
                                ready_state: "interactive".to_owned(),
                                elapsed_ms,
                            });
                        }
                    }
                    "dom-complete" => {
                        // Ignore pre-existing/stale dom-complete events that
                        // are not tied to *this* navigate call.  The watcher
                        // emits both existing and new resources, so an early
                        // dom-complete may arrive before our dom-loading.
                        if commit_url.is_none() {
                            continue;
                        }
                        let elapsed_ms =
                            u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
                        let committed = interactive_url
                            .take()
                            .or_else(|| commit_url.take())
                            .unwrap_or_default();
                        return Ok(CommitInfo {
                            committed_url: committed,
                            ready_state: "complete".to_owned(),
                            elapsed_ms,
                        });
                    }
                    _ => {}
                }
            }
        }

        // Pump the transport — will block up to `poll_interval` then return
        // Timeout, which we treat as idle (keep looping).
        match transport.recv() {
            Ok(msg) => bus.dispatch_event(&msg),
            Err(ff_rdp_core::ProtocolError::Timeout) => {}
            Err(e) => {
                return Err(AppError::from(anyhow::anyhow!(
                    "navigate: transport error waiting for dom-complete: {e:#}"
                )));
            }
        }
    }
}

/// Extract the `e=` parameter value from an `about:neterror` URL.
///
/// Returns the raw `e=` value so the caller can pass it to
/// [`NavCause::from_e_param`] for typed classification.
fn classify_neterror(url: &str) -> Option<&str> {
    // about:neterror?e=dnsNotFound&...
    let query = url.strip_prefix("about:neterror?")?;
    query
        .split('&')
        .find(|seg| seg.starts_with("e="))?
        .strip_prefix("e=")
}

/// Returns `true` when `url` begins with `about:neterror`.
fn is_neterror_url(url: &str) -> bool {
    url.starts_with("about:neterror")
}

/// Check whether two URLs refer to the same origin + path (ignoring query, hash,
/// and trailing slash).  Used by the cross-origin race fix (Theme G): when a
/// commit-wait times out but the landed URL shares scheme+host+port+path with
/// the requested URL, we treat the navigation as successful.
#[allow(dead_code)]
fn urls_match_scheme_host_path(a: &str, b: &str) -> bool {
    fn strip_query_and_hash(u: &str) -> &str {
        let no_hash = u.split_once('#').map_or(u, |(h, _)| h);
        no_hash.split_once('?').map_or(no_hash, |(h, _)| h)
    }
    fn strip_trailing_slash(u: &str) -> &str {
        u.strip_suffix('/').unwrap_or(u)
    }
    let norm_a = strip_trailing_slash(strip_query_and_hash(a));
    let norm_b = strip_trailing_slash(strip_query_and_hash(b));
    norm_a == norm_b
}

/// Run the `--wait-for` predicates from `wait_opts`, re-resolving actors first.
///
/// Returns `Some(json)` when predicates were specified, `None` when none were given.
fn run_wait_for_predicates(
    ctx: &mut super::connect_tab::ConnectedTab,
    opts: &WaitAfterNav<'_>,
) -> Result<Option<serde_json::Value>, AppError> {
    if opts.wait_for.is_empty() {
        return Ok(None);
    }

    let predicates: Vec<WaitForPredicate<'_>> = opts
        .wait_for
        .iter()
        .map(|s| WaitForPredicate::parse(s))
        .collect::<Result<_, _>>()?;

    // Re-resolve console actor for the new document.
    let tab_actor = ctx.target_tab_actor().clone();
    let fresh_target =
        TabActor::get_target(ctx.transport_mut(), &tab_actor).map_err(AppError::from)?;
    let console_actor = fresh_target.console_actor;

    let started = Instant::now();
    wait_for_predicates(ctx, &console_actor, &predicates, opts.wait_timeout)?;
    let elapsed = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);

    Ok(Some(json!({
        "waited": true,
        "elapsed_ms": elapsed,
        "predicates": opts.wait_for,
    })))
}

/// Refresh the console actor in `ctx` after navigation.
///
/// Theme K: the consoleActor ID cached in `ctx.target` is bound to the old
/// docshell.  After any navigate (including to about:neterror pages), call this
/// to fetch a fresh actor so the next `eval` does not get `noSuchActor`.
///
/// This is a best-effort operation; failures are logged to stderr and swallowed.
fn refresh_console_actor(ctx: &mut super::connect_tab::ConnectedTab) {
    ctx.refresh_target();
}

/// Check whether the REAL tab URL (from `listTabs`) is an about:neterror page.
///
/// Theme F: `window.location.href` on an about:neterror page returns the
/// **failed URL** (from the `u=` query parameter), not the `about:neterror?...`
/// URL itself.  So `CommitInfo.committed_url` — which comes from
/// `window.location.href` — cannot be used to detect neterror pages.
///
/// This function queries `listTabs` which returns the tab descriptor's URL
/// field, which Firefox populates with the REAL URL (`about:neterror?e=...`).
///
/// Returns an `AppError` when the tab has landed on an about:neterror page.
/// Returns `None` when the tab URL is clean or the check cannot be performed.
fn check_real_tab_url_for_neterror(
    ctx: &mut super::connect_tab::ConnectedTab,
    requested_url: &str,
) -> Option<AppError> {
    // listTabs is a root-level RPC and may interleave with other pending events,
    // so we only do this when we suspect a neterror (non-fatal: if it fails we
    // fall through to the caller's success path).
    let Ok(tabs) = RootActor::list_tabs(ctx.transport_mut()) else {
        return None;
    };

    // Find the selected tab (or any tab — we just launched a single navigate).
    let tab_url = tabs
        .into_iter()
        .find(|t| t.selected)
        .map(|t| t.url)
        .unwrap_or_default();

    if !is_neterror_url(&tab_url) {
        return None;
    }

    let nav_cause = classify_neterror(&tab_url).map_or(
        NavCause::Unknown("unknown".to_owned()),
        NavCause::from_e_param,
    );
    Some(AppError::Navigation {
        cause: nav_cause,
        url: requested_url.to_owned(),
    })
}

/// Navigate to `url` and return the result value without printing.
///
/// Called by the script runner, which handles its own NDJSON output.
///
/// # Navigation wait strategy (Theme A)
///
/// Instead of polling `window.location.href` + `document.readyState` via
/// `evaluateJSAsync`, we subscribe to `document-event` resources on the
/// watcher bus **before** sending `navigateTo`.  Firefox pushes `dom-loading`
/// (with the URL being loaded) and `dom-complete` as events; we wait for
/// `dom-complete` to declare success.  `dom-loading` with an `about:neterror`
/// URL signals a DNS/network failure without having to wait for a timeout.
///
/// This closes the `navigate-race-timeout` and `navigate-success-on-bad-dns`
/// gaps from the stability roadmap.
pub fn run_core(
    cli: &Cli,
    url: &str,
    wait_opts: &WaitAfterNav<'_>,
) -> Result<serde_json::Value, AppError> {
    if !cli.allow_unsafe_urls {
        validate_url(url)?;
    }
    let mut ctx = connect_and_get_target(cli)?;
    let target_actor = ctx.target.actor.clone();
    let tab_actor = ctx.target_tab_actor().clone();

    // Get the watcher actor and subscribe to document-event resources before
    // sending navigateTo so we don't miss any events that arrive immediately
    // after the navigate (Firefox may dispatch dom-loading very quickly).
    let watcher_actor =
        TabActor::get_watcher(ctx.transport_mut(), &tab_actor).map_err(AppError::from)?;

    let commit_info = if wait_opts.no_wait {
        // --no-wait: send navigateTo via the standard actor_request (response
        // is the navigateTo ack) and return immediately, no bus needed.
        WindowGlobalTarget::navigate_to(ctx.transport_mut(), &target_actor, url)
            .map_err(AppError::from)?;
        None
    } else {
        let mut bus = ResourceCommand::new(watcher_actor.clone());
        let (sub_id, rx) = bus
            .subscribe(ctx.transport_mut(), &[ResourceType::DocumentEvent])
            .map_err(|e| AppError::from(anyhow::anyhow!("document-event subscribe: {e:#}")))?;

        // Send navigateTo raw (not via actor_request) so we don't lose
        // resources-available-array events that arrive before the ack.
        ctx.transport_mut()
            .send(&json!({
                "to": target_actor.as_ref(),
                "type": "navigateTo",
                "url": url,
            }))
            .map_err(AppError::from)?;

        let result = wait_for_doc_complete(
            ctx.transport_mut(),
            &mut bus,
            &rx,
            cli.timeout,
            wait_opts.wait_level,
        );

        // Unsubscribe regardless of outcome so Firefox cleans up server state.
        let _ = bus.unsubscribe(ctx.transport_mut(), sub_id);

        // Restore the original timeout so subsequent RDP round-trips (e.g.
        // wait-text / wait-selector polling) use the configured timeout.
        restore_timeout(ctx.transport_mut(), cli.timeout);

        match result {
            Ok(ci) => Some(ci),
            Err(e) => return Err(e),
        }
    };

    // Theme K: invalidate the cached consoleActor after any navigate so the
    // next `eval` call fetches a fresh actor bound to the new docshell.
    refresh_console_actor(&mut ctx);

    let wait_result = wait_after_navigate(&mut ctx, wait_opts)?;

    // Parse and run --wait-for predicates after commit.
    let wait_for_result = run_wait_for_predicates(&mut ctx, wait_opts)?;

    let mut result = json!({"navigated": url});
    if let Some(ref ci) = commit_info
        && let Some(obj) = result.as_object_mut()
    {
        obj.insert("committed_url".to_string(), json!(ci.committed_url));
        obj.insert("ready_state".to_string(), json!(ci.ready_state));
        obj.insert("elapsed_ms".to_string(), json!(ci.elapsed_ms));
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
    Ok(result)
}

pub fn run(cli: &Cli, url: &str, wait_opts: &WaitAfterNav<'_>) -> Result<(), AppError> {
    let result = run_core(cli, url, wait_opts)?;
    let mut meta = json!({});
    crate::connection_meta::merge_into_if_verbose(
        &mut meta,
        &cli.host,
        cli.port,
        None,
        cli.is_verbose(),
    );
    let envelope = output::envelope(&result, 1, &meta);

    let hint_ctx = HintContext::new(HintSource::Navigate);
    OutputPipeline::from_cli(cli)?
        .finalize_with_hints(&envelope, Some(&hint_ctx))
        .map_err(AppError::from)
}

/// Navigate to `url` and capture all network requests made during navigation.
///
/// The flow on a single TCP connection is:
/// 1. Connect and resolve the target tab.
/// 2. Get the WatcherActor via `TabActor::get_watcher`.
/// 3. Subscribe to `"network-event"` resources via `WatcherActor::watch_resources`.
/// 4. Navigate with `WindowGlobalTarget::navigate_to`.
/// 5. Drain `resources-available-array` / `resources-updated-array` events
///    (timeout-bounded, same pattern as the `network` command).
/// 6. Merge updates into resources by `resource_id`.
/// 7. Unwatch resources to clean up server-side state.
/// 8. Optionally wait for a condition (--wait-text / --wait-selector).
/// 9. Emit combined JSON output.
pub fn run_with_network(
    cli: &Cli,
    url: &str,
    wait_opts: &WaitAfterNav<'_>,
    network_timeout_ms: u64,
) -> Result<(), AppError> {
    if !cli.allow_unsafe_urls {
        validate_url(url)?;
    }
    let mut ctx = connect_and_get_target(cli)?;
    let target_actor = ctx.target.actor.clone();

    if ctx.via_daemon {
        // Tell the daemon to stream network events in real-time instead of
        // buffering.  This clears the existing buffer so we only capture
        // events from *this* navigation.
        crate::daemon::client::start_daemon_stream(ctx.transport_mut(), "network-event")
            .map_err(AppError::from)?;

        // Send the navigateTo request without reading its response — same as
        // the non-daemon path.  The daemon will forward the ack and also
        // stream watcher events directly to us.
        ctx.transport_mut()
            .send(&json!({
                "to": target_actor.as_ref(),
                "type": "navigateTo",
                "url": url,
            }))
            .map_err(AppError::from)?;

        // Drain streamed watcher events for the total_timeout wall-clock
        // duration, using short 500ms poll intervals internally.  This
        // captures events that arrive in bursts with gaps (e.g. the page
        // navigation itself may take 1-2 seconds before any network events
        // start, which would incorrectly fire an idle-based timeout early).
        // Always stop streaming before propagating errors from drain so the
        // daemon does not get stuck in streaming mode on failure.
        let drain_result = drain_network_events_timed(
            ctx.transport_mut(),
            Duration::from_millis(network_timeout_ms),
        );

        // Restore the original connection timeout before stopping the stream
        // so any RDP round-trip uses the right timeout.
        restore_timeout(ctx.transport_mut(), cli.timeout);

        // Stop streaming and collect any in-flight watcher frames that arrived
        // between the idle-timeout cutoff and the stop-stream acknowledgement.
        // These are events the daemon forwarded after drain_network_events
        // returned but before it processed our stop-stream request.
        let inflight = match crate::daemon::client::stop_daemon_stream_draining(
            ctx.transport_mut(),
            "network-event",
        ) {
            Ok(frames) => frames,
            Err(e) => {
                eprintln!("warning: failed to stop daemon stream: {e:#}");
                vec![]
            }
        };

        let (mut all_resources, mut all_updates, timeout_reached) =
            drain_result.map_err(AppError::from)?;

        // Parse and merge any in-flight frames collected from stop_daemon_stream.
        for frame in &inflight {
            let msg_type = frame
                .get("type")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default();
            match msg_type {
                "resources-available-array" => {
                    all_resources.extend(parse_network_resources(frame));
                }
                "resources-updated-array" => {
                    all_updates.extend(parse_network_resource_updates(frame));
                }
                _ => {}
            }
        }

        // After stop-stream the daemon reverts to buffering.  Any events that
        // arrived at Firefox between the idle-timeout firing and the daemon
        // removing this client's stream subscription get buffered instead of
        // forwarded.  Drain that residual buffer now so nothing is lost.
        match drain_network_from_daemon(ctx.transport_mut()) {
            Ok((residual_resources, residual_updates)) => {
                all_resources.extend(residual_resources);
                all_updates.extend(residual_updates);
            }
            Err(e) => {
                eprintln!("warning: failed to drain residual daemon buffer after stream: {e:#}");
            }
        }

        // Store the collected events back into the daemon buffer so that a
        // subsequent `ff-rdp network` call can read them rather than falling
        // back to the Performance API (iter-61j G).  We record a navigation
        // boundary so `--since -1` scopes the result correctly.
        //
        // `all_updates` is consumed by `merge_updates` later, so we build the
        // update serialization before that.  Failures are non-fatal — streaming
        // already returned the data; the worst case is the next `network` call
        // falls back to the perf API as before.
        {
            let update_refs: Vec<(u64, &_)> =
                all_updates.iter().map(|u| (u.resource_id, u)).collect();
            let items = serialize_network_resources_for_buffer(&all_resources, &update_refs);
            if let Err(e) = crate::daemon::client::store_network_events(ctx.transport_mut(), &items)
                && cli.is_verbose()
            {
                eprintln!(
                    "warning: navigate --with-network: could not store events in daemon buffer: {e:#}"
                );
            }
        }

        // The network drain already waited for events to settle; no separate
        // commit-wait is needed.  Neterror detection runs via listTabs below.
        let commit_info: Option<CommitInfo> = None;

        // Theme K: refresh consoleActor after navigate.
        refresh_console_actor(&mut ctx);

        // Detect about:neterror in the daemon --with-network path.
        if let Some(err) = check_real_tab_url_for_neterror(&mut ctx, url) {
            return Err(err);
        }

        let wait_result = wait_after_navigate(&mut ctx, wait_opts)?;
        let wait_for_result = run_wait_for_predicates(&mut ctx, wait_opts)?;

        let update_map = merge_updates(all_updates);
        let network_entries = build_network_entries(&all_resources, &update_map);

        let network_entries = apply_network_controls(cli, network_entries, timeout_reached);

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
        let mut meta = json!({});
        crate::connection_meta::merge_into_if_verbose(
            &mut meta,
            &cli.host,
            cli.port,
            None,
            cli.is_verbose(),
        );
        let envelope = output::envelope(&result, 1, &meta);
        let hint_ctx = HintContext::new(HintSource::Navigate);
        return OutputPipeline::from_cli(cli)?
            .finalize_with_hints(&envelope, Some(&hint_ctx))
            .map_err(AppError::from);
    }

    let tab_actor = ctx.target_tab_actor().clone();

    // Get watcher actor for resource subscriptions.
    let watcher_actor =
        TabActor::get_watcher(ctx.transport_mut(), &tab_actor).map_err(AppError::from)?;

    // Subscribe to network events before navigating so we capture everything.
    WatcherActor::watch_resources(ctx.transport_mut(), &watcher_actor, &["network-event"])
        .map_err(AppError::from)?;

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

    let (all_resources, all_updates, timeout_reached) = drain_result.map_err(AppError::from)?;

    // Merge updates into resources by resource_id.
    let update_map = merge_updates(all_updates);

    // Build the network entries array (no URL/method filtering here).
    let network_entries = build_network_entries(&all_resources, &update_map);

    // Unwatch to clean up server-side resources.
    let _ =
        WatcherActor::unwatch_resources(ctx.transport_mut(), &watcher_actor, &["network-event"]);

    // NOTE: In the non-daemon path, wait_after_navigate is called *after*
    // draining network events and unwatching resources, so network data is
    // already fully collected before we begin waiting.  The daemon path
    // (above) starts the wait before building entries because there is no
    // subscription lifecycle to tear down.

    // The network drain already waited for events to settle; no separate
    // commit-wait is needed.  Neterror detection runs via listTabs below.
    let commit_info: Option<CommitInfo> = None;

    // Theme K: refresh consoleActor after navigate.
    refresh_console_actor(&mut ctx);

    // Detect about:neterror in the non-daemon --with-network path.
    if let Some(err) = check_real_tab_url_for_neterror(&mut ctx, url) {
        return Err(err);
    }

    let wait_result = wait_after_navigate(&mut ctx, wait_opts)?;
    let wait_for_result = run_wait_for_predicates(&mut ctx, wait_opts)?;

    let network_entries = apply_network_controls(cli, network_entries, timeout_reached);

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
    let mut meta = json!({});
    crate::connection_meta::merge_into_if_verbose(
        &mut meta,
        &cli.host,
        cli.port,
        None,
        cli.is_verbose(),
    );
    let envelope = output::envelope(&result, 1, &meta);

    let hint_ctx = HintContext::new(HintSource::Navigate);
    OutputPipeline::from_cli(cli)?
        .finalize_with_hints(&envelope, Some(&hint_ctx))
        .map_err(AppError::from)
}

/// Apply output controls (sort, limit, fields) to network entries from navigate.
///
/// In detail mode (when the user sets --detail, --jq, --sort, --limit, --fields,
/// or --all), returns the processed array. Otherwise returns a summary object.
///
/// `timeout_reached` is forwarded to [`build_network_summary`] so it can include
/// the hint field when the collection deadline fired while events were still arriving.
fn apply_network_controls(
    cli: &Cli,
    network_entries: Vec<serde_json::Value>,
    timeout_reached: bool,
) -> serde_json::Value {
    let use_detail = cli.detail
        || cli.jq.is_some()
        || cli.sort.is_some()
        || cli.limit.is_some()
        || cli.all
        || cli.fields.is_some();

    if use_detail {
        let controls = OutputControls::from_cli(cli, SortDir::Desc);
        let mut detail = network_entries;
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
        let limited = controls.apply_fields(limited);
        if truncated {
            let shown = limited.len();
            json!({
                "entries": limited,
                "shown": shown,
                "total": total,
                "truncated": true,
                "hint": format!("showing {shown} of {total}, use --all for complete list"),
            })
        } else {
            json!(limited)
        }
    } else {
        super::network::build_network_summary(&network_entries, timeout_reached)
    }
}

/// Poll a JS condition after navigation until it becomes truthy or times out.
///
/// Returns `Ok(Some(json))` when the condition is met, `Ok(None)` when no
/// condition was requested, and `Err` when the timeout expires or evaluation
/// fails with an exception.
fn wait_after_navigate(
    ctx: &mut super::connect_tab::ConnectedTab,
    opts: &WaitAfterNav<'_>,
) -> Result<Option<serde_json::Value>, AppError> {
    if !opts.has_condition() {
        return Ok(None);
    }

    let js = if let Some(sel) = opts.wait_selector {
        let escaped = escape_selector(sel);
        format!("document.querySelector('{escaped}') !== null")
    } else if let Some(text) = opts.wait_text {
        let escaped = serde_json::to_string(text)
            .map_err(|e| AppError::from(anyhow::anyhow!("failed to encode wait-text: {e}")))?;
        format!("(document.body && document.body.innerText.includes({escaped}))")
    } else {
        // has_condition() guarantees at least one is set; this branch is unreachable.
        return Ok(None);
    };

    // Re-resolve the target after navigation. The console actor cached during
    // the initial `connect_and_get_target` is bound to the docshell that
    // existed *before* navigation; once navigation tears that docshell down,
    // any `evaluateJSAsync` against the old console actor fails with
    // `noSuchActor`. Calling `getTarget` again on the tab descriptor returns a
    // fresh set of actors bound to the new docshell.
    let tab_actor = ctx.target_tab_actor().clone();
    let refreshed =
        TabActor::get_target(ctx.transport_mut(), &tab_actor).map_err(AppError::from)?;
    let console_actor = refreshed.console_actor;

    let condition = describe_wait_condition(opts);
    let timeout_msg = format!(
        "navigate wait timed out after {}ms — condition not met: {condition}; increase with --wait-timeout",
        opts.wait_timeout
    );

    let elapsed_ms = poll_js_condition(
        ctx,
        &console_actor,
        &js,
        opts.wait_timeout,
        "navigate wait aborted due to JS exception",
        &timeout_msg,
    )?;

    Ok(Some(json!({
        "waited": true,
        "elapsed_ms": elapsed_ms,
        "condition": condition,
    })))
}

fn describe_wait_condition(opts: &WaitAfterNav<'_>) -> String {
    if let Some(sel) = opts.wait_selector {
        format!("selector={sel:?}")
    } else if let Some(text) = opts.wait_text {
        format!("text={text:?}")
    } else {
        "(none)".into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_wait_opts<'a>() -> WaitAfterNav<'a> {
        WaitAfterNav {
            wait_text: None,
            wait_selector: None,
            wait_timeout: 5000,
            no_wait: false,
            wait_for: &[],
            wait_level: WaitLevel::Complete,
        }
    }

    #[test]
    fn wait_after_nav_no_condition_returns_none() {
        let opts = default_wait_opts();
        assert!(!opts.has_condition());
    }

    #[test]
    fn wait_after_nav_text_has_condition() {
        let opts = WaitAfterNav {
            wait_text: Some("Hello"),
            ..default_wait_opts()
        };
        assert!(opts.has_condition());
    }

    #[test]
    fn wait_after_nav_selector_has_condition() {
        let opts = WaitAfterNav {
            wait_selector: Some("button.submit"),
            ..default_wait_opts()
        };
        assert!(opts.has_condition());
    }

    #[test]
    fn describe_wait_condition_selector() {
        let opts = WaitAfterNav {
            wait_selector: Some("div#main"),
            wait_timeout: 3000,
            ..default_wait_opts()
        };
        assert_eq!(describe_wait_condition(&opts), r#"selector="div#main""#);
    }

    #[test]
    fn describe_wait_condition_text() {
        let opts = WaitAfterNav {
            wait_text: Some("Loaded"),
            wait_timeout: 3000,
            ..default_wait_opts()
        };
        assert_eq!(describe_wait_condition(&opts), r#"text="Loaded""#);
    }

    #[test]
    fn no_wait_field_skips_commit_wait() {
        let opts = WaitAfterNav {
            no_wait: true,
            ..default_wait_opts()
        };
        assert!(opts.no_wait);
        assert!(!opts.has_condition());
    }

    #[test]
    fn wait_for_empty_slice_is_none() {
        let opts = default_wait_opts();
        assert!(opts.wait_for.is_empty());
    }

    // -----------------------------------------------------------------------
    // Theme F / B: neterror detection + typed NavCause mapping
    // -----------------------------------------------------------------------

    #[test]
    fn classify_neterror_dns_not_found() {
        let url = "about:neterror?e=dnsNotFound&u=https%3A//bad.invalid/";
        let e_param = classify_neterror(url).unwrap();
        assert_eq!(e_param, "dnsNotFound");
        assert_eq!(NavCause::from_e_param(e_param), NavCause::DnsFail);
    }

    #[test]
    fn classify_neterror_connection_failure() {
        let url = "about:neterror?e=connectionFailure&u=foo";
        let e_param = classify_neterror(url).unwrap();
        assert_eq!(NavCause::from_e_param(e_param), NavCause::ConnReset);
    }

    #[test]
    fn classify_neterror_unknown_code_passthrough() {
        let url = "about:neterror?e=someNewFirefoxCode&u=foo";
        let e_param = classify_neterror(url).unwrap();
        assert!(matches!(
            NavCause::from_e_param(e_param),
            NavCause::Unknown(_)
        ));
    }

    #[test]
    fn classify_neterror_returns_none_for_non_neterror() {
        assert!(classify_neterror("https://example.com").is_none());
        assert!(classify_neterror("about:blank").is_none());
    }

    #[test]
    fn is_neterror_url_detects_about_neterror() {
        assert!(is_neterror_url("about:neterror?e=dnsNotFound"));
        assert!(!is_neterror_url("https://example.com"));
        assert!(!is_neterror_url("about:blank"));
    }

    // -----------------------------------------------------------------------
    // Theme G: cross-origin URL matching
    // -----------------------------------------------------------------------

    #[test]
    fn urls_match_scheme_host_path_identical() {
        assert!(urls_match_scheme_host_path(
            "https://example.com/path",
            "https://example.com/path"
        ));
    }

    #[test]
    fn urls_match_scheme_host_path_strips_query() {
        assert!(urls_match_scheme_host_path(
            "https://example.com/path?q=1",
            "https://example.com/path?q=2"
        ));
        assert!(urls_match_scheme_host_path(
            "https://example.com/path?q=1",
            "https://example.com/path"
        ));
    }

    #[test]
    fn urls_match_scheme_host_path_strips_hash() {
        assert!(urls_match_scheme_host_path(
            "https://example.com/path#a",
            "https://example.com/path#b"
        ));
    }

    #[test]
    fn urls_match_scheme_host_path_strips_trailing_slash() {
        assert!(urls_match_scheme_host_path(
            "https://example.com/path/",
            "https://example.com/path"
        ));
    }

    #[test]
    fn urls_do_not_match_different_paths_scheme_host_path() {
        assert!(!urls_match_scheme_host_path(
            "https://example.com/a",
            "https://example.com/b"
        ));
        assert!(!urls_match_scheme_host_path(
            "https://example.com/",
            "https://other.com/"
        ));
    }

    // -----------------------------------------------------------------------
    // wait_for_doc_complete — deadline ordering regression test (iter-61w)
    //
    // Verifies that the deadline check fires at the top of the outer loop,
    // so that events flooding the channel do not delay timeout detection beyond
    // `timeout_ms + poll_interval` (100 ms).
    // -----------------------------------------------------------------------

    #[test]
    fn deadline_fires_within_timeout_plus_one_poll_interval() {
        use std::io::Write;
        use std::net::TcpListener;
        use std::time::Instant;

        use ff_rdp_core::transport::{RdpTransport, encode_frame};

        const TIMEOUT_MS: u64 = 50;
        const POLL_MS: u64 = 100;
        // Maximum allowed elapsed: 50ms timeout + 100ms poll + 200ms margin.
        const MAX_ELAPSED_MS: u64 = TIMEOUT_MS + POLL_MS + 200;

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();

        // Spawn a server that only sends the greeting and then idles, so
        // every transport recv times out.  The dom-loading flood that
        // exercises the deadline logic is pre-loaded into the mpsc channel
        // below — the old (post-drain) deadline check could be starved by it.
        let server_handle = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut writer = stream.try_clone().unwrap();

            // Send greeting.
            let greeting = serde_json::json!({
                "from": "root",
                "applicationType": "browser",
                "traits": {}
            });
            let _ = writer
                .write_all(encode_frame(&serde_json::to_string(&greeting).unwrap()).as_bytes());

            // Keep the stream open; send nothing further so the transport times
            // out on every recv call and the deadline logic is exercised.
            // (We don't send dom-complete, so the timeout must fire.)
            std::thread::sleep(Duration::from_secs(1));
        });

        let mut transport =
            RdpTransport::connect("127.0.0.1", port, Duration::from_secs(5)).unwrap();

        // Build a ResourceCommand and an mpsc channel whose receiver we pass to
        // wait_for_doc_complete.  We pre-load the channel with many dom-loading
        // events so the inner drain loop has work to do on each iteration.
        let (tx, rx) = std::sync::mpsc::channel::<std::sync::Arc<Resource>>();
        let dom_loading = std::sync::Arc::new(Resource::DocumentEvent(serde_json::json!({
            "name": "dom-loading",
            "url": "https://example.com/",
        })));
        // Send enough events to fill several drain batches.
        for _ in 0..1000 {
            tx.send(std::sync::Arc::clone(&dom_loading)).unwrap();
        }

        let watcher_actor = ff_rdp_core::ActorId::from("conn0/watcher1");
        let mut bus = ResourceCommand::new(watcher_actor);

        let started = Instant::now();
        let result = wait_for_doc_complete(
            &mut transport,
            &mut bus,
            &rx,
            TIMEOUT_MS,
            WaitLevel::Complete,
        );
        let elapsed_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);

        server_handle.join().unwrap();

        assert!(
            matches!(result, Err(AppError::Timeout(_))),
            "expected Timeout, got: {result:?}"
        );
        assert!(
            elapsed_ms <= MAX_ELAPSED_MS,
            "deadline overrun: elapsed {elapsed_ms}ms > allowed {MAX_ELAPSED_MS}ms"
        );
    }
}
