use std::time::{Duration, Instant};

use ff_rdp_core::{
    RdpTransport, TabActor, WatcherActor, WebConsoleActor, WindowGlobalTarget,
    parse_network_resource_updates, parse_network_resources,
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
}

impl WaitAfterNav<'_> {
    fn has_condition(&self) -> bool {
        self.wait_text.is_some() || self.wait_selector.is_some()
    }
}

/// Read `window.location.href` from the current document.
///
/// Returns `None` when the URL cannot be observed (e.g. the console actor is
/// not yet responsive or the eval raised an exception).  The caller treats
/// this as "no pre-nav URL available" and falls back to readyState-only.
fn capture_current_url(ctx: &mut super::connect_tab::ConnectedTab) -> Option<String> {
    use super::js_helpers::resolve_result;
    let console_actor = ctx.target.console_actor.clone();
    let eval = WebConsoleActor::evaluate_js_async(
        ctx.transport_mut(),
        &console_actor,
        "window.location.href",
    )
    .ok()?;
    if eval.exception.is_some() {
        return None;
    }
    let resolved = resolve_result(ctx, &eval.result).ok()?;
    resolved.as_str().map(str::to_owned)
}

/// Returns `true` when two URLs refer to the same document under the
/// "same-URL navigate" heuristic: equal after stripping any `#fragment` and
/// a single trailing `/`.  Intentionally conservative — false positives turn
/// into a no-op (caller can `ff-rdp reload`), false negatives reintroduce
/// the dogfood-49 timeout bug.  We avoid a full URL parser; only normalise
/// what's likely to legitimately vary across "navigate to the page I'm on"
/// invocations.
fn urls_match_ignore_hash(a: &str, b: &str) -> bool {
    fn normalize(u: &str) -> &str {
        let no_hash = u.split_once('#').map_or(u, |(head, _)| head);
        no_hash.strip_suffix('/').unwrap_or(no_hash)
    }
    normalize(a) == normalize(b)
}

/// The result of waiting for a navigation to commit.
struct CommitInfo {
    /// The URL observed after the navigation committed.
    committed_url: String,
    /// The `document.readyState` observed when the commit condition was met.
    ready_state: String,
    /// Wall-clock milliseconds elapsed from navigate dispatch to commit.
    elapsed_ms: u64,
}

/// Classify an `about:neterror` URL into a structured error type string.
///
/// Firefox encodes the error kind in the `e=` query parameter.  We map the
/// known values to snake_case strings that callers can match on.  Unknown
/// values are returned as-is so callers are not broken by new Firefox error
/// codes.
fn classify_neterror(url: &str) -> Option<(&str, String)> {
    // about:neterror?e=dnsNotFound&...
    let query = url.strip_prefix("about:neterror?")?;
    let e_param = query
        .split('&')
        .find(|seg| seg.starts_with("e="))?
        .strip_prefix("e=")?;

    let error_type = match e_param {
        "dnsNotFound" => "dns_not_found",
        "connectionFailure" => "connection_failed",
        "netTimeout" => "net_timeout",
        "netReset" => "connection_reset",
        "netInterrupt" => "connection_interrupted",
        "connectionRefused" => "connection_refused",
        "unknownProtocolFound" => "unknown_protocol",
        "proxyConnectFailure" => "proxy_connect_failed",
        "proxyAuthorizationRequired" => "proxy_auth_required",
        "contentEncodingError" => "content_encoding_error",
        "remoteXUL" => "remote_xul",
        "cspBlocked" => "csp_blocked",
        "corruptedContentError" => "corrupted_content",
        "sslv3Used" => "ssl_version_error",
        "inadequateSecurityError" => "tls_security_error",
        "blockedByPolicy" => "blocked_by_policy",
        other => other,
    };
    Some((e_param, error_type.to_owned()))
}

/// Returns `true` when `url` begins with `about:neterror`.
fn is_neterror_url(url: &str) -> bool {
    url.starts_with("about:neterror")
}

/// Check whether two URLs refer to the same origin + path (ignoring query, hash,
/// and trailing slash).  Used by the cross-origin race fix (Theme G): when a
/// commit-wait times out but the landed URL shares scheme+host+port+path with
/// the requested URL, we treat the navigation as successful.
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

/// Poll until the page's URL differs from `pre_nav_url` AND
/// `document.readyState` reaches `interactive` / `complete`, whichever comes first.
///
/// The poll runs against the *new* target actors (refreshed via `getTarget`)
/// because the navigate tears down the old docshell; the old console actor
/// would return `noSuchActor`.
///
/// Without the URL change check, the *old* docshell may briefly answer
/// `evaluateJSAsync` with `readyState === 'complete'` for the old page,
/// causing a false-positive commit reading from the previous URL.  When
/// `pre_nav_url` is `None` (no observable pre-nav URL was captured) the
/// readyState-only check is used.
///
/// Returns an error when the timeout elapses before either condition is met.
fn wait_for_commit(
    ctx: &mut super::connect_tab::ConnectedTab,
    requested_url: &str,
    pre_nav_url: Option<&str>,
    timeout_ms: u64,
) -> Result<CommitInfo, AppError> {
    const POLL_MS: u64 = 150;

    let timeout = Duration::from_millis(timeout_ms);
    let poll = Duration::from_millis(POLL_MS);
    let started = Instant::now();

    // Same-URL navigate short-circuit (dogfood-49 #1):
    //
    // If the caller asked to navigate to the URL the tab is already on, the
    // URL-change guard in the wait JS would never become true and we would
    // time out at `cli.timeout`.  In that case, drop the URL guard and let
    // the steady-state `readyState === 'complete'` reading satisfy the wait
    // immediately.  This makes `navigate <currentUrl>` a no-op that returns
    // straight away.  Callers that want a forced refresh should use
    // `ff-rdp reload`, which has its own commit-wait path.
    let same_url = pre_nav_url.is_some_and(|p| urls_match_ignore_hash(p, requested_url));

    // The JS snippet returns a sentinel-prefixed JSON when the condition is met:
    // { ready: true, url: "<current>", readyState: "<state>" }
    // Returns null while the condition is not yet satisfied.
    //
    // When `pre_nav_url` is provided AND it differs from `requested_url`,
    // also require that the live URL differs from `pre_nav_url` — otherwise
    // the old docshell's `readyState === 'complete'` can satisfy the check
    // before the new document is installed.
    let pre_json = if same_url {
        "null".to_owned()
    } else {
        serde_json::to_string(&pre_nav_url).unwrap_or_else(|_| "null".to_owned())
    };
    let js = format!(
        r"(function() {{
  var rs = document.readyState;
  if (rs !== 'interactive' && rs !== 'complete') return null;
  var pre = {pre_json};
  if (pre && window.location.href === pre) return null;
  return '__FF_RDP_JSON__' + JSON.stringify({{ready: true, url: window.location.href, readyState: rs}});
}})()"
    );
    let js = js.as_str();

    loop {
        // Re-resolve actors to get the current docshell's console actor.
        let tab_actor = ctx.target_tab_actor().clone();
        let fresh_target =
            TabActor::get_target(ctx.transport_mut(), &tab_actor).map_err(AppError::from)?;
        let console_actor = fresh_target.console_actor;

        let eval = WebConsoleActor::evaluate_js_async(ctx.transport_mut(), &console_actor, js)
            .map_err(AppError::from)?;

        // Ignore exceptions — the new docshell may not be fully initialised yet.
        if eval.exception.is_none() {
            use super::js_helpers::resolve_result;
            use serde_json::Value;

            if let Ok(resolved) = resolve_result(ctx, &eval.result)
                && let Value::Object(ref map) = resolved
                && map.get("ready").and_then(Value::as_bool) == Some(true)
            {
                let committed_url = map
                    .get("url")
                    .and_then(Value::as_str)
                    .unwrap_or(requested_url)
                    .to_owned();
                let ready_state = map
                    .get("readyState")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown")
                    .to_owned();
                let elapsed_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
                return Ok(CommitInfo {
                    committed_url,
                    ready_state,
                    elapsed_ms,
                });
            }
        }

        if started.elapsed() >= timeout {
            // Theme G: cross-origin race fix.
            //
            // When the commit-wait times out, do one final URL check.  If the
            // current URL matches the requested URL (scheme+host+path) — the
            // page committed but our polling window missed the transition —
            // treat this as a success rather than surfacing a confusing timeout.
            use super::js_helpers::resolve_result;
            let tab_actor = ctx.target_tab_actor().clone();
            if let Ok(fresh) = TabActor::get_target(ctx.transport_mut(), &tab_actor) {
                let ca = fresh.console_actor;
                if let Ok(ev) = WebConsoleActor::evaluate_js_async(
                    ctx.transport_mut(),
                    &ca,
                    "window.location.href",
                ) && ev.exception.is_none()
                    && let Ok(v) = resolve_result(ctx, &ev.result)
                    && let Some(landed) = v.as_str()
                    && urls_match_scheme_host_path(landed, requested_url)
                {
                    let elapsed_ms =
                        u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
                    return Ok(CommitInfo {
                        committed_url: landed.to_owned(),
                        ready_state: "unknown".to_owned(),
                        elapsed_ms,
                    });
                }
            }

            return Err(AppError::Timeout(format!(
                "navigate: page did not commit within {timeout_ms}ms — use --no-wait to skip commit check or increase --timeout"
            )));
        }

        std::thread::sleep(poll);
    }
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

/// Navigate to `url` and return the result value without printing.
///
/// Called by the script runner, which handles its own NDJSON output.
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

    // Capture the pre-navigation URL before dispatching `navigateTo` so
    // `wait_for_commit` can distinguish a stale "complete" reading from the
    // old document from a real commit on the new one.
    let pre_nav_url = capture_current_url(&mut ctx);

    WindowGlobalTarget::navigate_to(ctx.transport_mut(), &target_actor, url)
        .map_err(AppError::from)?;

    // Default: block until the new document commits (URL changes + readyState interactive/complete).
    // --no-wait skips this and returns immediately (old fire-and-forget behaviour).
    let commit_info = if wait_opts.no_wait {
        None
    } else {
        Some(wait_for_commit(
            &mut ctx,
            url,
            pre_nav_url.as_deref(),
            cli.timeout,
        )?)
    };

    // Theme K: invalidate the cached consoleActor after any navigate so the
    // next `eval` call fetches a fresh actor bound to the new docshell.
    refresh_console_actor(&mut ctx);

    // Theme F: detect about:neterror after commit.
    if let Some(ref ci) = commit_info
        && is_neterror_url(&ci.committed_url)
    {
        let (raw_code, error_type) = classify_neterror(&ci.committed_url)
            .unwrap_or_else(|| ("unknown", "unknown".to_owned()));
        return Err(AppError::User(format!(
            "navigate: DNS/network error navigating to '{url}' — {error_type} (Firefox error: {raw_code})\n\
             landed_url: {landed}\n\
             hint: check the URL, DNS, or network connectivity",
            landed = ci.committed_url,
        )));
    }

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

    // Total wall-clock budget for the whole `--with-network` flow.  We
    // subtract elapsed time when computing the wait-for-commit deadline so
    // total time stays close to `cli.timeout` rather than `cli.timeout * 2+`.
    let nav_started = Instant::now();
    let total_budget = Duration::from_millis(cli.timeout);

    // Capture pre-nav URL before dispatching the navigate so `wait_for_commit`
    // can distinguish a stale "complete" reading on the old document from a
    // real commit on the new one.
    let pre_nav_url = capture_current_url(&mut ctx);

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
            if let Err(e) =
                crate::daemon::client::store_network_events(ctx.transport_mut(), &items, Some(url))
                && cli.is_verbose()
            {
                eprintln!(
                    "warning: navigate --with-network: could not store events in daemon buffer: {e:#}"
                );
            }
        }

        // After network drain, optionally wait for commit (no-wait skips).
        let commit_info = if wait_opts.no_wait {
            None
        } else {
            // Use whatever is left of the total budget (minimum 2 s) so the
            // overall navigate wall-clock time stays close to `cli.timeout`.
            let remaining = total_budget.saturating_sub(nav_started.elapsed());
            let remaining_ms = u64::try_from(remaining.as_millis())
                .unwrap_or(u64::MAX)
                .max(2000);
            wait_for_commit(&mut ctx, url, pre_nav_url.as_deref(), remaining_ms).ok()
        };

        // Theme K: refresh consoleActor after navigate.
        refresh_console_actor(&mut ctx);

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

    // After network drain, optionally wait for commit (no-wait skips).
    let commit_info = if wait_opts.no_wait {
        None
    } else {
        let remaining = total_budget.saturating_sub(nav_started.elapsed());
        let remaining_ms = u64::try_from(remaining.as_millis())
            .unwrap_or(u64::MAX)
            .max(2000);
        wait_for_commit(&mut ctx, url, pre_nav_url.as_deref(), remaining_ms).ok()
    };

    // Theme K: refresh consoleActor after navigate.
    refresh_console_actor(&mut ctx);

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
        }
    }

    #[test]
    fn wait_after_nav_no_condition_returns_none() {
        let opts = default_wait_opts();
        assert!(!opts.has_condition());
    }

    // -----------------------------------------------------------------------
    // urls_match_ignore_hash — same-URL navigate short-circuit (dogfood-49 #1)
    // -----------------------------------------------------------------------

    #[test]
    fn urls_match_identical() {
        assert!(urls_match_ignore_hash(
            "https://example.com/path",
            "https://example.com/path"
        ));
    }

    #[test]
    fn urls_match_trailing_slash_only_on_one() {
        assert!(urls_match_ignore_hash(
            "https://example.com/",
            "https://example.com"
        ));
        assert!(urls_match_ignore_hash(
            "https://example.com/path/",
            "https://example.com/path"
        ));
    }

    #[test]
    fn urls_match_hash_only_on_one() {
        assert!(urls_match_ignore_hash(
            "https://example.com/path#section",
            "https://example.com/path"
        ));
        assert!(urls_match_ignore_hash(
            "https://example.com/path#a",
            "https://example.com/path#b"
        ));
    }

    #[test]
    fn urls_match_trailing_slash_and_hash() {
        assert!(urls_match_ignore_hash(
            "https://example.com/path/#section",
            "https://example.com/path"
        ));
    }

    #[test]
    fn urls_do_not_match_different_paths() {
        assert!(!urls_match_ignore_hash(
            "https://example.com/a",
            "https://example.com/b"
        ));
    }

    #[test]
    fn urls_do_not_match_different_queries() {
        // Query strings are NOT normalised — different ?p=… is different page.
        assert!(!urls_match_ignore_hash(
            "https://news.ycombinator.com/?p=2",
            "https://news.ycombinator.com/?p=3"
        ));
    }

    #[test]
    fn urls_do_not_match_different_schemes() {
        assert!(!urls_match_ignore_hash(
            "http://example.com/",
            "https://example.com/"
        ));
    }

    #[test]
    fn urls_do_not_match_different_hosts() {
        assert!(!urls_match_ignore_hash(
            "https://a.example.com/",
            "https://b.example.com/"
        ));
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
    // Theme F: neterror detection
    // -----------------------------------------------------------------------

    #[test]
    fn classify_neterror_dns_not_found() {
        let url = "about:neterror?e=dnsNotFound&u=https%3A//bad.invalid/";
        let (raw, typed) = classify_neterror(url).unwrap();
        assert_eq!(raw, "dnsNotFound");
        assert_eq!(typed, "dns_not_found");
    }

    #[test]
    fn classify_neterror_connection_failure() {
        let url = "about:neterror?e=connectionFailure&u=foo";
        let (_, typed) = classify_neterror(url).unwrap();
        assert_eq!(typed, "connection_failed");
    }

    #[test]
    fn classify_neterror_unknown_code_passthrough() {
        let url = "about:neterror?e=someNewFirefoxCode&u=foo";
        let (raw, typed) = classify_neterror(url).unwrap();
        assert_eq!(raw, typed); // unknown codes are returned as-is
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
}
