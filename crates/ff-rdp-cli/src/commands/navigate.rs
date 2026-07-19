use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use ff_rdp_core::{
    Grip, NavCause, RdpTransport, Resource, ResourceCommand, ResourceType, RootActor, TabActor,
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
    WaitForPredicate, escape_selector, eval_or_bail, poll_js_condition, wait_for_predicates,
};
use super::network_events::{
    build_network_entries, drain_network_events_timed, drain_network_from_daemon, merge_updates,
    serialize_network_resources_for_buffer,
};
use super::url_validation::validate_url_with_opts;

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

/// Strategy for waiting for navigation readiness.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, clap::ValueEnum)]
#[clap(rename_all = "lowercase")]
pub enum WaitStrategy {
    /// Wait for Firefox document-event resources (dom-complete).
    Events,
    /// Poll `document.readyState == "complete"` until timeout.
    Readystate,
    /// Wait on document-event resources while interleaving a lightweight
    /// `document.readyState` probe; return as soon as either reports the page
    /// is complete, then fall back to a dedicated readystate poll only if the
    /// events phase times out. Default. Avoids the FF152 case where a page has
    /// loaded but `dom-complete` never fires, burning the whole events budget.
    #[default]
    Both,
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
    /// Strategy for waiting for navigation readiness (default: `Both`).
    pub wait_strategy: WaitStrategy,
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

/// Configuration for the interleaved `document.readyState` fast-path used by the
/// `Both` wait strategy (iter-122 Theme A).
///
/// On FF152 the `dom-complete` `document-event` resource may never fire for a
/// page that has, in fact, finished loading — so a naive event-only wait burns
/// the whole events budget (~7 s) before the readystate fallback ever runs.
/// When this config is present, [`wait_for_doc_complete`] interleaves a
/// lightweight `document.readyState === 'complete'` probe (guarded by the same
/// `navigationStart > pre_epoch` freshness check as the pure readystate path,
/// iter-92) into its drain loop, and returns as soon as the page reports
/// `complete` — without waiting out the events budget.
struct ReadyStateProbe<'a> {
    /// Console actor bound to the navigating docshell, used to evaluate JS.
    console_actor: &'a ff_rdp_core::ActorId,
    /// `performance.timing.navigationStart` captured before `navigateTo`; a
    /// `complete` reading whose `navigationStart` is not fresher than this is
    /// stale (belongs to the prior page) and is ignored.
    pre_epoch: f64,
    /// Do not probe until this instant, giving the (faster, richer)
    /// `dom-complete` event a head start on pages that do fire it promptly.
    first_probe_at: Instant,
    /// Minimum spacing between readystate probes so events keep priority.
    probe_interval: Duration,
}

/// Evaluate `document.readyState === 'complete'` (with the `navigationStart`
/// freshness guard) on `console_actor`, returning `true` only when the *current*
/// document has finished loading.
///
/// A transport-level `recv` timeout or any eval error is treated as "not ready
/// yet" (`false`) rather than a hard error — the caller keeps waiting on the
/// events stream. The probe deliberately swallows these so a flaky mid-load
/// eval never aborts the navigation.
fn probe_readystate_complete(
    transport: &mut RdpTransport,
    console_actor: &ff_rdp_core::ActorId,
    pre_epoch: f64,
) -> bool {
    let condition = format!(
        "document.readyState === 'complete' && \
         performance.timing.navigationStart > {pre_epoch}"
    );
    match ff_rdp_core::WebConsoleActor::evaluate_js_async(transport, console_actor, &condition) {
        Ok(result) if result.exception.is_none() => super::js_helpers::is_truthy(&result.result),
        _ => false,
    }
}

/// Resolve `window.location.href` via `console_actor`, returning an empty string
/// on any error. Used as the URL source for both the readystate fast-path and as
/// a fallback when a committing `document-event` carries no `url` (iter-122
/// Theme B — avoids emitting `about:blank` for SPAs that never fire
/// `dom-loading` with a URL).
fn eval_location_href(
    transport: &mut RdpTransport,
    console_actor: &ff_rdp_core::ActorId,
) -> String {
    match ff_rdp_core::WebConsoleActor::evaluate_js_async(
        transport,
        console_actor,
        "window.location.href",
    ) {
        Ok(result) => match result.result {
            Grip::Value(serde_json::Value::String(s)) => s,
            _ => String::new(),
        },
        Err(_) => String::new(),
    }
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
/// Wait for the navigation to reach `wait_level` by pumping the transport and
/// dispatching received events through the bus.
///
/// # Lock discipline
///
/// The `bus_arc` mutex is acquired **per dispatch operation only** — it is
/// never held across the `transport.recv()` call (which may block up to
/// `poll_interval`).  This prevents a deadlock where another thread tries to
/// acquire the same mutex while this call is waiting for Firefox.
fn wait_for_doc_complete(
    transport: &mut RdpTransport,
    bus_arc: &Arc<Mutex<ResourceCommand>>,
    rx: &std::sync::mpsc::Receiver<std::sync::Arc<Resource>>,
    timeout_ms: u64,
    wait_level: WaitLevel,
    nav_start: Instant,
    probe: Option<&ReadyStateProbe<'_>>,
) -> Result<CommitInfo, AppError> {
    let deadline = Instant::now() + Duration::from_millis(timeout_ms);

    // Use a short socket read timeout so we can check the deadline
    // even when the server is quiet.
    let poll_interval = Duration::from_millis(100);
    transport
        .set_read_timeout(Some(poll_interval))
        .map_err(|e| AppError::from(anyhow::anyhow!("set_read_timeout: {e:#}")))?;

    let mut commit_url: Option<String> = None;
    // Track whether we've seen dom-interactive so Loading/Interactive can return early.
    let mut interactive_url: Option<String> = None;
    // Next instant at which the interleaved readystate probe (Theme A) may run.
    let mut next_probe_at = probe.map(|p| p.first_probe_at);

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
                                u64::try_from(nav_start.elapsed().as_millis()).unwrap_or(u64::MAX);
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
                                u64::try_from(nav_start.elapsed().as_millis()).unwrap_or(u64::MAX);
                            // Theme B: if the event carried no URL, resolve the
                            // real URL via location.href rather than emitting an
                            // empty string (rendered as about:blank).
                            let committed_url = if eff_url.is_empty() {
                                probe.map_or_else(String::new, |p| {
                                    eval_location_href(transport, p.console_actor)
                                })
                            } else {
                                eff_url
                            };
                            return Ok(CommitInfo {
                                committed_url,
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
                            u64::try_from(nav_start.elapsed().as_millis()).unwrap_or(u64::MAX);
                        let committed = interactive_url
                            .take()
                            .or_else(|| commit_url.take())
                            .unwrap_or_default();
                        // Theme B: an empty/blank URL (SPA that never fired a
                        // dom-loading with a URL) must be resolved from the live
                        // document rather than surfaced as about:blank.
                        let committed = if committed.is_empty() {
                            probe.map_or(committed, |p| {
                                eval_location_href(transport, p.console_actor)
                            })
                        } else {
                            committed
                        };
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

        // Theme A fast-path: interleave a lightweight readystate probe so a page
        // that is already `complete` returns without waiting out the events
        // budget for a `dom-complete` event that may never fire on FF152. Only
        // active for the `Both` strategy (probe is None for `Events`).  Runs at
        // most once per `probe_interval`, after `first_probe_at`, so the richer
        // event stream keeps priority on pages that do fire dom-complete.
        if let (Some(p), Some(when)) = (probe, next_probe_at)
            && Instant::now() >= when
        {
            if probe_readystate_complete(transport, p.console_actor, p.pre_epoch) {
                let elapsed_ms = u64::try_from(nav_start.elapsed().as_millis()).unwrap_or(u64::MAX);
                let committed = eval_location_href(transport, p.console_actor);
                return Ok(CommitInfo {
                    committed_url: committed,
                    ready_state: "complete".to_owned(),
                    elapsed_ms,
                });
            }
            // Re-arm the probe timer regardless of the outcome above.
            next_probe_at = Some(Instant::now() + p.probe_interval);
        }

        // Pump the transport — will block up to `poll_interval` then return
        // Timeout, which we treat as idle (keep looping).
        // The lock is acquired ONLY for dispatch_event (not held during recv).
        match transport.recv() {
            Ok(msg) => {
                // Acquire the lock for dispatch only; release immediately after.
                // SAFETY invariant: no panic path inside dispatch_event can
                // leave the guard dropped while the bus is in a bad state —
                // dispatch_event only pushes to channels and prunes dead ones.
                bus_arc
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .dispatch_event(&msg);
            }
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

/// Returns `true` when the navigation captured by `current_nav_start` is fresh
/// (i.e., the page loaded *after* the `pre_epoch` snapshot taken before the
/// navigate dispatch).
///
/// A pre-existing completed page (same URL reloaded, or stale state from a
/// prior session) will have `current_nav_start <= pre_epoch` and returns
/// `false`, keeping the wait loop alive until a genuine new load completes.
///
/// # Unit-test target
///
/// `unit_navigate_rejects_stale_ready_state` exercises this function directly
/// so the freshness logic can be verified without a live Firefox connection.
#[cfg(test)]
fn is_readystate_fresh(current_nav_start: f64, pre_epoch: f64) -> bool {
    current_nav_start > pre_epoch
}

/// Poll `document.readyState == "complete"` until the deadline, returning a
/// `CommitInfo` when the condition is met.
///
/// `pre_epoch` is the value of `performance.timing.navigationStart` captured
/// *before* the navigate was dispatched.  Any `readyState == complete` reading
/// whose `navigationStart` is not fresher than `pre_epoch` is treated as stale
/// and the poll continues.  This prevents the second navigate to the same tab
/// from short-circuiting on the pre-existing completed state.
///
/// Used by the `readystate` and `both` wait strategies as a fallback when the
/// document-event resource stream doesn't fire within the timeout budget.
fn wait_for_readystate_complete(
    ctx: &mut super::connect_tab::ConnectedTab,
    timeout_ms: u64,
    pre_epoch: f64,
    nav_start: Instant,
) -> Result<CommitInfo, AppError> {
    use crate::commands::js_helpers::poll_js_condition;

    let console_actor = ctx.target.console_actor.clone();

    // Combine readyState check with navigationStart freshness guard so that a
    // pre-existing "complete" state from the prior page load is rejected.
    // `performance.timing.navigationStart` is milliseconds since the Unix epoch
    // (matching the value captured before navigate dispatch).
    let condition = format!(
        "document.readyState === 'complete' && \
         performance.timing.navigationStart > {pre_epoch}"
    );

    // The poll's own elapsed only covers the readystate phase; discard it in
    // favour of `nav_start` so `CommitInfo.elapsed_ms` reflects total
    // wall-clock across the events→readystate fallback (iter-122 Theme B).
    poll_js_condition(
        ctx,
        &console_actor,
        &condition,
        timeout_ms,
        "navigate readystate: JS evaluation error",
        &format!(
            "navigate: document.readyState did not reach 'complete' (with fresh navigation) \
             within {timeout_ms}ms — use --no-wait to skip or increase --timeout"
        ),
    )?;

    let url = {
        let console_actor = ctx.target.console_actor.clone();
        match super::js_helpers::eval_or_bail(
            ctx,
            &console_actor,
            "window.location.href",
            "navigate readystate: url eval",
        ) {
            Ok(result) => match result.result {
                ff_rdp_core::Grip::Value(serde_json::Value::String(s)) => s,
                _ => String::new(),
            },
            Err(_) => String::new(),
        }
    };

    let elapsed_ms = u64::try_from(nav_start.elapsed().as_millis()).unwrap_or(u64::MAX);
    Ok(CommitInfo {
        committed_url: url,
        ready_state: "complete".to_owned(),
        elapsed_ms,
    })
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

/// Split a total wait `timeout_ms` into `(reserved_ms, events_budget)` for the
/// `Both` wait strategy: `reserved_ms` goes to the readystate fallback,
/// `events_budget` to the events wait.
///
/// `reserved_ms` is 30% of the total, floored at 1 000 ms so the fallback
/// always gets a meaningful window, but capped at half the total so the
/// events wait is never starved down to a 1 ms sliver for small timeouts.
/// Saturating arithmetic keeps degenerate inputs (`timeout_ms` of 0 or 1)
/// from panicking.
fn split_wait_budget(timeout_ms: u64) -> (u64, u64) {
    let reserved_ms = (timeout_ms * 30 / 100).max(1000).min(timeout_ms / 2);
    let events_budget = timeout_ms.saturating_sub(reserved_ms);
    (reserved_ms, events_budget)
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

/// Map a commit-wait [`AppError::Timeout`] to a neterror-shaped
/// [`AppError::Navigation`] when the tab actually landed on `about:neterror`
/// (iter-106 Theme B).
///
/// The plain `navigate` path (`run_core`) waits for `dom-complete` / a fresh
/// `readyState === 'complete'`.  On a DNS-resolution failure Firefox loads
/// `about:neterror` instead — that document never reaches the awaited state, so
/// the wait exhausts its budget and returns a generic
/// `readyState did not reach 'complete'` [`AppError::Timeout`] (exit code 124).
/// That masks the real cause: the domain does not resolve.
///
/// `run_with_network` already calls [`check_real_tab_url_for_neterror`] after
/// its drain settles; `run_core` did not, so a bad-DNS `navigate` surfaced a
/// timeout rather than a `nav_dns_fail`.  This helper closes that gap: on a
/// `Timeout`, it queries `listTabs` for an `about:neterror` landing and, if
/// found, returns the classified [`AppError::Navigation`] (rendered as e.g.
/// "DNS resolution failed", `error_type: "nav_dns_fail"`, exit code 7).  Any
/// non-timeout error, or a timeout with no neterror landing, passes through
/// unchanged.
fn reclassify_timeout_as_neterror(
    ctx: &mut super::connect_tab::ConnectedTab,
    requested_url: &str,
    result: Result<CommitInfo, AppError>,
) -> Result<CommitInfo, AppError> {
    match result {
        Ok(ci) => Ok(ci),
        Err(AppError::Timeout(msg)) => {
            // Refresh the console/tab fronts so `listTabs` sees the committed
            // about:neterror document rather than a stale target.
            refresh_console_actor(ctx);
            match check_real_tab_url_for_neterror(ctx, requested_url) {
                Some(nav_err) => Err(nav_err),
                None => Err(AppError::Timeout(msg)),
            }
        }
        Err(other) => Err(other),
    }
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
    validate_url_with_opts(url, cli.allow_file_urls, cli.allow_unsafe_urls)?;
    let mut ctx = connect_and_get_target(cli)?;
    let target_actor = ctx.target.actor.clone();
    let tab_actor = ctx.target_tab_actor().clone();

    // Get the watcher actor and subscribe to document-event resources before
    // sending navigateTo so we don't miss any events that arrive immediately
    // after the navigate (Firefox may dispatch dom-loading very quickly).
    let watcher_actor =
        TabActor::get_watcher(ctx.transport_mut(), &tab_actor).map_err(AppError::from)?;

    // iter-92 Theme B: capture navigationStart *before* dispatching navigateTo
    // so the readystate-poll path can reject a pre-existing "complete" state
    // that belongs to the prior page load (the stale-dom-complete regression).
    //
    // This is a best-effort capture; if eval fails (e.g. the page is still
    // loading when we connect), we fall back to 0.0 which effectively disables
    // the freshness guard rather than blocking the navigate.
    let pre_nav_epoch: f64 = if wait_opts.no_wait {
        0.0 // freshness guard not needed for --no-wait
    } else {
        let console_actor = ctx.target.console_actor.clone();
        match eval_or_bail(
            &mut ctx,
            &console_actor,
            "performance.timing.navigationStart",
            "navigate: pre-nav epoch eval",
        ) {
            Ok(result) => match result.result {
                Grip::Value(serde_json::Value::Number(ref n)) => n.as_f64().unwrap_or(0.0),
                other => {
                    tracing::warn!(
                        ?other,
                        "navigate: pre-nav epoch eval returned non-numeric grip; \
                         freshness guard disabled"
                    );
                    0.0
                }
            },
            Err(e) => {
                tracing::warn!(
                    error = %e,
                    "navigate: pre-nav epoch eval failed; freshness guard disabled"
                );
                0.0
            }
        }
    };

    let commit_info = if wait_opts.no_wait {
        // --no-wait: send navigateTo via the standard actor_request (response
        // is the navigateTo ack) and return immediately, no bus needed.
        WindowGlobalTarget::navigate_to(ctx.transport_mut(), &target_actor, url)
            .map_err(AppError::from)?;
        None
    } else if wait_opts.wait_strategy == WaitStrategy::Readystate {
        // --wait-strategy readystate: skip the document-event bus entirely.
        // Sending navigateTo + immediately polling document.readyState avoids
        // the full event-wait timeout cost that the default Events path pays.
        let nav_start = Instant::now();
        WindowGlobalTarget::navigate_to(ctx.transport_mut(), &target_actor, url)
            .map_err(AppError::from)?;
        // Theme K: refresh console actor so eval hits the new document.
        refresh_console_actor(&mut ctx);
        let rs_result =
            wait_for_readystate_complete(&mut ctx, cli.timeout, pre_nav_epoch, nav_start);
        let ci = reclassify_timeout_as_neterror(&mut ctx, url, rs_result)?;
        Some(ci)
    } else {
        // Events or Both strategy: subscribe to document-event resources before
        // sending navigateTo so we don't miss events that arrive immediately.
        //
        // Engage the watcher's frame-target subscription BEFORE subscribing
        // to document-event resources.  Per the Firefox watcher contract
        // (devtools/shared/specs/watcher.js + kb/rdp/actors/watcher.md), a
        // WatcherActor delivers nothing until BOTH `watchTargets("frame")` and
        // `watchResources([...])` have been issued — so without this call the
        // document-event stream stays empty and `wait_for_doc_complete` times
        // out even on pages that load successfully (iter-79 Theme A).
        WatcherActor::watch_targets(ctx.transport_mut(), &watcher_actor, "frame")
            .map_err(AppError::from)?;

        // Obtain (or create) the ResourceCommand bus via the session so it can
        // be reused by other command helpers without constructing a new bus each
        // time.  The Arc clone detaches ownership from `ctx` so we can still
        // call `ctx.transport_mut()` below without a double-borrow.
        let bus_arc = ctx.get_or_init_resource_command(watcher_actor.clone());

        // Lock per-operation: subscribe, wait, gc, unsubscribe.
        // The lock is released between each operation so other threads can
        // acquire it without blocking on the full navigation wait time.
        let (sub_id, rx) = bus_arc
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .subscribe(ctx.transport_mut(), &[ResourceType::DocumentEvent])
            .map_err(|e| AppError::from(anyhow::anyhow!("document-event subscribe: {e:#}")))?;

        // Record the wall-clock instant before sending navigateTo so we can
        // compute the remaining budget for the Both readystate fallback.
        let nav_start = Instant::now();

        // Send navigateTo raw (not via actor_request) so we don't lose
        // resources-available-array events that arrive before the ack.
        ctx.transport_mut()
            .send(&json!({
                "to": target_actor.as_ref(),
                "type": "navigateTo",
                "url": url,
            }))
            .map_err(AppError::from)?;

        // Theme C (iter-84): when the `Both` strategy is active, split the
        // timeout budget so the readystate fallback is guaranteed at least 30%
        // of the total.  Without this split, `wait_for_doc_complete` can
        // consume the entire budget and leave `remaining == 0` for the
        // readystate pass — which is the bug that caused `navigate
        // https://example.com` to always time out on real cross-origin pages.
        //
        // For the `Events`-only strategy, pass the full budget so behaviour
        // is unchanged for users who explicitly opted in to event-only waiting.
        //
        // `split_wait_budget` caps the reserve at half the total (not the full
        // total) so short `--timeout` values, like the 1000 ms used by e2e
        // tests, still leave the events wait a real window instead of
        // collapsing it to 1 ms — see the regression tests next to that
        // function.
        //
        // iter-122 Theme A re-tuning: the `Both` events phase now *also* probes
        // `document.readyState` in-loop (see `ReadyStateProbe`), so a page that
        // is `complete` returns from `wait_for_doc_complete` itself without the
        // dedicated fallback ever running. The 30% reserve is kept only as a
        // safety net for the case where the interleaved console eval is entirely
        // unavailable (e.g. every probe times out) — the fast path, not the
        // reserve, is what now saves the ~7 s on FF152.
        let events_budget = if wait_opts.wait_strategy == WaitStrategy::Both {
            split_wait_budget(cli.timeout).1
        } else {
            cli.timeout
        };

        // Theme A: build the interleaved readystate probe for the `Both`
        // strategy only. `Events` keeps its pure event-only semantics (probe
        // stays None) so users who opted into event-only waiting are unaffected.
        let probe_console_actor = ctx.target.console_actor.clone();
        let readystate_probe = if wait_opts.wait_strategy == WaitStrategy::Both {
            Some(ReadyStateProbe {
                console_actor: &probe_console_actor,
                pre_epoch: pre_nav_epoch,
                // Give dom-complete a 300 ms head start on pages that fire it
                // promptly (comparis fired it in ~0.69 s), then probe every
                // 250 ms so events keep priority but a stuck page is caught
                // quickly rather than after the full events budget.
                first_probe_at: nav_start + Duration::from_millis(300),
                probe_interval: Duration::from_millis(250),
            })
        } else {
            None
        };

        // wait_for_doc_complete acquires the lock only during dispatch_event,
        // not across the full recv() wait — see its lock-discipline doc-comment.
        let event_result = wait_for_doc_complete(
            ctx.transport_mut(),
            &bus_arc,
            &rx,
            events_budget,
            wait_opts.wait_level,
            nav_start,
            readystate_probe.as_ref(),
        );

        // Flush any pending `unwatchResources` from dead-channel pruning that
        // occurred inside `wait_for_doc_complete` before we unsubscribe.
        let _ = bus_arc
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .gc(ctx.transport_mut());

        // Unsubscribe regardless of outcome so Firefox cleans up server state.
        let _ = bus_arc
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .unsubscribe(ctx.transport_mut(), sub_id);

        // Pair the prelude's `watchTargets("frame")` with `unwatchTargets`
        // (oneway, no reply) so the server-side frame-target subscription is
        // cleared.  Best-effort like the neighbouring `unsubscribe` call —
        // we don't want a teardown error to mask the navigation result.
        let _ =
            WatcherActor::unwatch_targets(ctx.transport_mut(), &watcher_actor, Some("frame"), None);

        // Restore the original timeout so subsequent RDP round-trips (e.g.
        // wait-text / wait-selector polling) use the configured timeout.
        restore_timeout(ctx.transport_mut(), cli.timeout);

        // Apply wait_strategy.  `Readystate` was handled by the early branch
        // above and never reaches this code.  Only `Events` and `Both` run here.
        //
        // For `Both`, if events timed out, fall back to readystate polling with
        // only the REMAINING budget so we don't re-pay the full timeout.
        let result = match event_result {
            r @ Ok(_) => r,
            Err(e) if wait_opts.wait_strategy != WaitStrategy::Both => Err(e),
            Err(AppError::Timeout(_)) => {
                // Events timed out — give readystate the reserved 30% slice,
                // capped to whatever is actually left of cli.timeout so the
                // total wall time stays inside the user's budget.
                refresh_console_actor(&mut ctx);
                let elapsed_ms =
                    u64::try_from(nav_start.elapsed().as_millis()).unwrap_or(cli.timeout);
                let remaining = cli.timeout.saturating_sub(elapsed_ms);
                if remaining == 0 {
                    Err(AppError::Timeout(
                        "navigate: no remaining budget for readystate fallback".to_string(),
                    ))
                } else {
                    wait_for_readystate_complete(&mut ctx, remaining, pre_nav_epoch, nav_start)
                }
            }
            Err(e) => Err(e),
        };

        Some(reclassify_timeout_as_neterror(&mut ctx, url, result)?)
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
    validate_url_with_opts(url, cli.allow_file_urls, cli.allow_unsafe_urls)?;
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
            if let Err(e) =
                crate::daemon::client::store_network_events(ctx.transport_mut(), url, &items)
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

    // Pair the `watchTargets("frame")` prelude with `unwatchTargets` so the
    // server-side frame-target subscription is cleared (oneway, best-effort).
    let _ = WatcherActor::unwatch_targets(ctx.transport_mut(), &watcher_actor, Some("frame"), None);

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
            wait_strategy: WaitStrategy::Events,
        }
    }

    // -----------------------------------------------------------------------
    // iter-92 Theme B: unit_navigate_rejects_stale_ready_state
    //
    // Verifies the freshness helper: a `readyState == complete` reading whose
    // navigationStart predates the pre-epoch (from before the navigate dispatch)
    // must be treated as stale so the poll keeps waiting.
    // -----------------------------------------------------------------------

    /// `unit_navigate_rejects_stale_ready_state`:
    ///
    /// Feed the `is_readystate_fresh` helper a `navigationStart` that is equal
    /// to or older than the pre-epoch and assert it returns `false`.  Then feed
    /// a fresh `navigationStart` and assert `true`.
    #[test]
    fn unit_navigate_rejects_stale_ready_state() {
        let pre_epoch = 1_000_000.0_f64;

        // Stale: navigationStart == pre_epoch (same load, not a new nav).
        assert!(
            !is_readystate_fresh(pre_epoch, pre_epoch),
            "navigationStart equal to pre_epoch must be stale"
        );

        // Stale: navigationStart < pre_epoch.
        assert!(
            !is_readystate_fresh(pre_epoch - 100.0, pre_epoch),
            "navigationStart before pre_epoch must be stale"
        );

        // Fresh: navigationStart clearly after pre_epoch.
        assert!(
            is_readystate_fresh(pre_epoch + 1.0, pre_epoch),
            "navigationStart 1 ms after pre_epoch must be fresh"
        );
        assert!(
            is_readystate_fresh(pre_epoch + 5000.0, pre_epoch),
            "navigationStart 5 s after pre_epoch must be fresh"
        );
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

    /// iter-83 Theme C: assert the default `WaitStrategy` is `Both` so the
    /// CLI's documented default (events first, readystate fallback) is exercised
    /// when callers omit `--wait-strategy`.
    #[test]
    fn wait_strategy_default_is_both() {
        assert_eq!(WaitStrategy::default(), WaitStrategy::Both);
    }

    /// iter-85 Theme C: the `Both` budget-split formula must always reserve at
    /// least 1 000 ms for the readystate fallback — even at the default 10 s
    /// timeout — so the fallback has a meaningful window instead of 0 ms
    /// (the bug that caused example.com to always time out). Revised for the
    /// Ubuntu CI regression: the reserve is also capped at half the total, so
    /// the events wait keeps a real window at small `--timeout` values instead
    /// of collapsing to 1 ms — see `split_wait_budget_exact_values` below.
    #[test]
    fn navigate_both_strategy_reserves_readystate_budget() {
        let timeout_ms: u64 = 10_000; // default cli.timeout
        let (reserved_ms, events_budget) = split_wait_budget(timeout_ms);
        // Reserved slice must be at least 1 s.
        assert!(
            reserved_ms >= 1000,
            "readystate reserve must be ≥ 1000 ms; got {reserved_ms}"
        );
        // Events budget must get at least half the total timeout.
        assert!(
            events_budget >= timeout_ms / 2,
            "events budget must be ≥ half the timeout; got events_budget={events_budget}, \
             timeout={timeout_ms}"
        );
        // The two slices must not exceed the total budget.
        assert!(
            events_budget + reserved_ms <= timeout_ms,
            "events_budget ({events_budget}) + reserved_ms ({reserved_ms}) \
             exceeds timeout ({timeout_ms})"
        );
    }

    /// Regression test for the Ubuntu CI failure: at `--timeout 1000` (used by
    /// the e2e tests `navigate_outputs_json_envelope` and
    /// `navigate_with_jq_extracts_url`), the old formula reserved the *entire*
    /// 1000 ms for the readystate fallback, leaving `events_budget == 1` and
    /// causing the events wait to time out instantly ("timed out after 0ms
    /// (phase: recv)"). Pin the exact split across the timeout range,
    /// including tiny inputs that must not panic.
    #[test]
    fn split_wait_budget_exact_values() {
        assert_eq!(split_wait_budget(1000), (500, 500));
        assert_eq!(split_wait_budget(10_000), (3000, 7000)); // unchanged at the default
        assert_eq!(split_wait_budget(2000), (1000, 1000));
        assert_eq!(split_wait_budget(10), (5, 5));
        assert_eq!(split_wait_budget(1), (0, 1));
        assert_eq!(split_wait_budget(0), (0, 0));
    }

    /// iter-83 Theme C: parsing the navigate command without `--wait-strategy`
    /// must resolve to `WaitStrategy::Both`.
    #[test]
    fn navigate_clap_default_wait_strategy_is_both() {
        use clap::Parser as _;
        let cli =
            crate::cli::args::Cli::try_parse_from(["ff-rdp", "navigate", "https://example.com/"])
                .expect("clap parse navigate");
        match cli.command {
            crate::cli::args::Command::Navigate(args) => {
                let wait_strategy = args.wait_strategy;
                assert_eq!(
                    wait_strategy,
                    WaitStrategy::Both,
                    "clap default for --wait-strategy must be Both (iter-83 Theme C)"
                );
            }
            _ => panic!("expected Navigate command variant"),
        }
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
        let bus_arc = Arc::new(Mutex::new(ResourceCommand::new(watcher_actor)));

        let started = Instant::now();
        let result = wait_for_doc_complete(
            &mut transport,
            &bus_arc,
            &rx,
            TIMEOUT_MS,
            WaitLevel::Complete,
            started,
            None,
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

    // -----------------------------------------------------------------------
    // navigate_bus_lock_released_during_wait (iter-71b AC)
    //
    // Verify that `wait_for_doc_complete` does NOT hold the bus lock across
    // `transport.recv()`.  We do this by attempting to acquire the lock from
    // a second thread while the function is blocked in recv — if the lock were
    // held the second thread would also block, causing the test to time out.
    // -----------------------------------------------------------------------

    // -----------------------------------------------------------------------
    // navigate_subscribes_before_navigateto (iter-79 Theme A AC)
    //
    // The navigate prelude must issue, in this exact order:
    //   1. watchTargets("frame")           — engages the frame-target stream
    //   2. watchResources(["document-event"]) — engages the resource stream
    //   3. navigateTo                       — triggers the navigation
    //
    // Without (1) Firefox suppresses document-event resources entirely (per
    // the watcher contract), so wait_for_doc_complete never observes the
    // events on a real page and the CLI times out.  This test pins the
    // prelude to that order by capturing outbound packets on a mock server.
    // -----------------------------------------------------------------------

    #[test]
    fn navigate_subscribes_before_navigateto() {
        use std::io::Write as _;
        use std::net::TcpListener;

        use ff_rdp_core::transport::{RdpTransport, encode_frame, recv_from};

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();

        // Mock Firefox: greeting, then accept three packets.  Reply to the
        // first two (watchTargets, watchResources) so actor_request returns;
        // the third (navigateTo) is fire-and-forget.
        let server_handle = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut writer = stream.try_clone().unwrap();
            let mut reader = std::io::BufReader::new(stream);

            let greeting = serde_json::json!({
                "from": "root",
                "applicationType": "browser",
                "traits": {}
            });
            writer
                .write_all(encode_frame(&serde_json::to_string(&greeting).unwrap()).as_bytes())
                .unwrap();

            let p1 = recv_from(&mut reader).unwrap();
            // Reply to watchTargets.
            let reply1 = serde_json::json!({
                "from": p1["to"].as_str().unwrap_or("conn0/watcher1"),
            });
            writer
                .write_all(encode_frame(&serde_json::to_string(&reply1).unwrap()).as_bytes())
                .unwrap();

            let p2 = recv_from(&mut reader).unwrap();
            // Reply to watchResources.
            let reply2 = serde_json::json!({
                "from": p2["to"].as_str().unwrap_or("conn0/watcher1"),
            });
            writer
                .write_all(encode_frame(&serde_json::to_string(&reply2).unwrap()).as_bytes())
                .unwrap();

            let p3 = recv_from(&mut reader).unwrap();
            (p1, p2, p3)
        });

        let mut transport =
            RdpTransport::connect("127.0.0.1", port, Duration::from_secs(5)).unwrap();

        let watcher_actor = ff_rdp_core::ActorId::from("conn0/watcher1");
        let target_actor = ff_rdp_core::ActorId::from("conn0/target1");

        // Drive the prelude exactly as run_core() does: watchTargets, then
        // ResourceCommand::subscribe (which sends watchResources), then a raw
        // navigateTo send.
        WatcherActor::watch_targets(&mut transport, &watcher_actor, "frame").unwrap();

        let mut bus = ResourceCommand::new(watcher_actor.clone());
        let (_sub_id, _rx) = bus
            .subscribe(&mut transport, &[ResourceType::DocumentEvent])
            .unwrap();

        transport
            .send(&json!({
                "to": target_actor.as_ref(),
                "type": "navigateTo",
                "url": "https://example.com/",
            }))
            .unwrap();

        let (p1, p2, p3) = server_handle.join().unwrap();

        assert_eq!(
            p1["type"].as_str(),
            Some("watchTargets"),
            "first packet must be watchTargets, got: {p1}"
        );
        assert_eq!(
            p1["targetType"].as_str(),
            Some("frame"),
            "watchTargets must target 'frame', got: {p1}"
        );
        assert_eq!(
            p2["type"].as_str(),
            Some("watchResources"),
            "second packet must be watchResources, got: {p2}"
        );
        let res_types = p2["resourceTypes"]
            .as_array()
            .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>())
            .unwrap_or_default();
        assert!(
            res_types.contains(&"document-event"),
            "watchResources must include 'document-event', got: {p2}"
        );
        assert_eq!(
            p3["type"].as_str(),
            Some("navigateTo"),
            "third packet must be navigateTo, got: {p3}"
        );
        assert_eq!(
            p3["url"].as_str(),
            Some("https://example.com/"),
            "navigateTo URL must match request, got: {p3}"
        );
    }

    #[test]
    fn navigate_bus_lock_released_during_wait() {
        use std::io::Write as _;
        use std::net::TcpListener;
        use std::sync::atomic::{AtomicBool, Ordering};

        use ff_rdp_core::transport::{RdpTransport, encode_frame};

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();

        // Server: send greeting, then sleep 500ms before sending anything else
        // so the transport blocks in recv for that window.
        let server_handle = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut writer = stream.try_clone().unwrap();
            let greeting =
                serde_json::json!({"from": "root", "applicationType": "browser", "traits": {}});
            let _ = writer
                .write_all(encode_frame(&serde_json::to_string(&greeting).unwrap()).as_bytes());
            std::thread::sleep(Duration::from_millis(500));
        });

        let mut transport =
            RdpTransport::connect("127.0.0.1", port, Duration::from_secs(5)).unwrap();
        // short timeout so wait_for_doc_complete times out quickly
        let (tx, rx) = std::sync::mpsc::channel::<std::sync::Arc<Resource>>();
        drop(tx); // empty channel — wait will timeout

        let watcher_actor = ff_rdp_core::ActorId::from("conn0/watcher1");
        let bus_arc = Arc::new(Mutex::new(ResourceCommand::new(watcher_actor)));
        let bus_arc_clone = Arc::clone(&bus_arc);

        // Probe: attempt to acquire the lock from a second thread while
        // wait_for_doc_complete is running. We record whether the lock was
        // acquired within a 300 ms window.
        let lock_acquired = Arc::new(AtomicBool::new(false));
        let lock_acquired_clone = Arc::clone(&lock_acquired);

        let probe_handle = std::thread::spawn(move || {
            // Give wait_for_doc_complete time to start its first recv call.
            std::thread::sleep(Duration::from_millis(60));
            // Try to lock with a generous timeout: if the lock is held across
            // recv() this will block for ~100ms (the poll_interval) or more.
            // We just try to acquire it; success means it was released.
            if bus_arc_clone.try_lock().is_ok() {
                lock_acquired_clone.store(true, Ordering::Relaxed);
            }
        });

        // Run wait_for_doc_complete with a 200ms timeout so the test finishes quickly.
        let _ = wait_for_doc_complete(
            &mut transport,
            &bus_arc,
            &rx,
            200,
            WaitLevel::Complete,
            Instant::now(),
            None,
        );

        probe_handle.join().unwrap();
        server_handle.join().unwrap();

        assert!(
            lock_acquired.load(Ordering::Relaxed),
            "navigate_bus_lock_released_during_wait: second thread could not acquire \
             the bus lock while wait_for_doc_complete was running — lock held too long"
        );
    }

    /// Answer one `evaluateJSAsync` round-trip on `writer`/`reader`, replying
    /// with the immediate `resultID` ack followed by an `evaluationResult`
    /// carrying `result_value`.  Returns the `text` the client asked to evaluate
    /// so the caller can assert on it.
    ///
    /// Shared by the iter-122 Theme A/B unit tests below.
    fn answer_one_eval(
        reader: &mut std::io::BufReader<std::net::TcpStream>,
        writer: &mut std::net::TcpStream,
        console_actor: &str,
        result_value: &serde_json::Value,
    ) -> String {
        use std::io::Write as _;

        use ff_rdp_core::transport::{encode_frame, recv_from};

        let req = recv_from(reader).unwrap();
        let text = req["text"].as_str().unwrap_or_default().to_owned();
        // Immediate ack (a reply — no `type` field) carrying the resultID.
        let ack = serde_json::json!({ "from": console_actor, "resultID": "r1" });
        writer
            .write_all(encode_frame(&serde_json::to_string(&ack).unwrap()).as_bytes())
            .unwrap();
        // The evaluationResult push event.
        let eval_result = serde_json::json!({
            "from": console_actor,
            "type": "evaluationResult",
            "resultID": "r1",
            "result": result_value,
        });
        writer
            .write_all(encode_frame(&serde_json::to_string(&eval_result).unwrap()).as_bytes())
            .unwrap();
        text
    }

    /// iter-122 Theme A: `unit_navigate_readystate_probe_short_circuits`
    ///
    /// When the events stream never fires `dom-complete` (the FF152 symptom) but
    /// the interleaved probe observes `document.readyState === 'complete'`,
    /// `wait_for_doc_complete` must return promptly — well inside the events
    /// budget — with `ready_state: "complete"` and `committed_url` resolved from
    /// `location.href` (Theme B), never an empty string.
    #[test]
    fn unit_navigate_readystate_probe_short_circuits() {
        use std::io::Write as _;
        use std::net::TcpListener;

        use ff_rdp_core::transport::{RdpTransport, encode_frame};

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let console_actor = "conn0/console1";

        // Mock Firefox: greeting, then answer two eval round-trips — the
        // readyState probe (truthy) and the follow-up location.href fetch.
        // It never sends any document-event, so only the probe can resolve.
        let server_handle = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut writer = stream.try_clone().unwrap();
            let mut reader = std::io::BufReader::new(stream);

            let greeting = serde_json::json!({
                "from": "root", "applicationType": "browser", "traits": {}
            });
            writer
                .write_all(encode_frame(&serde_json::to_string(&greeting).unwrap()).as_bytes())
                .unwrap();

            let ready_text = answer_one_eval(
                &mut reader,
                &mut writer,
                console_actor,
                &serde_json::json!(true),
            );
            let href_text = answer_one_eval(
                &mut reader,
                &mut writer,
                console_actor,
                &serde_json::json!("https://example.com/"),
            );
            (ready_text, href_text)
        });

        let mut transport =
            RdpTransport::connect("127.0.0.1", port, Duration::from_secs(5)).unwrap();

        // Empty channel — no document-event ever arrives, so the probe is the
        // only way out other than the timeout.
        let (tx, rx) = std::sync::mpsc::channel::<std::sync::Arc<Resource>>();
        drop(tx);

        let watcher_actor = ff_rdp_core::ActorId::from("conn0/watcher1");
        let bus_arc = Arc::new(Mutex::new(ResourceCommand::new(watcher_actor)));
        let console = ff_rdp_core::ActorId::from(console_actor);

        let nav_start = Instant::now();
        let probe = ReadyStateProbe {
            console_actor: &console,
            pre_epoch: 0.0,
            // Probe almost immediately so the test does not wait 300 ms.
            first_probe_at: nav_start,
            probe_interval: Duration::from_millis(50),
        };

        // Generous events budget: the probe must return long before this.
        let result = wait_for_doc_complete(
            &mut transport,
            &bus_arc,
            &rx,
            5_000,
            WaitLevel::Complete,
            nav_start,
            Some(&probe),
        );

        let (ready_text, href_text) = server_handle.join().unwrap();

        let ci = result.expect("probe should short-circuit to a CommitInfo");
        assert_eq!(ci.ready_state, "complete");
        assert_eq!(
            ci.committed_url, "https://example.com/",
            "committed_url must come from location.href, not be empty/about:blank"
        );
        assert!(
            ready_text.contains("readyState"),
            "first eval should be the readyState probe, got: {ready_text}"
        );
        assert!(
            href_text.contains("location.href"),
            "second eval should be the location.href fetch, got: {href_text}"
        );
        assert!(
            nav_start.elapsed() < Duration::from_secs(4),
            "probe must return well inside the events budget; took {:?}",
            nav_start.elapsed()
        );
    }

    /// iter-122 Theme B: `unit_navigate_dom_complete_empty_url_falls_back_to_href`
    ///
    /// When a `dom-complete` event commits with no URL (an SPA that never fired
    /// `dom-loading` with a real URL), `committed_url` must be resolved from
    /// `location.href` instead of surfacing as an empty string (about:blank).
    #[test]
    fn unit_navigate_dom_complete_empty_url_falls_back_to_href() {
        use std::io::Write as _;
        use std::net::TcpListener;

        use ff_rdp_core::transport::{RdpTransport, encode_frame};

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let console_actor = "conn0/console1";

        // Server: greeting, then a single location.href eval answer (the empty
        // dom-complete triggers exactly one href fetch).
        let server_handle = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut writer = stream.try_clone().unwrap();
            let mut reader = std::io::BufReader::new(stream);

            let greeting = serde_json::json!({
                "from": "root", "applicationType": "browser", "traits": {}
            });
            writer
                .write_all(encode_frame(&serde_json::to_string(&greeting).unwrap()).as_bytes())
                .unwrap();

            answer_one_eval(
                &mut reader,
                &mut writer,
                console_actor,
                &serde_json::json!("https://spa.example/app"),
            )
        });

        let mut transport =
            RdpTransport::connect("127.0.0.1", port, Duration::from_secs(5)).unwrap();

        // Pre-load a dom-loading (empty url) + dom-complete (empty url) sequence:
        // commit_url becomes Some("") so dom-complete resolves, but the URL is
        // empty and must be back-filled via location.href.
        let (tx, rx) = std::sync::mpsc::channel::<std::sync::Arc<Resource>>();
        tx.send(std::sync::Arc::new(Resource::DocumentEvent(
            serde_json::json!({ "name": "dom-loading", "url": "" }),
        )))
        .unwrap();
        tx.send(std::sync::Arc::new(Resource::DocumentEvent(
            serde_json::json!({ "name": "dom-complete", "url": "" }),
        )))
        .unwrap();
        drop(tx);

        let watcher_actor = ff_rdp_core::ActorId::from("conn0/watcher1");
        let bus_arc = Arc::new(Mutex::new(ResourceCommand::new(watcher_actor)));
        let console = ff_rdp_core::ActorId::from(console_actor);

        // No probe timer needed — the empty dom-complete triggers the fallback
        // — but a probe must be present so the console_actor is available.
        let nav_start = Instant::now();
        let probe = ReadyStateProbe {
            console_actor: &console,
            pre_epoch: 0.0,
            // Push the probe far into the future so only the dom-complete
            // fallback path (not the interleaved probe) fires.
            first_probe_at: nav_start + Duration::from_secs(30),
            probe_interval: Duration::from_secs(30),
        };

        let result = wait_for_doc_complete(
            &mut transport,
            &bus_arc,
            &rx,
            5_000,
            WaitLevel::Complete,
            nav_start,
            Some(&probe),
        );

        server_handle.join().unwrap();

        let ci = result.expect("dom-complete should resolve to a CommitInfo");
        assert_eq!(ci.ready_state, "complete");
        assert_eq!(
            ci.committed_url, "https://spa.example/app",
            "empty dom-complete URL must fall back to location.href, not about:blank"
        );
    }
}
