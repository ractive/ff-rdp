use std::time::{Duration, Instant};

use ff_rdp_core::{RdpTransport, ResourceType, WatcherActor, WindowGlobalTarget};
use serde_json::{Value, json};

use crate::cli::args::Cli;
use crate::error::AppError;
use crate::hints::{HintContext, HintSource};
use crate::output;
use crate::output_pipeline::OutputPipeline;

use super::connect_tab::connect_and_get_target;
use super::js_helpers::{escape_selector, poll_js_condition};
use super::url_validation::validate_content_navigation_url;

mod consent;
mod network_capture;
mod readiness;
mod status;

pub(crate) use network_capture::{NetworkDetail, run_with_network};
pub(crate) use readiness::{
    check_real_tab_url_for_neterror, eval_location_href, wait_for_navigation_commit,
};
pub(crate) use status::not_observed_status;

use consent::merge_auto_consent;
use readiness::{
    ReadinessCheck, ReadyStateProbe, capture_pre_nav_epoch, get_navigation_watcher,
    reclassify_timeout_as_neterror, refresh_console_actor, run_wait_for_predicates,
    split_wait_budget, wait_for_doc_complete_retaining_status, wait_for_readystate_complete,
};
use status::{FallbackStatusEvidence, StatusUnknown};

// Exercised only by this module's unit tests, which call deep into the
// readiness/status internals directly (no live Firefox needed for these).
#[cfg(test)]
use ff_rdp_core::{NavCause, Resource, ResourceCommand};
#[cfg(test)]
use readiness::{
    READINESS_SAMPLE, classify_neterror, epoch_ms_at, error_page_cause, is_neterror_url,
    is_readystate_fresh, is_stale_lifecycle_event, must_reresolve_href, needs_href_fallback,
    probe_same_document_commit, scripted_readiness, wait_for_doc_complete,
};
#[cfg(test)]
use status::{
    DocumentStatusTracker, MAX_STATUS_GRACE_MS, canonical_doc_url, extract_document_status,
    status_grace_budget_ms,
};
#[cfg(test)]
use std::sync::{Arc, Mutex};

/// Restore the socket read timeout to the value established at connect time.
///
/// Called after `drain_network_events` completes so that subsequent RDP
/// round-trips (e.g. unwatch, wait condition polling) use the original timeout.
/// Failures are logged and swallowed — the drain has already completed.
fn restore_timeout(transport: &mut RdpTransport, original_timeout_ms: u64) {
    if let Err(e) = transport.set_read_timeout(Some(Duration::from_millis(original_timeout_ms))) {
        // stderr-ok: (b) warn-and-continue — see the doc comment above; the
        // drain already completed so this failure is logged and swallowed.
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
// they correspond to (--wait-text, --wait-selector, --timeout-ms).
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
    conditions: &crate::cli::args::NetworkConditionsArgs,
    user_agent: Option<&str>,
    page_args: &crate::cli::args::PageViewArgs,
) -> Result<serde_json::Value, AppError> {
    validate_content_navigation_url(url, cli.allow_file_urls, cli.allow_unsafe_urls)?;
    if wait_opts.no_wait && super::network_conditions::is_requested(conditions) {
        return Err(AppError::User(
            "--throttle/--block only last as long as the command's connection — they \
             cannot be combined with --no-wait"
                .to_owned(),
        ));
    }
    if wait_opts.no_wait && user_agent.is_some() {
        return Err(AppError::User(
            "--user-agent only lasts as long as the command's connection — it cannot be \
             combined with --no-wait"
                .to_owned(),
        ));
    }
    let mut ctx = connect_and_get_target(cli)?;
    let target_actor = ctx.target().actor.clone();
    let tab_actor = ctx.target_tab_actor().clone();

    // Get the watcher actor and subscribe to document-event resources before
    // sending navigateTo so we don't miss any events that arrive immediately
    // after the navigate (Firefox may dispatch dom-loading very quickly).
    //
    // iter-174: this must request server-side target switching, or the
    // `document-event` half of the wait is dead on a direct connection and
    // only the `Both` strategy's `document.readyState` poll ever answers —
    // `--wait-strategy events` timed out unconditionally. See
    // `get_navigation_watcher`.
    let watcher_actor = get_navigation_watcher(&mut ctx, &tab_actor)?;
    // `--throttle`/`--block`: set on this connection's watcher before
    // `navigateTo`, so they govern the load this command waits for.
    let conditions_applied =
        super::network_conditions::apply(&mut ctx, &watcher_actor, conditions)?;
    // `--user-agent`: same lifetime as the conditions above — this load and
    // whatever the command does on the page before it disconnects.
    if let Some(ua) = user_agent {
        let configuration = ff_rdp_core::TargetConfiguration {
            custom_user_agent: Some(ua.to_owned()),
            ..Default::default()
        };
        super::emulation::apply(&mut ctx, &watcher_actor, &configuration)?;
    }

    // Missing baseline evidence stays absent, never a synthetic epoch zero.
    let pre_nav_epoch = if wait_opts.no_wait {
        None
    } else {
        capture_pre_nav_epoch(&mut ctx, "navigate: pre-nav epoch eval")
    };

    // `window.location.href` captured before dispatch (iter-138 Themes B/C)
    // — see `wait_for_navigation_commit`'s identical capture for why: it's
    // the baseline `probe_same_document_commit` needs to detect a same-page
    // fragment navigation, which (like SPA `pushState`/`popstate`) never
    // fires a `document-event` and never advances `navigationStart`.
    let pre_nav_href: String = if wait_opts.no_wait {
        String::new()
    } else {
        let console_actor = ctx.target().console_actor.clone();
        eval_location_href(ctx.transport_mut(), &console_actor)
    };
    tracing::debug!(requested_url = url, pre_href = %pre_nav_href, pre_epoch = ?pre_nav_epoch, target = ?ctx.target(), "navigate: baseline captured");

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
        refresh_console_actor(&mut ctx);
        let rs_result = wait_for_readystate_complete(
            &mut ctx,
            cli.timeout,
            ReadinessCheck {
                pre_epoch: pre_nav_epoch,
                pre_href: &pre_nav_href,
                requested_url: url,
            },
            nav_start,
        );
        let ci = reclassify_timeout_as_neterror(&mut ctx, url, rs_result)?;
        // An error page reaches `complete` too: no status is observed on
        // this route, so check the tab's real URL on success as well.
        if let Some(nav_err) = check_real_tab_url_for_neterror(&mut ctx, url) {
            return Err(nav_err);
        }
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
        //
        // iter-138 Theme A: also subscribe to `NetworkEvent` so
        // `wait_for_doc_complete` can observe the main document's HTTP
        // status. Both types share one subscription/channel — dispatch_event
        // fans out by type to whichever subscribers registered for it.
        let (sub_id, rx) = bus_arc
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .subscribe(
                ctx.transport_mut(),
                &[ResourceType::DocumentEvent, ResourceType::NetworkEvent],
            )
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
        //
        // `console_actor` is captured from the PRE-navigation target — it is
        // refreshed against the new docshell inside `wait_for_doc_complete`
        // once `dom-loading` commits (or lazily before the first probe
        // attempt). Without that refresh every probe eval fails with
        // `noSuchActor` for the lifetime of the wait, silently defeating this
        // fast path and falling through to the full events-budget timeout
        // (the iter-124 fix for the iter-122 Theme A regression).
        let mut readystate_probe = if wait_opts.wait_strategy == WaitStrategy::Both {
            Some(ReadyStateProbe {
                console_actor: Some(ctx.target().console_actor.clone()),
                tab_actor: &tab_actor,
                pre_epoch: pre_nav_epoch,
                // Give dom-complete a 300 ms head start on pages that fire it
                // promptly (comparis fired it in ~0.69 s), then probe every
                // 250 ms so events keep priority but a stuck page is caught
                // quickly rather than after the full events budget.
                first_probe_at: nav_start + Duration::from_millis(300),
                probe_interval: Duration::from_millis(250),
                poll_enabled: true,
                pre_href: pre_nav_href.clone(),
                trust_event_url: true,
            })
        } else {
            None
        };

        // wait_for_doc_complete acquires the lock only during dispatch_event,
        // not across the full recv() wait — see its lock-discipline doc-comment.
        let mut fallback_status = FallbackStatusEvidence::new(ctx.target());
        let event_result = wait_for_doc_complete_retaining_status(
            ctx.transport_mut(),
            &bus_arc,
            &rx,
            events_budget,
            wait_opts.wait_level,
            nav_start,
            readystate_probe.as_mut(),
            url,
            // This is the one route that subscribes to `NetworkEvent`
            // alongside `DocumentEvent` (see the `subscribe` call above), so a
            // missing status here really does mean the server or the document
            // produced none (iter-166).
            true,
            (wait_opts.wait_strategy == WaitStrategy::Both).then_some(&mut fallback_status),
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
        let mut used_fallback = false;
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
                    used_fallback = true;
                    wait_for_readystate_complete(
                        &mut ctx,
                        remaining,
                        ReadinessCheck {
                            pre_epoch: pre_nav_epoch,
                            pre_href: &pre_nav_href,
                            requested_url: url,
                        },
                        nav_start,
                    )
                }
            }
            Err(e) => Err(e),
        };
        let commit = reclassify_timeout_as_neterror(&mut ctx, url, result)?;

        // iter-174: the same check on the SUCCESS path, gated on "no HTTP
        // status was observed".
        //
        // `reclassify_timeout_as_neterror` only fires on a `Timeout`, and
        // before iter-174 that was enough on the direct route *by accident*:
        // no `dom-complete` ever arrived, so a bad-DNS `navigate` always timed
        // out and got reclassified. With the events path working, the commit
        // now succeeds — and a neterror document is indistinguishable from a
        // real one by URL, because Firefox reports the FAILED url from both
        // `location.href` and the `document-event`s (measured: `dom-loading`
        // url = `https://…invalid/`, never `about:neterror`; see
        // `check_real_tab_url_for_neterror`'s doc comment for why only
        // `listTabs` sees the truth).
        //
        // Gated on `http_status.is_none()` rather than run unconditionally: a
        // navigation whose response line was observed reached a server and
        // cannot be a neterror, so the common path keeps its round-trip count.
        // A neterror never produces one — the request failed before any
        // response.
        let commit = if commit.http_status.is_none() {
            match check_real_tab_url_for_neterror(&mut ctx, url) {
                Some(nav_err) => return Err(nav_err),
                None => commit,
            }
        } else {
            commit
        };

        // Preserve the existing fallback neterror check above: an observed
        // response from a candidate request must never suppress that check.
        let mut commit = commit;
        if used_fallback {
            (commit.http_status, commit.status_reason) =
                fallback_status.resolve(&commit.committed_url);
        }
        Some(commit)
    };

    // This connection's target and navigation latch do not escape run_core.
    // Refresh only when a postcommit consumer will use them; plain committed
    // navigation can return its captured result and drop the connection. Keep
    // no-wait's existing sequence because it has not established a commit.
    if wait_opts.no_wait
        || wait_opts.has_condition()
        || !wait_opts.wait_for.is_empty()
        || page_args.with_page
    {
        refresh_console_actor(&mut ctx);
    }

    let wait_result = wait_after_navigate(&mut ctx, wait_opts)?;

    // Parse and run --wait-for predicates after commit.
    let wait_for_result = run_wait_for_predicates(&mut ctx, wait_opts)?;

    // iter-138 Theme A: `status` is always present, defaulting to `null` —
    // consistent with iter-128's always-present-nullable-key convention.
    // iter-166 Theme B: `status_reason` is present alongside it and says which
    // kind of `null` this is. The default pair below is what `--no-wait`
    // reports: no network subscription is ever started, so nothing was
    // observed — as opposed to the server having answered without a status.
    let mut result = json!({
        "navigated": url,
        "status": Value::Null,
        "status_reason": StatusUnknown::NotObserved.as_str(),
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

    // iter-210 Theme A: `--with-page`. Collected here, last, and on the
    // connection this navigation already owns — after the commit wait and
    // after any `--wait-text`/`--wait-selector`/`--wait-for` predicate, so
    // the view describes the document this command produced rather than the
    // one it left. See `page_view::collect`.
    if page_args.with_page {
        super::page_view::attach(cli, &mut ctx, &mut result, Some(cli.timeout), page_args)?;
    }

    super::network_conditions::insert_echo(&mut result, conditions_applied.as_ref());
    if let (Some(ua), Some(obj)) = (user_agent, result.as_object_mut()) {
        obj.insert("user_agent".to_owned(), json!(ua));
    }
    Ok(result)
}

pub fn run(
    cli: &Cli,
    url: &str,
    wait_opts: &WaitAfterNav<'_>,
    auto_consent: bool,
    conditions: &crate::cli::args::NetworkConditionsArgs,
    user_agent: Option<&str>,
    page_args: &crate::cli::args::PageViewArgs,
) -> Result<(), AppError> {
    // iter-210: `--with-page` promises the page it returns describes the
    // document *this command* produced. `--auto-consent`'s dismiss click
    // runs after `run_core` returns (on a fresh connection — see
    // `merge_auto_consent`'s doc comment), so collecting the page inside
    // `run_core` would hand back the pre-consent document. When both flags
    // are set, defer collection to a second connection opened after consent
    // runs, matching `run_with_network`'s ordering (consent before
    // `page_view::attach`).
    let defer_with_page = auto_consent && page_args.with_page;
    // Borrow on the common path; clone only when the deferred branch needs an
    // owned copy with `with_page` flipped off (iter-219 review).
    let core_args: std::borrow::Cow<'_, crate::cli::args::PageViewArgs> = if defer_with_page {
        std::borrow::Cow::Owned(crate::cli::args::PageViewArgs {
            with_page: false,
            ..page_args.clone()
        })
    } else {
        std::borrow::Cow::Borrowed(page_args)
    };
    let mut result = run_core(
        cli,
        url,
        wait_opts,
        conditions,
        user_agent,
        core_args.as_ref(),
    )?;
    if auto_consent {
        merge_auto_consent(cli, &mut result);
    }
    if defer_with_page {
        let mut ctx = connect_and_get_target(cli)?;
        super::page_view::attach(cli, &mut ctx, &mut result, Some(cli.timeout), page_args)?;
    }
    let mut meta = json!({});
    let page_text = super::page_view::lift_meta(cli, &mut result, &mut meta);
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
    super::page_view::render_text_section(page_text.as_ref());
    Ok(())
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
    let console_actor =
        ctx.refresh_target_until(Instant::now() + Duration::from_millis(opts.wait_timeout))?;

    let condition = describe_wait_condition(opts);
    let timeout_msg = format!(
        "navigate wait timed out after {}ms — condition not met: {condition}; increase with --timeout-ms",
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
mod tests;
