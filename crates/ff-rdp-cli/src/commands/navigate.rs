use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use ff_rdp_core::{
    Grip, NavCause, RdpTransport, Resource, ResourceCommand, ResourceType, RootActor, TabActor,
    WatcherActor, WindowGlobalTarget, parse_network_resource_updates, parse_network_resources,
};
use serde_json::{Value, json};

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
use super::network_events::{build_network_entries, drain_network_events_timed, merge_updates};
use super::url_validation::validate_url_with_opts;

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
    /// The main document's HTTP status code (iter-138 Theme A), when observed
    /// via a `network-event` resource whose `cause_type == "document"` and
    /// whose `url` matches the requested URL. `None` when the caller didn't
    /// subscribe to network events (`back`/`forward`/`reload` don't — Theme A
    /// only covers `navigate`) or when the status update hadn't arrived by
    /// the time the wait resolved (e.g. events-phase timeout fell back to the
    /// readystate poll, which has no network subscription at all). Callers
    /// must surface this as an explicit `null`, never omit it — consistent
    /// with iter-128's always-present-nullable-key convention.
    http_status: Option<u16>,
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
    ///
    /// Captured *before* `navigateTo` is dispatched, so it is bound to the
    /// **pre-navigation** docshell. Firefox tears down the docshell (and, for
    /// cross-process navigations, the child process) once the new document
    /// commits, which invalidates this ID (`noSuchActor` on every eval). The
    /// wait loop refreshes it via `tab_actor` as soon as `dom-loading` is
    /// observed (see the `noSuchActor` fix, iter-124).
    console_actor: ff_rdp_core::ActorId,
    /// Tab descriptor actor used to re-resolve `console_actor` once the new
    /// docshell has committed (`getTarget` returns fresh actor IDs).
    tab_actor: &'a ff_rdp_core::ActorId,
    /// `performance.timing.navigationStart` captured before `navigateTo`; a
    /// `complete` reading whose `navigationStart` is not fresher than this is
    /// stale (belongs to the prior page) and is ignored.
    pre_epoch: f64,
    /// Do not probe until this instant, giving the (faster, richer)
    /// `dom-complete` event a head start on pages that do fire it promptly.
    first_probe_at: Instant,
    /// Minimum spacing between readystate probes so events keep priority.
    probe_interval: Duration,
    /// When `true` (the default for `navigate`'s `Both` strategy), the wait
    /// loop eagerly refreshes `console_actor` on the very first `dom-loading`
    /// (regardless of whether the event's own URL is usable) and interleaves
    /// the periodic `document.readyState` poll below. When `false`
    /// (`wait_for_navigation_commit`'s `back`/`forward`/`reload` — iter-130
    /// Theme B), both of those are skipped and the probe exists solely to
    /// supply `console_actor`/`tab_actor` to the need-gated
    /// `needs_href_fallback` resolution paths on `dom-loading`/
    /// `dom-interactive`/`dom-complete`.
    ///
    /// This distinction matters, not just for the FF152 dom-complete-never-
    /// fires workaround `back`/`forward`/`reload` don't need: the eager
    /// refresh calls `refresh_probe_console_actor`, a **blocking** `getTarget`
    /// round-trip issued synchronously from inside this loop's `dom-loading`
    /// handling. If a `dom-complete` for the same navigation is already
    /// in-flight on the wire at that moment (a real, observed race — Firefox
    /// can fire `dom-loading` and `dom-complete` back-to-back), that blocking
    /// call's `recv_reply_from` will read it first while scanning for the
    /// `getTarget` reply and — with no event sink installed on this raw
    /// `transport` — silently drop it (the exact class of bug documented in
    /// `kb/rdp/actors/watcher.md`'s iter-129 Note 1). `navigate` accepts this
    /// narrow risk in exchange for the FF152 fast-path; `back`/`forward`/
    /// `reload` have no such trade to make, so they opt out entirely.
    poll_enabled: bool,
    /// `window.location.href` captured immediately before the navigation
    /// action was dispatched (iter-138 Themes B/C).
    ///
    /// Feeds [`probe_same_document_commit`], which detects same-document
    /// navigations (SPA `history.pushState`/`popstate` traversal, same-page
    /// fragment navigation) that never produce a `document-event` at all —
    /// Firefox does not tear down/reload the document for these, so the
    /// `dom-loading`/`dom-complete` event stream this function otherwise
    /// relies on stays silent forever, and the freshness-guarded
    /// `probe_readystate_complete` fast path can't help either (its guard
    /// requires `navigationStart` to advance, which same-document
    /// navigations never do). An empty string disables the check (no
    /// baseline to compare against — see `probe_same_document_commit`).
    pre_href: String,
    /// Whether a `document-event`'s own `url` field may be trusted as the
    /// committed URL (iter-138 Theme F).
    ///
    /// `true` for `navigate` (the default, preserving pre-iter-138
    /// behaviour). `false` for `wait_for_navigation_commit`'s
    /// `back`/`forward`/`reload`: `watchTargets("frame")` (required to make
    /// the watcher deliver anything at all — iter-79 Theme A) makes Firefox
    /// also emit `document-event`s for subframe targets, and a same-tab
    /// history traversal can restore the top-level document from BFCache
    /// (firing no document-event of its own) while an unrelated subframe
    /// (e.g. an ad/analytics iframe) reloads and fires a perfectly normal
    /// `dom-loading`/`dom-complete` cycle — which this wait loop would
    /// otherwise mistake for the real navigation's completion, reporting the
    /// subframe's URL as `committed_url`. When `false`, every commit
    /// resolution path re-resolves via `eval_location_href` against
    /// `console_actor` refreshed through `tab_actor` (always the TAB's
    /// top-level target, never a subframe's) instead of trusting the event's
    /// own `url`, regardless of whether that URL looks well-formed.
    trust_event_url: bool,
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

/// Detect a completed same-document navigation (iter-138 Themes B/C).
///
/// Same-document navigations — `history.pushState`/`history.replaceState`,
/// `popstate` traversal (`back`/`forward` across SPA route entries), and
/// same-page fragment navigation (`#frag`) — never tear down or reload the
/// document, so they never fire a `document-event` and never advance
/// `performance.timing.navigationStart`. [`probe_readystate_complete`]'s
/// freshness guard can therefore never be satisfied for them, and the plain
/// event wait in [`wait_for_doc_complete`] has nothing to observe at all —
/// the exact cause of the iter-130 regression this iteration fixes: a
/// correct same-document traversal burned the full wait budget and returned
/// `AppError::Timeout` (exit 124) even though `location.href` confirmed it
/// had already succeeded.
///
/// Because the document is never torn down, `console_actor` never goes
/// stale for this check — unlike the cross-document paths elsewhere in this
/// module, no `refresh_probe_console_actor` call is needed before evaluating
/// this condition.
///
/// Returns the new `location.href` once it differs from `pre_href` AND
/// `document.readyState === 'complete'` (the document was already fully
/// loaded before the same-document navigation began, and same-document
/// navigations never change that). Returns `None` while the condition
/// doesn't hold, on any transport/eval error (treated as "not yet" so a
/// transient hiccup never aborts the wait), and when `pre_href` is empty
/// (no baseline to compare against — e.g. the pre-navigation `location.href`
/// eval itself failed).
fn probe_same_document_commit(
    transport: &mut RdpTransport,
    console_actor: &ff_rdp_core::ActorId,
    pre_href: &str,
) -> Option<String> {
    if pre_href.is_empty() {
        return None;
    }
    let pre_href_json = serde_json::to_string(pre_href).unwrap_or_else(|_| "\"\"".to_owned());
    let condition = format!(
        "(function() {{ \
           if (document.readyState !== 'complete') return null; \
           var h = window.location.href; \
           return h !== {pre_href_json} ? h : null; \
         }})()"
    );
    match ff_rdp_core::WebConsoleActor::evaluate_js_async(transport, console_actor, &condition) {
        Ok(result) if result.exception.is_none() => match result.result {
            Grip::Value(Value::String(s)) if !s.is_empty() => Some(s),
            _ => None,
        },
        _ => None,
    }
}

/// [`probe_same_document_commit`], guarded against swallowing an in-flight
/// `document-event` (iter-138 hardening — the exact bug class documented in
/// `kb/rdp/actors/watcher.md`'s iter-129 Note 1, and the reason this same
/// pattern is already used by `enumerate_frame_targets`, iter-129 Theme A).
///
/// `evaluate_js_async`'s blocking `recv_reply_from` reads raw packets off
/// `transport` looking for its own reply; any *other* packet it reads first —
/// including a genuine `dom-loading`/`dom-complete` document-event that
/// arrived on the wire before this probe fired — is forwarded to whatever
/// event sink is installed, or silently dropped if none is. Because this
/// check runs unconditionally on every `probe_interval` tick (unlike the
/// FF152 `poll_enabled` fast path, which only runs for `navigate` and
/// therefore never contends with `back`/`forward`/`reload`'s in-flight
/// events), it MUST install a temporary sink around the eval and replay
/// anything captured back through `bus_arc.dispatch_event` — otherwise a
/// same-document check that happens to fire while the real commit event is
/// already buffered on the socket would eat that event and the main loop
/// would then wait forever for an event that already arrived and was
/// discarded.
fn probe_same_document_commit_safe(
    transport: &mut RdpTransport,
    bus_arc: &Arc<Mutex<ResourceCommand>>,
    console_actor: &ff_rdp_core::ActorId,
    pre_href: &str,
) -> Option<String> {
    if pre_href.is_empty() {
        // Skip the sink dance entirely when the check itself is a no-op —
        // `probe_same_document_commit` would return `None` immediately
        // without touching `transport`.
        return None;
    }
    let (tx, rx) = std::sync::mpsc::channel::<Value>();
    let prev_sink = transport.swap_event_sink(Some(tx));
    let result = probe_same_document_commit(transport, console_actor, pre_href);
    transport.swap_event_sink(prev_sink);

    // Replay anything the eval swallowed, in delivery order, so the main
    // loop's next top-of-loop drain observes it exactly as if it had read it
    // directly off the wire itself.
    let captured: Vec<Value> = rx.try_iter().collect();
    if !captured.is_empty() {
        let mut bus = bus_arc
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        for packet in captured {
            bus.dispatch_event(&packet);
        }
    }

    result
}

/// Decide whether a candidate committed URL from a `document-event` must be
/// re-resolved via `eval_location_href` rather than trusted verbatim.
///
/// Combines the pre-existing `needs_href_fallback` (empty/placeholder
/// `about:blank`, iter-122/130) with the Theme F guard: when the probe opts
/// out of trusting event URLs at all (`trust_event_url: false` —
/// `back`/`forward`/`reload`, see [`ReadyStateProbe::trust_event_url`]),
/// re-resolution is unconditional regardless of how well-formed `candidate`
/// looks, because a well-formed-but-wrong subframe URL passes
/// `needs_href_fallback` unmodified.
fn must_reresolve_href(
    probe: Option<&ReadyStateProbe<'_>>,
    candidate: &str,
    requested_url: &str,
) -> bool {
    probe.is_some_and(|p| !p.trust_event_url) || needs_href_fallback(candidate, requested_url)
}

/// Re-resolve [`ReadyStateProbe::console_actor`] against the current docshell.
///
/// The probe's console actor is captured *before* `navigateTo` is dispatched
/// (see [`ReadyStateProbe`]), so it is bound to the pre-navigation docshell.
/// Firefox tears down that docshell — and, for cross-process navigations, the
/// child process — once the new document commits, which invalidates the old
/// actor ID (every subsequent eval fails with `noSuchActor`). This refresh
/// must run once the new docshell has committed server-side (signalled by
/// `dom-loading`, or lazily on first probe attempt if the events stream is
/// quiet) so the interleaved fast-path can actually observe `complete`
/// instead of failing silently for the rest of the wait (iter-124 fix for
/// the iter-122 Theme A regression).
///
/// Best-effort: a failed refresh leaves the stale actor in place and returns
/// `false` so the caller does NOT latch `probe_refreshed` — the new docshell
/// may not have finished registering server-side yet (a transient
/// `getTarget` failure), so the next probe-timer tick should retry rather
/// than permanently stranding the probe on the stale actor (iter-124 review
/// fix: latching on `Err` reintroduced the exact `noSuchActor` bug this
/// function exists to fix, just intermittently instead of always).
fn refresh_probe_console_actor(
    transport: &mut RdpTransport,
    probe: &mut ReadyStateProbe<'_>,
) -> bool {
    match ff_rdp_core::TabActor::get_target(transport, probe.tab_actor) {
        Ok(fresh) => {
            probe.console_actor = fresh.console_actor;
            true
        }
        Err(e) => {
            tracing::debug!(
                error = %e,
                "navigate: readystate probe console actor refresh failed; \
                 probe will keep using the stale actor and retry on the next attempt"
            );
            false
        }
    }
}

/// Resolve `window.location.href` via `console_actor`, returning an empty string
/// on any error. Used as the URL source for both the readystate fast-path and as
/// a fallback when a committing `document-event` carries no `url` (iter-122
/// Theme B — avoids emitting `about:blank` for SPAs that never fire
/// `dom-loading` with a URL).
pub(crate) fn eval_location_href(
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

/// Evaluate `document.readyState` via `console_actor`, returning an empty
/// string on any error.
///
/// Used by `navigate --with-network` (iter-138 Theme G) to populate the same
/// `ready_state` field the plain `navigate` envelope reports — the network
/// drain already waits for the page to settle, so by the time this is called
/// the document should genuinely be `complete`, but the eval is best-effort
/// like `eval_location_href`: a failure just leaves the field empty rather
/// than failing the whole command.
pub(crate) fn eval_document_ready_state(
    transport: &mut RdpTransport,
    console_actor: &ff_rdp_core::ActorId,
) -> String {
    match ff_rdp_core::WebConsoleActor::evaluate_js_async(
        transport,
        console_actor,
        "document.readyState",
    ) {
        Ok(result) => match result.result {
            Grip::Value(serde_json::Value::String(s)) => s,
            _ => String::new(),
        },
        Err(_) => String::new(),
    }
}

/// Returns `true` when `candidate` (the URL reported by a `document-event`)
/// cannot be trusted as the real committed URL and must be re-resolved via
/// `location.href` (iter-130 Theme A).
///
/// Two cases:
/// - `candidate` is empty — the event carried no URL at all (the original
///   iter-122 Theme B case).
/// - `candidate` is the literal string `"about:blank"` while the navigation
///   actually requested a different (non-`about:blank`) URL. Firefox's SPA
///   route-commit flow (observed on comparis.ch) can report a committing
///   `document-event` whose `url` field is `about:blank` even though the
///   real document has already landed on the requested URL — `ready_state`
///   and a manual `eval location.href` both confirm the real page loaded.
///   A caller trusting a literal `"about:blank"` here would wrongly
///   conclude the navigation failed.
fn needs_href_fallback(candidate: &str, requested_url: &str) -> bool {
    candidate.is_empty() || (candidate == "about:blank" && requested_url != "about:blank")
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
///
/// Update `doc_resource_id`/`doc_status` from a network resource (iter-138
/// Theme A tracking, shared by `wait_for_doc_complete`'s main drain and its
/// post-loop grace-wait), and return the inner `Value` when `resource` is a
/// `DocumentEvent` (the caller should continue processing it), or `None`
/// when `resource` was a `NetworkEvent`/`NetworkUpdate` (already handled
/// here) or an unrelated resource type (nothing to do).
///
/// Matched by `cause_type == "document"` (Firefox's netmonitor cause for a
/// top-level or subframe HTML document load) AND `url == requested_url`
/// (the exact URL passed to `navigateTo`) so a subframe's own document
/// request (same cause type, different URL) is not mistaken for the page's
/// own navigation — the same subframe-contamination risk Theme F hits for
/// `document-event`.
fn extract_document_event<'a>(
    resource: &'a Resource,
    requested_url: &str,
    doc_resource_id: &mut Option<u64>,
    doc_status: &mut Option<u16>,
) -> Option<&'a Value> {
    match resource {
        Resource::NetworkEvent(res) => {
            if res.cause_type == "document" && res.url == requested_url {
                *doc_resource_id = Some(res.resource_id);
            }
            None
        }
        Resource::NetworkUpdate(upd) => {
            if *doc_resource_id == Some(upd.resource_id)
                && let Some(ref s) = upd.status
                && let Ok(code) = s.parse::<u16>()
            {
                *doc_status = Some(code);
            }
            None
        }
        Resource::DocumentEvent(v) => Some(v),
        _ => None,
    }
}

/// `requested_url` (iter-130 Theme A) pushed the parameter count to 8; the
/// function is already heavily documented per-parameter above and splitting
/// it would obscure the single event-drain loop it implements.
#[allow(clippy::too_many_arguments)]
fn wait_for_doc_complete(
    transport: &mut RdpTransport,
    bus_arc: &Arc<Mutex<ResourceCommand>>,
    rx: &std::sync::mpsc::Receiver<std::sync::Arc<Resource>>,
    timeout_ms: u64,
    wait_level: WaitLevel,
    nav_start: Instant,
    mut probe: Option<&mut ReadyStateProbe<'_>>,
    requested_url: &str,
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
    let mut next_probe_at = probe.as_ref().map(|p| p.first_probe_at);
    // Next instant at which the same-document commit check (iter-138 Themes
    // B/C) may run. Shares `probe`'s cadence (`first_probe_at`/
    // `probe_interval`) but its own timer, because it runs unconditionally
    // (not gated by `poll_enabled` — see `probe_same_document_commit`'s doc
    // comment for why `back`/`forward`/`reload` need this just as much as
    // `navigate` does).
    let mut same_doc_next_check_at = probe.as_ref().map(|p| p.first_probe_at);
    // Tracks whether the probe's console actor has been refreshed against the
    // post-navigation docshell yet (see the noSuchActor fix, iter-124).
    let mut probe_refreshed = false;
    // The main document's network resource_id and observed HTTP status
    // (iter-138 Theme A). Populated only when the caller subscribed to
    // `ResourceType::NetworkEvent` alongside `DocumentEvent` (currently only
    // `navigate`'s `run_core` does — `back`/`forward`/`reload` don't, so
    // these stay `None`/`None` and `CommitInfo.http_status` reports `null`
    // for them, which is correct: Theme A only covers `navigate`).
    let mut doc_resource_id: Option<u64> = None;
    let mut doc_status: Option<u16> = None;

    let mut commit_info: CommitInfo = 'wait: loop {
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
            let Some(v) = extract_document_event(
                arc.as_ref(),
                requested_url,
                &mut doc_resource_id,
                &mut doc_status,
            ) else {
                continue;
            };
            {
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
                        // The new docshell has committed server-side, so its
                        // consoleActor can now be re-resolved (see the
                        // noSuchActor fix, iter-124): the probe was built
                        // from the *pre-navigation* target, and that ID goes
                        // stale the moment Firefox tears down the old
                        // docshell/process — which can happen before this
                        // point. Refresh once so the interleaved readystate
                        // probe (and the location.href fallbacks below) hit
                        // a live actor instead of failing with noSuchActor
                        // on every attempt for the rest of the wait.
                        //
                        // Only worth the round-trip when the refreshed actor
                        // will actually be consumed: the `Complete` probe
                        // always uses it later, and the immediate
                        // loading/interactive fallback below only fires when
                        // this event's URL is empty. A `Loading`/`Interactive`
                        // wait with a non-empty URL resolves straight from
                        // `url` a few lines down without ever touching
                        // `p.console_actor` (iter-124 review fix — avoids a
                        // wasted blocking eval round-trip on the common case).
                        //
                        // iter-138 Theme F note: `trust_event_url: false`
                        // (`back`/`forward`/`reload`) deliberately does NOT
                        // gate this eager refresh — doing so would reintroduce
                        // the exact blocking-`getTarget`-swallows-an-in-flight-
                        // `dom-complete` race `poll_enabled: false` exists to
                        // avoid for these three verbs (see
                        // `ReadyStateProbe::poll_enabled`'s doc comment).
                        // Instead, the dom-complete branch below re-resolves
                        // lazily: it first evals against whatever actor is
                        // already cached (cheap, and correct whenever the
                        // docshell survived, e.g. a same-document`
                        // BFCache-restored back), and only pays for a fresh
                        // `getTarget` if that first eval comes back empty
                        // (stale actor).
                        if !probe_refreshed
                            && let Some(p) = probe.as_deref_mut()
                            && ((wait_level == WaitLevel::Complete && p.poll_enabled)
                                || needs_href_fallback(&url, requested_url))
                            && refresh_probe_console_actor(transport, p)
                        {
                            probe_refreshed = true;
                        }
                        // --wait loading: resolve immediately on dom-loading.
                        if wait_level == WaitLevel::Loading {
                            let elapsed_ms =
                                u64::try_from(nav_start.elapsed().as_millis()).unwrap_or(u64::MAX);
                            // Theme B/iter-130 Theme A: if the event carried no
                            // URL (or a literal "about:blank" that doesn't match
                            // what was requested), resolve the real URL via
                            // location.href rather than trusting the event's
                            // placeholder value — same fallback applied to the
                            // Interactive/Complete paths below. iter-138 Theme F:
                            // `must_reresolve_href` additionally forces this for
                            // any probe with `trust_event_url: false`
                            // (`back`/`forward`/`reload`), regardless of how
                            // well-formed `url` looks — it may be a subframe's.
                            let committed_url =
                                if must_reresolve_href(probe.as_deref(), &url, requested_url) {
                                    probe.as_ref().map_or_else(String::new, |p| {
                                        eval_location_href(transport, &p.console_actor)
                                    })
                                } else {
                                    url
                                };
                            break 'wait CommitInfo {
                                committed_url,
                                ready_state: "loading".to_owned(),
                                elapsed_ms,
                                http_status: doc_status,
                            };
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
                            // Theme B/iter-130 Theme A: if the event carried no
                            // URL (or a literal "about:blank" mismatch), resolve
                            // the real URL via location.href rather than trusting
                            // the placeholder value. iter-138 Theme F:
                            // `must_reresolve_href` forces this unconditionally
                            // for `trust_event_url: false` probes.
                            let committed_url =
                                if must_reresolve_href(probe.as_deref(), &eff_url, requested_url) {
                                    probe.as_ref().map_or_else(String::new, |p| {
                                        eval_location_href(transport, &p.console_actor)
                                    })
                                } else {
                                    eff_url
                                };
                            break 'wait CommitInfo {
                                committed_url,
                                ready_state: "interactive".to_owned(),
                                elapsed_ms,
                                http_status: doc_status,
                            };
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
                        // Theme B/iter-130 Theme A: an empty URL (SPA that never
                        // fired a dom-loading with a URL) or a literal
                        // "about:blank" that doesn't match the requested URL
                        // (the comparis.ch SPA route-commit case — dom-complete
                        // fires with `ready_state: complete` and the real page
                        // has genuinely landed, but the event's own `url` field
                        // is still the initial `about:blank` placeholder) must
                        // be resolved from the live document rather than
                        // surfaced verbatim. iter-138 Theme F:
                        // `must_reresolve_href` also forces this path
                        // unconditionally for `trust_event_url: false` probes
                        // (`back`/`forward`/`reload`) — `committed` may be a
                        // subframe's URL, which passes `needs_href_fallback`
                        // unmodified because it looks like a perfectly valid
                        // (non-empty, non-"about:blank") URL.
                        if must_reresolve_href(probe.as_deref(), &committed, requested_url) {
                            let mut href = probe.as_ref().map_or_else(String::new, |p| {
                                eval_location_href(transport, &p.console_actor)
                            });
                            // iter-130 Theme A hardening (comparis.ch live-Firefox
                            // repro, not caught by any mock-based unit test): this
                            // `dom-complete` may be Firefox's transient
                            // about:blank intermediate docshell for a
                            // cross-process navigation (it fires a full
                            // loading→interactive→complete cycle of its own,
                            // typically before the real cross-process swap even
                            // starts) rather than the requested page — and by
                            // the time we eval it, that transitional docshell may
                            // already be torn down (`href` empty/noSuchActor) as
                            // well as reporting a literal about:blank while
                            // alive. `needs_href_fallback` treats both the same
                            // way, so re-run it on `href` itself (not a bespoke
                            // `== "about:blank"` check) before and after the
                            // forced refresh, or a torn-down-actor empty read
                            // would silently fall through to the stale `committed`
                            // value below instead of being caught as ambiguous.
                            if needs_href_fallback(&href, requested_url)
                                && let Some(p) = probe.as_deref_mut()
                                && refresh_probe_console_actor(transport, p)
                            {
                                probe_refreshed = true;
                                href = eval_location_href(transport, &p.console_actor);
                            }
                            if needs_href_fallback(&href, requested_url) {
                                // Still ambiguous after a fresh lookup — most
                                // likely still the intermediate docshell's own
                                // dom-complete. Don't return a lie: drop this
                                // reading and keep waiting for the real
                                // navigation's dom-loading/dom-complete (or a
                                // later probe tick, which retries the same fresh
                                // lookup). `commit_url`/`interactive_url` were
                                // already reset by the `.take()` calls above, so
                                // the next real dom-loading is tracked cleanly.
                                continue;
                            }
                            break 'wait CommitInfo {
                                committed_url: href,
                                ready_state: "complete".to_owned(),
                                elapsed_ms,
                                http_status: doc_status,
                            };
                        }
                        break 'wait CommitInfo {
                            committed_url: committed,
                            ready_state: "complete".to_owned(),
                            elapsed_ms,
                            http_status: doc_status,
                        };
                    }
                    _ => {}
                }
            }
        }

        // iter-138 Themes B/C: check for a completed same-document navigation
        // (SPA `pushState`/`popstate` traversal, same-page fragment nav).
        // Unconditional — not gated by `p.poll_enabled` like the FF152
        // fast-path below — because `back`/`forward`/`reload` need this
        // check just as much as `navigate` does (their probes are built with
        // `poll_enabled: false`). It does NOT skip the blocking-round-trip
        // race the FF152 fast-path avoids by staying off for those three
        // verbs — an in-flight `dom-complete` can equally be sitting on the
        // wire when THIS check's eval fires, so it goes through
        // `probe_same_document_commit_safe`, which installs a temporary event
        // sink and replays anything the eval's `recv_reply_from` would
        // otherwise have swallowed.
        if wait_level == WaitLevel::Complete
            && let (Some(p), Some(when)) = (probe.as_deref(), same_doc_next_check_at)
            && Instant::now() >= when
        {
            if let Some(href) =
                probe_same_document_commit_safe(transport, bus_arc, &p.console_actor, &p.pre_href)
            {
                let elapsed_ms = u64::try_from(nav_start.elapsed().as_millis()).unwrap_or(u64::MAX);
                break 'wait CommitInfo {
                    committed_url: href,
                    ready_state: "complete".to_owned(),
                    elapsed_ms,
                    http_status: doc_status,
                };
            }
            same_doc_next_check_at = Some(Instant::now() + p.probe_interval);
        }

        // Theme A fast-path: interleave a lightweight readystate probe so a page
        // that is already `complete` returns without waiting out the events
        // budget for a `dom-complete` event that may never fire on FF152. Only
        // active for the `Both` strategy (probe is None for `Events`) AND only
        // when the caller actually wants `Complete` — the probe can only ever
        // observe `document.readyState === 'complete'`, so honoring it for
        // `--wait loading`/`--wait interactive` would return the wrong
        // `ready_state` (and skip waiting for the dom-loading/dom-interactive
        // event those levels are documented to resolve on). Runs at most once
        // per `probe_interval`, after `first_probe_at`, so the richer event
        // stream keeps priority on pages that do fire dom-complete.
        if wait_level == WaitLevel::Complete
            && let (Some(p), Some(when)) = (probe.as_deref_mut(), next_probe_at)
            && p.poll_enabled
            && Instant::now() >= when
        {
            // Fallback refresh: normally `dom-loading` already refreshed the
            // console actor above, but if the events stream is quiet (no
            // document-event delivered yet) this is the first opportunity.
            if !probe_refreshed && refresh_probe_console_actor(transport, p) {
                probe_refreshed = true;
            }
            if probe_readystate_complete(transport, &p.console_actor, p.pre_epoch) {
                let mut committed = eval_location_href(transport, &p.console_actor);
                // iter-130 Theme A hardening (comparis.ch live-Firefox repro,
                // not caught by any mock-based unit test): a
                // `readyState === 'complete'` reading whose resolved URL is a
                // literal `about:blank` mismatch is Firefox's transient
                // about:blank intermediate docshell for a cross-process
                // navigation, not the real requested page — that docshell
                // also gets a fresh `navigationStart`, so it passes the
                // `pre_epoch` freshness guard, and `about:blank` loads and
                // reports `complete` almost instantly, letting this probe
                // "win" the race against the real navigation. Force one more
                // fresh actor lookup + re-eval before trusting the reading —
                // `refresh_probe_console_actor` re-resolves via `getTarget`,
                // which returns the *current* docshell's actors.
                if needs_href_fallback(&committed, requested_url)
                    && refresh_probe_console_actor(transport, p)
                {
                    probe_refreshed = true;
                    committed = eval_location_href(transport, &p.console_actor);
                }
                // Still ambiguous (empty — e.g. a torn-down transitional actor
                // reporting noSuchActor — or a literal about:blank mismatch)
                // after the fresh lookup: most likely still on the
                // intermediate docshell. Don't falsely declare the navigation
                // complete — fall through and keep polling (a later probe
                // tick or the events path will observe the real commit)
                // rather than returning a lie.
                if !needs_href_fallback(&committed, requested_url) {
                    let elapsed_ms =
                        u64::try_from(nav_start.elapsed().as_millis()).unwrap_or(u64::MAX);
                    break 'wait CommitInfo {
                        committed_url: committed,
                        ready_state: "complete".to_owned(),
                        elapsed_ms,
                        http_status: doc_status,
                    };
                }
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
    };

    // iter-138 Theme A hardening (live-Firefox finding, not reproducible
    // against the mock): a real localhost round-trip showed the
    // `network-event` resource-available/updated pair for the main document
    // arriving a few ms *after* the docshell's own `dom-complete` — Firefox's
    // netmonitor pipeline and its document-lifecycle pipeline are not
    // synchronized. Trusting `doc_status` the instant the events loop above
    // resolves made `navigate` report a false `status: null` on pages that
    // plainly did have one. Give it a short, bounded grace window to catch up
    // before finalizing — well under the caller's overall timeout, and a
    // no-op for callers that never subscribed to `NetworkEvent`
    // (`back`/`forward`/`reload`, where `doc_status` can never become `Some`
    // no matter how long we wait, so the loop below exits on its first check).
    if commit_info.http_status.is_none() {
        let grace_deadline = Instant::now() + Duration::from_millis(300);
        while doc_status.is_none() && Instant::now() < grace_deadline {
            while let Ok(arc) = rx.try_recv() {
                let _ = extract_document_event(
                    arc.as_ref(),
                    requested_url,
                    &mut doc_resource_id,
                    &mut doc_status,
                );
            }
            if doc_status.is_some() {
                break;
            }
            match transport.recv() {
                Ok(msg) => {
                    bus_arc
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .dispatch_event(&msg);
                }
                Err(ff_rdp_core::ProtocolError::Timeout) => {}
                // A transport error here is not the wait's problem to solve —
                // the commit itself already succeeded — so just stop trying
                // for the status and report whatever was captured (possibly
                // still `None`).
                Err(_) => break,
            }
        }
        commit_info.http_status = doc_status;
    }

    Ok(commit_info)
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
    //
    // iter-138 Theme D: on timeout, report the REAL wall-clock elapsed since
    // `nav_start` rather than echoing back `timeout_ms`. For the `Both`
    // strategy's fallback call (the common case that actually times out in
    // practice), `timeout_ms` here is only the leftover sub-budget *after*
    // the events phase already consumed part of the user's `--timeout` —
    // echoing it under-reported the true wait by ~3x in dogfooding
    // (`--timeout 8000` produced "within 2384ms" against an 8.1s measured
    // wall-clock), leading an agent to under-size a retry.
    match poll_js_condition(
        ctx,
        &console_actor,
        &condition,
        timeout_ms,
        "navigate readystate: JS evaluation error",
        "navigate: document.readyState did not reach 'complete' (with fresh navigation) \
         within its sub-budget — use --no-wait to skip or increase --timeout",
    ) {
        Ok(_) => {}
        Err(AppError::Timeout(_)) => {
            let total_elapsed_ms =
                u64::try_from(nav_start.elapsed().as_millis()).unwrap_or(u64::MAX);
            return Err(AppError::Timeout(format!(
                "navigate: document.readyState did not reach 'complete' (with fresh navigation) \
                 within {total_elapsed_ms}ms — use --no-wait to skip or increase --timeout"
            )));
        }
        Err(e) => return Err(e),
    }

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
        // The readystate poll doesn't subscribe to network events — no
        // status is ever observable from this path (iter-138 Theme A covers
        // only the primary events-based `wait_for_doc_complete` path).
        http_status: None,
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

/// Wait for a navigation triggered by `dispatch` to commit, returning the same
/// `{committed_url, ready_state, elapsed_ms}` envelope `navigate` produces
/// (iter-130 Theme B — shared by `back`, `forward`, `reload` so all four
/// navigation verbs report the same shape).
///
/// `dispatch` MUST send its request as a **raw, un-acked write**
/// (`transport.send(...)`, not `WindowGlobalTarget::reload`/`go_back`/
/// `go_forward`, which route through `actor_request`'s blocking
/// `recv_reply_from`). This function's own `wait_for_doc_complete` pump reads
/// every packet on the wire directly, so a raw dispatch lets it observe both
/// the action's ack AND any `document-event` that races ahead of that ack.
/// Routing the dispatch through `recv_reply_from` instead would risk losing a
/// document-event that arrives before the ack: `recv_reply_from` forwards
/// non-reply packets to the transport's event sink, and no sink is installed
/// on this path, so `forward_event` would silently drop it (see
/// `kb/rdp/actors/watcher.md`'s iter-129 Note 1 — the exact bug this
/// docstring warns against reintroducing).
///
/// `requested_url` feeds `needs_href_fallback`'s literal-`"about:blank"`
/// detection: pass the known target URL when there is one (e.g. reload's
/// pre-reload `location.href`), or `""` when it isn't knowable ahead of time
/// (`back`/`forward`) — `""` never equals the literal `"about:blank"` event
/// value, so the fallback still triggers safely rather than trusting a stale
/// placeholder.
///
/// Mirrors `run_core`'s `Both`-strategy fallback (events wait, then a bounded
/// `document.readyState` poll with whatever budget remains), including its
/// interleaved `ReadyStateProbe`. The probe is not just a "dom-complete never
/// fires" fast-path here — `wait_for_doc_complete`'s `needs_href_fallback`
/// branch can *only* resolve an empty/stale-`about:blank` `dom-complete` URL
/// by calling `eval_location_href` on `probe.console_actor`; without a probe
/// that branch has no actor to eval against, so `href` stays empty forever,
/// `needs_href_fallback` never clears, and the loop `continue`s on every such
/// event until the full `events_budget` elapses — silently defeating the
/// fast path for exactly the SPA/empty-URL cases Theme A/B exist to handle,
/// and reintroducing the class of bug iter-124 already fixed for `navigate`
/// (see the `readystate_probe` comment in `run_core`).
pub(crate) fn wait_for_navigation_commit(
    ctx: &mut super::connect_tab::ConnectedTab,
    cli_timeout: u64,
    requested_url: &str,
    dispatch: impl FnOnce(&mut RdpTransport) -> Result<(), AppError>,
) -> Result<serde_json::Value, AppError> {
    let tab_actor = ctx.target_tab_actor().clone();
    let watcher_actor =
        TabActor::get_watcher(ctx.transport_mut(), &tab_actor).map_err(AppError::from)?;

    // Best-effort freshness epoch, same pattern as run_core's pre_nav_epoch:
    // a failed/exceptional eval disables the freshness guard (0.0) rather
    // than blocking the navigation action.
    let pre_nav_epoch: f64 = {
        let console_actor = ctx.target.console_actor.clone();
        match eval_or_bail(
            ctx,
            &console_actor,
            "performance.timing.navigationStart",
            "nav_action: pre-nav epoch eval",
        ) {
            Ok(result) => match result.result {
                Grip::Value(serde_json::Value::Number(ref n)) => n.as_f64().unwrap_or(0.0),
                _ => 0.0,
            },
            Err(_) => 0.0,
        }
    };

    // `window.location.href` captured before dispatch (iter-138 Themes B/C)
    // — the baseline `probe_same_document_commit` compares against to detect
    // a completed same-document traversal (SPA `popstate`, fragment nav)
    // that never fires a `document-event` at all. Best-effort like
    // `pre_nav_epoch`: an empty string just disables that check rather than
    // blocking the navigation action.
    let pre_nav_href: String = {
        let console_actor = ctx.target.console_actor.clone();
        eval_location_href(ctx.transport_mut(), &console_actor)
    };

    // See run_core's identical prelude for why watchTargets("frame") must
    // precede watchResources — the watcher delivers nothing until both have
    // been issued (iter-79 Theme A).
    WatcherActor::watch_targets(ctx.transport_mut(), &watcher_actor, "frame")
        .map_err(AppError::from)?;

    let bus_arc = ctx.get_or_init_resource_command(watcher_actor.clone());
    let (sub_id, rx) = bus_arc
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .subscribe(ctx.transport_mut(), &[ResourceType::DocumentEvent])
        .map_err(|e| AppError::from(anyhow::anyhow!("document-event subscribe: {e:#}")))?;

    let (_reserved_ms, events_budget) = split_wait_budget(cli_timeout);
    let nav_start = Instant::now();

    // Without this, the `needs_href_fallback` branches inside
    // `wait_for_doc_complete` have no console actor to eval `location.href`
    // against and can never resolve an empty/stale `about:blank`
    // `dom-complete` URL — see this function's own doc comment.
    //
    // `poll_enabled: false` — unlike `run_core`'s identical construction,
    // this probe opts out of the eager dom-loading pre-warm and the periodic
    // `document.readyState` poll (see `ReadyStateProbe::poll_enabled`'s doc
    // comment for why: `back`/`forward`/`reload` don't need the FF152
    // fast-path those exist for, and the eager pre-warm's blocking
    // `getTarget` call carries a real risk of swallowing an already in-flight
    // `dom-complete`). The probe is here purely as an actor source for the
    // need-gated fallback paths.
    //
    // `trust_event_url: false` (iter-138 Theme F) — `back`/`forward`/`reload`
    // never trust a `document-event`'s own `url`, always re-resolving via
    // `eval_location_href` against the top-level tab target instead, because
    // `watchTargets("frame")` above makes Firefox also deliver document
    // events for subframes and a same-tab traversal can restore the
    // top-level document from BFCache (no event of its own) while an
    // unrelated subframe reloads and fires a normal-looking cycle — see
    // `ReadyStateProbe::trust_event_url`'s doc comment for the full story.
    let mut readystate_probe = Some(ReadyStateProbe {
        console_actor: ctx.target.console_actor.clone(),
        tab_actor: &tab_actor,
        pre_epoch: pre_nav_epoch,
        first_probe_at: nav_start + Duration::from_millis(300),
        probe_interval: Duration::from_millis(250),
        poll_enabled: false,
        pre_href: pre_nav_href,
        trust_event_url: false,
    });

    let event_result = dispatch(ctx.transport_mut()).and_then(|()| {
        wait_for_doc_complete(
            ctx.transport_mut(),
            &bus_arc,
            &rx,
            events_budget,
            WaitLevel::Complete,
            nav_start,
            readystate_probe.as_mut(),
            requested_url,
        )
    });

    // Flush any pending `unwatchResources` from dead-channel pruning, then
    // unsubscribe/unwatch regardless of outcome so Firefox cleans up
    // server-side state (mirrors run_core's teardown).
    let _ = bus_arc
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .gc(ctx.transport_mut());
    let _ = bus_arc
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .unsubscribe(ctx.transport_mut(), sub_id);
    let _ = WatcherActor::unwatch_targets(ctx.transport_mut(), &watcher_actor, Some("frame"), None);
    restore_timeout(ctx.transport_mut(), cli_timeout);

    let commit_info = match event_result {
        r @ Ok(_) => r,
        Err(AppError::Timeout(_)) => {
            refresh_console_actor(ctx);
            let elapsed_ms = u64::try_from(nav_start.elapsed().as_millis()).unwrap_or(cli_timeout);
            let remaining = cli_timeout.saturating_sub(elapsed_ms);
            if remaining == 0 {
                Err(AppError::Timeout(
                    "nav_action: no remaining budget for readystate fallback".to_string(),
                ))
            } else {
                wait_for_readystate_complete(ctx, remaining, pre_nav_epoch, nav_start)
            }
        }
        Err(e) => Err(e),
    }?;

    refresh_console_actor(ctx);

    Ok(json!({
        "committed_url": commit_info.committed_url,
        "ready_state": commit_info.ready_state,
        "elapsed_ms": commit_info.elapsed_ms,
    }))
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
) -> Result<(serde_json::Value, bool), AppError> {
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

    // `window.location.href` captured before dispatch (iter-138 Themes B/C)
    // — see `wait_for_navigation_commit`'s identical capture for why: it's
    // the baseline `probe_same_document_commit` needs to detect a same-page
    // fragment navigation, which (like SPA `pushState`/`popstate`) never
    // fires a `document-event` and never advances `navigationStart`.
    let pre_nav_href: String = if wait_opts.no_wait {
        String::new()
    } else {
        let console_actor = ctx.target.console_actor.clone();
        eval_location_href(ctx.transport_mut(), &console_actor)
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

        // iter-138 Theme A, daemon-mode correction: the daemon manages
        // `network-event` watching centrally and does NOT forward it to a
        // client that only issued the generic `watchResources` call above
        // (confirmed live — through the daemon, `status` stayed `null`
        // forever with no amount of extra wait; `document-event` isn't
        // affected because the daemon doesn't intercept it). Real-time
        // delivery requires explicitly asking the daemon to stream, exactly
        // like `navigate --with-network`'s daemon path already does. The
        // `subscribe` call above still matters — it's what makes
        // `dispatch_event` route incoming `network-event` frames (however
        // they arrive) to `rx` at all.
        if ctx.via_daemon {
            crate::daemon::client::start_daemon_stream(ctx.transport_mut(), "network-event")
                .map_err(AppError::from)?;
        }

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
                console_actor: ctx.target.console_actor.clone(),
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
        let event_result = wait_for_doc_complete(
            ctx.transport_mut(),
            &bus_arc,
            &rx,
            events_budget,
            wait_opts.wait_level,
            nav_start,
            readystate_probe.as_mut(),
            url,
        );

        // iter-138 Theme A: stop the daemon stream so it reverts to buffering
        // (best-effort — a failure here doesn't invalidate the navigation
        // result, it just means the daemon stays in streaming mode for
        // `network-event` a little longer than ideal).
        if ctx.via_daemon {
            let _ = crate::daemon::client::stop_daemon_stream(ctx.transport_mut(), "network-event");
        }

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

    // iter-138 Theme A: `status` is always present, defaulting to `null` —
    // consistent with iter-128's always-present-nullable-key convention.
    // Stays `null` under `--no-wait` (no network subscription is ever
    // started) or when the main document's status update simply hadn't
    // arrived by the time the commit wait resolved.
    let mut result = json!({"navigated": url, "status": Value::Null});
    if let Some(ref ci) = commit_info
        && let Some(obj) = result.as_object_mut()
    {
        obj.insert("committed_url".to_string(), json!(ci.committed_url));
        obj.insert("ready_state".to_string(), json!(ci.ready_state));
        obj.insert("elapsed_ms".to_string(), json!(ci.elapsed_ms));
        obj.insert("status".to_string(), json!(ci.http_status));
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
    Ok((result, ctx.via_daemon))
}

/// Run the iter-129 CMP-detection-and-accept flow and merge its result into
/// `result["consent"]`.
///
/// Best-effort by design (see `--auto-consent`'s long_about): a fresh
/// connection is opened (the one `run_core` used has already been dropped)
/// and any failure — connection or protocol — is reported as a stderr
/// warning plus `{"cmp": null, "action": null}` rather than failing the
/// navigate itself. The keys are always present either way, matching
/// `consent accept`'s always-present-key discipline.
fn merge_auto_consent(cli: &Cli, result: &mut Value) {
    let consent = detect_and_accept_best_effort(cli);
    if let Some(obj) = result.as_object_mut() {
        obj.insert("consent".to_owned(), consent);
    }
}

/// How long `--with-network --auto-consent` keeps draining after the consent
/// click, so requests unblocked by the dismissal are part of the same capture.
///
/// Short by design: [`drain_network_events_timed`] returns as soon as the
/// stream goes quiet, so this is a ceiling for a page that keeps loading, not a
/// fixed wait.
const CONSENT_POST_DRAIN_MS: u64 = 8_000;

/// Run the CMP-detection-and-accept flow on its own connection and return the
/// `{cmp, action}` object, never failing the caller.
///
/// Both keys are always present, matching `consent accept`'s discipline; a
/// connection or protocol failure becomes a stderr warning plus two nulls.
///
/// Only for plain `navigate`, whose `run_core` connection is already dropped by
/// the time this runs. `--with-network` must reuse its live connection instead
/// — see [`detect_and_accept_on`].
fn detect_and_accept_best_effort(cli: &Cli) -> Value {
    match connect_and_get_target(cli)
        .and_then(|mut ctx| super::consent::detect_and_accept(&mut ctx))
    {
        Ok(v) => v,
        Err(e) => {
            eprintln!("warning: --auto-consent: consent detection failed: {e}");
            json!({"cmp": null, "action": null})
        }
    }
}

/// [`detect_and_accept_best_effort`] on an **existing** connection.
///
/// iter-159: `--with-network` cannot open a second connection for the consent
/// step. In daemon mode the daemon serialises proxied RPC and this invocation
/// is still holding the slot, so the second connection sat there until the read
/// timeout fired — measured, the flag degraded to `consent detection failed:
/// operation timed out after 10000ms (phase: recv)` on every run. Reusing `ctx`
/// also keeps the resource subscription live across the interaction, so the
/// requests the banner dismissal unblocks are still captured.
fn detect_and_accept_on(ctx: &mut crate::commands::connect_tab::ConnectedTab) -> Value {
    match super::consent::detect_and_accept(ctx) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("warning: --auto-consent: consent detection failed: {e}");
            json!({"cmp": null, "action": null})
        }
    }
}

pub fn run(
    cli: &Cli,
    url: &str,
    wait_opts: &WaitAfterNav<'_>,
    auto_consent: bool,
) -> Result<(), AppError> {
    let (mut result, via_daemon) = run_core(cli, url, wait_opts)?;
    if auto_consent {
        merge_auto_consent(cli, &mut result);
    }
    let mut meta = json!({});
    crate::connection_meta::merge_into_if_verbose(
        &mut meta,
        &cli.host,
        cli.port,
        None,
        cli.is_verbose(),
    );
    // iter-134: always present, not gated by --verbose — matches the
    // `--with-network` variant below, which already got this in iter-128.
    crate::connection_meta::merge_route(&mut meta, via_daemon);
    let envelope = output::envelope(&result, 1, &meta);

    let hint_ctx = HintContext::new(HintSource::Navigate);
    OutputPipeline::from_cli(cli)?.finalize_with_hints(&envelope, Some(&hint_ctx))
}

/// Find the main document's HTTP status among captured network resources
/// (iter-138 Theme G — `navigate --with-network` gets this "for free" since
/// it already captures every request; see Theme A's `wait_for_doc_complete`
/// tracker for the identical `cause_type`/`url` matching rationale).
///
/// Iterates in reverse so a redirect chain's *last* matching resource wins
/// (the one that actually committed), rather than the first (the original,
/// possibly-redirected request).
fn extract_document_status(
    resources: &[ff_rdp_core::NetworkResource],
    updates: &[ff_rdp_core::NetworkResourceUpdate],
    requested_url: &str,
) -> Option<u16> {
    let resource_id = resources
        .iter()
        .rev()
        .find(|r| r.cause_type == "document" && r.url == requested_url)?
        .resource_id;
    // `resources-updated-array` entries are incremental partial updates —
    // Firefox typically carries `status` only on the FIRST update for a
    // resource, with later updates (contentSize, totalTime, ...) leaving it
    // `None`. Taking the single most-recent update record (as `merge_updates`
    // does for *all* fields) would silently lose the status the instant a
    // second update arrives. Instead, take the last update that actually
    // carried a status — mirroring `merge_updates`'s own "last non-None value
    // wins per field" semantics rather than "last record wins overall".
    updates
        .iter()
        .filter(|u| u.resource_id == resource_id)
        .filter_map(|u| u.status.as_deref())
        .next_back()
        .and_then(|s| s.parse::<u16>().ok())
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
///
/// iter-159: `auto_consent` is honoured here too.  `--with-network` and
/// `--auto-consent` used to be mutually exclusive at the clap level, so on any
/// consent-walled site — the exact case where you want both — you had to choose
/// between dismissing the banner and capturing the network. The consent step now
/// runs while capture is still in effect, on the **same** connection (a second
/// one deadlocks against the daemon's RPC serialisation — see
/// [`detect_and_accept_on`]).
///
/// The two paths differ in what reaches *this* envelope. Direct mode owns its
/// watcher subscription, so a short follow-up drain after the click collects the
/// requests the dismissal unblocks. Daemon mode has already stopped its stream
/// by then; the post-consent requests go to the daemon buffer, where
/// `ff-rdp network` reads them, rather than into this result.
pub fn run_with_network(
    cli: &Cli,
    url: &str,
    wait_opts: &WaitAfterNav<'_>,
    network_timeout_ms: u64,
    auto_consent: bool,
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

        // iter-138 Theme G: wall-clock start, so the envelope's `elapsed_ms`
        // matches what plain `navigate` reports rather than being absent.
        let nav_start = Instant::now();

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

        // iter-159: dismiss the consent overlay on this same connection. The
        // requests it unblocks land in the daemon buffer (which never stops
        // buffering) rather than in this envelope — see `run_with_network`'s
        // doc comment for why the two paths differ here.
        let consent = if auto_consent {
            Some(detect_and_accept_on(&mut ctx))
        } else {
            None
        };

        // iter-159: the daemon's residual buffer is deliberately **not** drained
        // here, and the `store-events` push-back that used to follow it is gone.
        //
        // Both existed to paper over a daemon that never buffered its own
        // watcher's events: the drain scooped up whatever landed between the
        // idle cutoff and `stop-stream`, and `store-events` (iter-61j G) pushed
        // this invocation's whole capture back so a later `ff-rdp network`
        // would find something instead of falling through to the Performance
        // API. The daemon now buffers every watcher resource unconditionally,
        // so those events are already there — draining them would consume the
        // buffer this navigation just filled and leave a following `network`
        // with zero rows (measured: `navigate --with-network` then `network
        // --security` returned `results: []`), while re-inserting them on top
        // would duplicate every request.
        //
        // The cost is that a request arriving after the idle cutoff is not in
        // *this* envelope. It is not lost: it is in the daemon buffer, which is
        // what `ff-rdp network` reads.
        //
        // Collapse by `resource_id` anyway — cheap, and the in-flight frames
        // collected by `stop_daemon_stream_draining` can overlap the tail of
        // the stream. Updates need no dedupe: `merge_updates` folds them by
        // `resource_id` with last-write-wins.
        {
            let mut seen = std::collections::HashSet::new();
            all_resources.retain(|r| seen.insert(r.resource_id));
        }

        // The network drain already waited for events to settle; no separate
        // commit-wait is needed. Neterror detection runs via listTabs below.
        //
        // iter-138 Theme G: `committed_url`/`ready_state` are no longer
        // dropped here. Previously `commit_info` was hardcoded `None`
        // because "no separate commit-wait is needed" — true for the wait
        // itself, but it also meant the envelope silently omitted the two
        // fields plain `navigate` always reports, forcing a caller to choose
        // between truthful navigation info and network data. The drain has
        // already settled by this point, so a direct eval is exactly as
        // truthful as the plain path's post-commit reads.
        let doc_status = extract_document_status(&all_resources, &all_updates, url);

        // Theme K: refresh consoleActor after navigate — MUST happen before
        // the eval below: `ctx.target.console_actor` is still bound to the
        // pre-navigation docshell at this point, and evaluating against it
        // would fail with `noSuchActor` on any real cross-document
        // navigation.
        refresh_console_actor(&mut ctx);

        let commit_info: Option<CommitInfo> = {
            let console_actor = ctx.target.console_actor.clone();
            let committed_url = eval_location_href(ctx.transport_mut(), &console_actor);
            let ready_state = eval_document_ready_state(ctx.transport_mut(), &console_actor);
            let elapsed_ms = u64::try_from(nav_start.elapsed().as_millis()).unwrap_or(u64::MAX);
            Some(CommitInfo {
                committed_url,
                ready_state,
                elapsed_ms,
                http_status: doc_status,
            })
        };

        // Detect about:neterror in the daemon --with-network path.
        if let Some(err) = check_real_tab_url_for_neterror(&mut ctx, url) {
            return Err(err);
        }

        let wait_result = wait_after_navigate(&mut ctx, wait_opts)?;
        let wait_for_result = run_wait_for_predicates(&mut ctx, wait_opts)?;

        let update_map = merge_updates(all_updates);
        let network_entries = build_network_entries(&all_resources, &update_map);

        let network_entries = apply_network_controls(cli, &network_entries, timeout_reached);

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
        let mut meta = json!({});
        crate::connection_meta::merge_into_if_verbose(
            &mut meta,
            &cli.host,
            cli.port,
            None,
            cli.is_verbose(),
        );
        // iter-128 Theme D: always present, not gated by --verbose.
        crate::connection_meta::merge_route(&mut meta, ctx.via_daemon);
        let envelope = output::envelope(&result, 1, &meta);
        let hint_ctx = HintContext::new(HintSource::Navigate);
        return OutputPipeline::from_cli(cli)?.finalize_with_hints(&envelope, Some(&hint_ctx));
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
                eprintln!("warning: --auto-consent: post-consent network drain failed: {e}");
            }
        }
        let mut seen = std::collections::HashSet::new();
        all_resources.retain(|r| seen.insert(r.resource_id));
        Some(c)
    } else {
        None
    };

    // iter-138 Theme G/A: extract the main document's status before
    // `merge_updates` consumes `all_updates` by value below.
    let doc_status = extract_document_status(&all_resources, &all_updates, url);

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

    // iter-138 Theme G: the network drain already waited for events to
    // settle, so a direct eval here is exactly as truthful as plain
    // `navigate`'s post-commit reads — no separate commit-wait is needed,
    // but `committed_url`/`ready_state`/`status` are no longer dropped (see
    // the daemon branch above for the full rationale). Neterror detection
    // still runs via listTabs below.
    //
    // Theme K: refresh consoleActor before evaluating — `ctx.target
    // .console_actor` is still bound to the pre-navigation docshell here,
    // and evaluating against it would fail with `noSuchActor` on any real
    // cross-document navigation.
    refresh_console_actor(&mut ctx);

    let commit_info: Option<CommitInfo> = {
        let console_actor = ctx.target.console_actor.clone();
        let committed_url = eval_location_href(ctx.transport_mut(), &console_actor);
        let ready_state = eval_document_ready_state(ctx.transport_mut(), &console_actor);
        let elapsed_ms = u64::try_from(nav_start.elapsed().as_millis()).unwrap_or(u64::MAX);
        Some(CommitInfo {
            committed_url,
            ready_state,
            elapsed_ms,
            http_status: doc_status,
        })
    };

    // Detect about:neterror in the non-daemon --with-network path.
    if let Some(err) = check_real_tab_url_for_neterror(&mut ctx, url) {
        return Err(err);
    }

    let wait_result = wait_after_navigate(&mut ctx, wait_opts)?;
    let wait_for_result = run_wait_for_predicates(&mut ctx, wait_opts)?;

    let network_entries = apply_network_controls(cli, &network_entries, timeout_reached);

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
    let mut meta = json!({});
    crate::connection_meta::merge_into_if_verbose(
        &mut meta,
        &cli.host,
        cli.port,
        None,
        cli.is_verbose(),
    );
    // iter-128 Theme D: always present, not gated by --verbose.
    crate::connection_meta::merge_route(&mut meta, ctx.via_daemon);
    let envelope = output::envelope(&result, 1, &meta);

    let hint_ctx = HintContext::new(HintSource::Navigate);
    OutputPipeline::from_cli(cli)?.finalize_with_hints(&envelope, Some(&hint_ctx))
}

/// Apply output controls (sort, limit, fields) to network entries from navigate.
///
/// Iteration 126: this always returns the ONE canonical network object built by
/// [`super::network::build_canonical_network`] —
/// `{entries, shown, total, truncated, total_requests, total_transfer_bytes,
/// by_cause_type, slowest, timeout_reached, ...}` — on every path (busy or
/// quiet page, detail or summary mode, `--all` or default). Previously this
/// flipped between a bare array (quiet/`--all`), a truncation object (busy), and
/// a summary object (non-detail), so `.results.network.entries` and
/// `.results.network.total_requests` threw `cannot index array` half the time.
///
/// In detail mode (`--detail`/`--jq`/`--sort`/`--limit`/`--fields`/`--all`) the
/// `entries` list is sorted, capped at 20 (unless `--all`), and field-projected;
/// in summary mode `entries` carries the full unsorted capture. Summary fields
/// (`total_requests`, …) always reflect the full capture regardless of the view.
///
/// `timeout_reached` is forwarded to [`super::network::build_network_summary`]
/// so the object carries the hint field when the collection deadline fired while
/// events were still arriving.
fn apply_network_controls(
    cli: &Cli,
    network_entries: &[serde_json::Value],
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
        let mut detail = network_entries.to_vec();
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
        let shown = limited.len();
        // Summary fields are computed from the FULL capture (`network_entries`),
        // never the truncated/field-projected `limited` view.
        super::network::build_canonical_network(
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
        let total = network_entries.len();
        super::network::build_canonical_network(
            network_entries.to_vec(),
            total,
            total,
            false,
            network_entries,
            timeout_reached,
        )
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
            "https://example.com/",
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
            "https://example.com/",
        );

        probe_handle.join().unwrap();
        server_handle.join().unwrap();

        assert!(
            lock_acquired.load(Ordering::Relaxed),
            "navigate_bus_lock_released_during_wait: second thread could not acquire \
             the bus lock while wait_for_doc_complete was running — lock held too long"
        );
    }

    /// Answer one `getTarget` round-trip on `writer`/`reader` with a `frame`
    /// response carrying `console_actor`.
    ///
    /// The `wait_for_doc_complete` probe refresh (iter-124) issues this call
    /// as soon as `dom-loading` commits (or lazily before its first probe
    /// attempt), so any mock server driving a `probe: Some(..)` case must
    /// answer it before the eval round-trips it gates.
    fn answer_get_target(
        reader: &mut std::io::BufReader<std::net::TcpStream>,
        writer: &mut std::net::TcpStream,
        tab_actor: &str,
        console_actor: &str,
    ) {
        use std::io::Write as _;

        use ff_rdp_core::transport::{encode_frame, recv_from};

        let _req = recv_from(reader).unwrap();
        let response = serde_json::json!({
            "from": tab_actor,
            "frame": { "actor": "conn0/target1", "consoleActor": console_actor },
        });
        writer
            .write_all(encode_frame(&serde_json::to_string(&response).unwrap()).as_bytes())
            .unwrap();
    }

    /// Answer one `getTarget` round-trip on `writer`/`reader` with an
    /// actor-level error reply (`noSuchActor`), matching the wire shape
    /// `recv_reply_from` parses into `ProtocolError::ActorError`.
    ///
    /// Used by the iter-124 review-fix unit tests to prove a transient (or
    /// persistent) `getTarget` failure does not permanently strand the probe
    /// on its stale console actor.
    fn answer_get_target_error(
        reader: &mut std::io::BufReader<std::net::TcpStream>,
        writer: &mut std::net::TcpStream,
        tab_actor: &str,
    ) {
        use std::io::Write as _;

        use ff_rdp_core::transport::{encode_frame, recv_from};

        let _req = recv_from(reader).unwrap();
        let response = serde_json::json!({
            "from": tab_actor,
            "error": "noSuchActor",
            "message": "No such actor for ID: conn0/tabDescriptor1",
        });
        writer
            .write_all(encode_frame(&serde_json::to_string(&response).unwrap()).as_bytes())
            .unwrap();
    }

    /// Answer one `evaluateJSAsync` round-trip on `writer`/`reader` with an
    /// immediate actor-level error reply (`noSuchActor`), simulating an eval
    /// sent to a stale (already-invalidated) console actor. Returns the
    /// `text` the client asked to evaluate so the caller can assert on it.
    ///
    /// Used by the iter-124 review-fix tests: when a probe-timer tick's
    /// `getTarget` refresh fails, `wait_for_doc_complete` still attempts
    /// `probe_readystate_complete` with the (still-stale) actor before
    /// looping — this answers that attempt so the mock server's expected
    /// request sequence matches the real code path exactly.
    fn answer_one_eval_error(
        reader: &mut std::io::BufReader<std::net::TcpStream>,
        writer: &mut std::net::TcpStream,
        console_actor: &str,
    ) -> String {
        use std::io::Write as _;

        use ff_rdp_core::transport::{encode_frame, recv_from};

        let req = recv_from(reader).unwrap();
        let text = req["text"].as_str().unwrap_or_default().to_owned();
        let error = serde_json::json!({
            "from": console_actor,
            "error": "noSuchActor",
            "message": format!("No such actor for ID: {console_actor}"),
        });
        writer
            .write_all(encode_frame(&serde_json::to_string(&error).unwrap()).as_bytes())
            .unwrap();
        text
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

        let tab_actor = "conn0/tabDescriptor1";

        // Mock Firefox: greeting, then answer the probe's lazy console-actor
        // refresh (getTarget — no dom-loading event arrives to trigger the
        // earlier refresh, iter-124) followed by two eval round-trips — the
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

            answer_get_target(&mut reader, &mut writer, tab_actor, console_actor);

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
        let tab = ff_rdp_core::ActorId::from(tab_actor);

        let nav_start = Instant::now();
        let mut probe = ReadyStateProbe {
            // Deliberately stale — the pre-navigation actor — to prove the
            // refresh (via `tab_actor`) is what makes the probe usable.
            console_actor: ff_rdp_core::ActorId::from("conn0/stale-console"),
            tab_actor: &tab,
            pre_epoch: 0.0,
            // Probe almost immediately so the test does not wait 300 ms.
            first_probe_at: nav_start,
            probe_interval: Duration::from_millis(50),
            poll_enabled: true,
            pre_href: String::new(),
            trust_event_url: true,
        };

        // Generous events budget: the probe must return long before this.
        let result = wait_for_doc_complete(
            &mut transport,
            &bus_arc,
            &rx,
            5_000,
            WaitLevel::Complete,
            nav_start,
            Some(&mut probe),
            "https://example.com/",
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

    /// iter-124 review fix: `unit_navigate_probe_refresh_retries_after_transient_error`
    ///
    /// A `getTarget` failure (e.g. `noSuchActor` because the new docshell
    /// hasn't finished registering server-side yet) must NOT permanently
    /// latch `probe_refreshed` — the review found the original fix set the
    /// latch unconditionally, so one failed attempt stranded the probe on
    /// its stale actor for the rest of the wait, intermittently
    /// reintroducing the exact bug this PR fixes. This test drives the mock
    /// server through: error reply, then a successful `getTarget` reply on
    /// the *next* probe-timer tick, then the readyState + location.href
    /// evals — proving the probe recovers instead of staying stuck.
    #[test]
    fn unit_navigate_probe_refresh_retries_after_transient_error() {
        use std::io::Write as _;
        use std::net::TcpListener;

        use ff_rdp_core::transport::{RdpTransport, encode_frame};

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let console_actor = "conn0/console1";
        let tab_actor = "conn0/tabDescriptor1";

        // Mock Firefox: greeting, then a FAILED getTarget (noSuchActor) on the
        // first probe tick. `wait_for_doc_complete` still attempts
        // `probe_readystate_complete` with the stale actor after a failed
        // refresh (best-effort — see `refresh_probe_console_actor`'s doc
        // comment), so that stale-actor eval also errors before the loop
        // re-arms and tries again on the SECOND tick: a SUCCESSFUL getTarget,
        // then the two evals the now-fresh probe needs to short-circuit.
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

            answer_get_target_error(&mut reader, &mut writer, tab_actor);
            let stale_eval_text =
                answer_one_eval_error(&mut reader, &mut writer, "conn0/stale-console");

            answer_get_target(&mut reader, &mut writer, tab_actor, console_actor);

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
                &serde_json::json!("https://retry.example/"),
            );
            (stale_eval_text, ready_text, href_text)
        });

        let mut transport =
            RdpTransport::connect("127.0.0.1", port, Duration::from_secs(5)).unwrap();

        // Empty channel — no document-event ever arrives, so the probe-timer
        // loop (with its refresh-retry) is the only way out other than the
        // timeout.
        let (tx, rx) = std::sync::mpsc::channel::<std::sync::Arc<Resource>>();
        drop(tx);

        let watcher_actor = ff_rdp_core::ActorId::from("conn0/watcher1");
        let bus_arc = Arc::new(Mutex::new(ResourceCommand::new(watcher_actor)));
        let tab = ff_rdp_core::ActorId::from(tab_actor);

        let nav_start = Instant::now();
        let mut probe = ReadyStateProbe {
            console_actor: ff_rdp_core::ActorId::from("conn0/stale-console"),
            tab_actor: &tab,
            pre_epoch: 0.0,
            // Probe almost immediately, then again after a short interval so
            // the second (successful) getTarget attempt happens quickly.
            first_probe_at: nav_start,
            probe_interval: Duration::from_millis(50),
            poll_enabled: true,
            pre_href: String::new(),
            trust_event_url: true,
        };

        let result = wait_for_doc_complete(
            &mut transport,
            &bus_arc,
            &rx,
            5_000,
            WaitLevel::Complete,
            nav_start,
            Some(&mut probe),
            "https://retry.example/",
        );

        let (stale_eval_text, ready_text, href_text) = server_handle.join().unwrap();

        let ci = result.expect(
            "probe must recover after a transient getTarget failure and \
             short-circuit to a CommitInfo on the retry",
        );
        assert_eq!(ci.ready_state, "complete");
        assert_eq!(
            ci.committed_url, "https://retry.example/",
            "committed_url must come from the post-recovery location.href fetch"
        );
        assert!(
            stale_eval_text.contains("readyState"),
            "the best-effort eval attempted with the still-stale actor (after \
             the failed refresh) should be the readyState probe, got: {stale_eval_text}"
        );
        assert!(
            ready_text.contains("readyState"),
            "the eval after recovery should be the readyState probe, got: {ready_text}"
        );
        assert!(
            href_text.contains("location.href"),
            "the final eval should be the location.href fetch, got: {href_text}"
        );
        assert!(
            nav_start.elapsed() < Duration::from_secs(4),
            "recovery must happen well inside the events budget; took {:?}",
            nav_start.elapsed()
        );
    }

    /// iter-124 review fix: `unit_navigate_probe_refresh_persistent_error_falls_back_to_timeout`
    ///
    /// When `getTarget` fails on every attempt (the docshell genuinely never
    /// becomes queryable during the wait), the probe must keep retrying
    /// without panicking and `wait_for_doc_complete` must fall through
    /// cleanly to the events-budget timeout — never a crash, never a hang
    /// past the deadline.
    #[test]
    fn unit_navigate_probe_refresh_persistent_error_falls_back_to_timeout() {
        use std::io::Write as _;
        use std::net::TcpListener;

        use ff_rdp_core::transport::{RdpTransport, encode_frame};

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let tab_actor = "conn0/tabDescriptor1";

        // Mock Firefox: greeting, then answer every getTarget-or-eval request
        // with an actor error for as long as the client keeps asking. The
        // client is expected to give up and close its socket once its
        // timeout budget is exhausted (well before the bounded 50-iteration
        // cap below) — that's a normal end condition for this test, not a
        // server bug, so the loop stops quietly on a read/write failure
        // instead of unwrapping and panicking the (detached, unjoined)
        // server thread.
        let server_handle = std::thread::spawn(move || {
            use ff_rdp_core::transport::recv_from;

            let (stream, _) = listener.accept().unwrap();
            let mut writer = stream.try_clone().unwrap();
            let mut reader = std::io::BufReader::new(stream);

            let greeting = serde_json::json!({
                "from": "root", "applicationType": "browser", "traits": {}
            });
            writer
                .write_all(encode_frame(&serde_json::to_string(&greeting).unwrap()).as_bytes())
                .unwrap();

            for _ in 0..50 {
                let Ok(_req) = recv_from(&mut reader) else {
                    break;
                };
                let error = serde_json::json!({
                    "from": tab_actor,
                    "error": "noSuchActor",
                    "message": "No such actor for ID: conn0/tabDescriptor1",
                });
                if writer
                    .write_all(encode_frame(&serde_json::to_string(&error).unwrap()).as_bytes())
                    .is_err()
                {
                    break;
                }
            }
        });

        let mut transport =
            RdpTransport::connect("127.0.0.1", port, Duration::from_secs(5)).unwrap();

        // Empty channel — no document-event ever arrives, so with every
        // refresh attempt also failing, the only way out is the timeout.
        let (tx, rx) = std::sync::mpsc::channel::<std::sync::Arc<Resource>>();
        drop(tx);

        let watcher_actor = ff_rdp_core::ActorId::from("conn0/watcher1");
        let bus_arc = Arc::new(Mutex::new(ResourceCommand::new(watcher_actor)));
        let tab = ff_rdp_core::ActorId::from(tab_actor);

        let nav_start = Instant::now();
        let mut probe = ReadyStateProbe {
            console_actor: ff_rdp_core::ActorId::from("conn0/stale-console"),
            tab_actor: &tab,
            pre_epoch: 0.0,
            first_probe_at: nav_start,
            // Short interval + short overall budget below so a persistently
            // failing refresh still exercises several retries without
            // making this test slow.
            probe_interval: Duration::from_millis(50),
            poll_enabled: true,
            pre_href: String::new(),
            trust_event_url: true,
        };

        let budget_ms = 800;
        let result = wait_for_doc_complete(
            &mut transport,
            &bus_arc,
            &rx,
            budget_ms,
            WaitLevel::Complete,
            nav_start,
            Some(&mut probe),
            "https://example.com/",
        );

        // The server thread's loop is bounded (50 iterations) purely so it
        // cannot hang the test process if something unexpected happens; the
        // assertion under test is on `result`/timing, not on the server
        // thread's join (which may still be mid-loop when the client times
        // out and stops asking).
        drop(server_handle);

        match result {
            Err(AppError::Timeout(msg)) => {
                assert!(
                    msg.contains("dom-complete"),
                    "timeout message should name the awaited event: {msg}"
                );
            }
            other => panic!(
                "persistent getTarget failure must fall through to a clean \
                 Timeout, not panic or hang; got {other:?}"
            ),
        }
        assert!(
            nav_start.elapsed() < Duration::from_secs(3),
            "must not overrun the {budget_ms}ms budget by more than the poll \
             interval; took {:?}",
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
        let tab_actor = "conn0/tabDescriptor1";

        // Server: greeting, then the dom-loading-triggered getTarget refresh
        // (iter-124), then a single location.href eval answer (the empty
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

            answer_get_target(&mut reader, &mut writer, tab_actor, console_actor);

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
        let tab = ff_rdp_core::ActorId::from(tab_actor);

        // No probe timer needed — the empty dom-complete triggers the fallback
        // — but a probe must be present so the console_actor is available (and
        // gets refreshed to `console_actor` on the dom-loading event above).
        let nav_start = Instant::now();
        let mut probe = ReadyStateProbe {
            console_actor: ff_rdp_core::ActorId::from("conn0/stale-console"),
            tab_actor: &tab,
            pre_epoch: 0.0,
            // Push the probe far into the future so only the dom-complete
            // fallback path (not the interleaved probe) fires.
            first_probe_at: nav_start + Duration::from_secs(30),
            probe_interval: Duration::from_secs(30),
            poll_enabled: true,
            pre_href: String::new(),
            trust_event_url: true,
        };

        let result = wait_for_doc_complete(
            &mut transport,
            &bus_arc,
            &rx,
            5_000,
            WaitLevel::Complete,
            nav_start,
            Some(&mut probe),
            "https://spa.example/app",
        );

        server_handle.join().unwrap();

        let ci = result.expect("dom-complete should resolve to a CommitInfo");
        assert_eq!(ci.ready_state, "complete");
        assert_eq!(
            ci.committed_url, "https://spa.example/app",
            "empty dom-complete URL must fall back to location.href, not about:blank"
        );
    }

    /// iter-122 review fix: `unit_navigate_probe_ignored_for_non_complete_wait_level`
    ///
    /// The interleaved readystate probe (Theme A) can only ever observe
    /// `document.readyState === 'complete'`. When the caller asked for
    /// `--wait loading` (or `--wait interactive`), the probe must NOT be
    /// honored even if it fires and reports truthy — only the matching
    /// `dom-loading` event may resolve the wait, with the correct
    /// `ready_state: "loading"` and its own Theme B URL fallback.
    #[test]
    fn unit_navigate_probe_ignored_for_non_complete_wait_level() {
        use std::io::Write as _;
        use std::net::TcpListener;

        use ff_rdp_core::transport::{RdpTransport, encode_frame};

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let console_actor = "conn0/console1";
        let tab_actor = "conn0/tabDescriptor1";

        // Server: greeting, then the dom-loading-triggered getTarget refresh
        // (iter-124), then a single location.href eval answer used by the
        // dom-loading fallback. If the probe were (incorrectly) honored first it
        // would ask for `readyState` instead — asserted on below.
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

            answer_get_target(&mut reader, &mut writer, tab_actor, console_actor);

            answer_one_eval(
                &mut reader,
                &mut writer,
                console_actor,
                &serde_json::json!("https://spa.example/loading"),
            )
        });

        let mut transport =
            RdpTransport::connect("127.0.0.1", port, Duration::from_secs(5)).unwrap();

        // A dom-loading event with an empty URL arrives after a short delay —
        // long enough for an (incorrectly) armed probe to have fired first.
        let (tx, rx) = std::sync::mpsc::channel::<std::sync::Arc<Resource>>();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(120));
            tx.send(std::sync::Arc::new(Resource::DocumentEvent(
                serde_json::json!({ "name": "dom-loading", "url": "" }),
            )))
            .unwrap();
        });

        let watcher_actor = ff_rdp_core::ActorId::from("conn0/watcher1");
        let bus_arc = Arc::new(Mutex::new(ResourceCommand::new(watcher_actor)));
        let tab = ff_rdp_core::ActorId::from(tab_actor);

        let nav_start = Instant::now();
        // Probe is armed to fire almost immediately and would report `true`
        // for `document.readyState` if it were ever evaluated — but it must
        // stay silent because wait_level is `Loading`, not `Complete`.
        let mut probe = ReadyStateProbe {
            console_actor: ff_rdp_core::ActorId::from("conn0/stale-console"),
            tab_actor: &tab,
            pre_epoch: 0.0,
            first_probe_at: nav_start,
            probe_interval: Duration::from_millis(10),
            poll_enabled: true,
            pre_href: String::new(),
            trust_event_url: true,
        };

        let result = wait_for_doc_complete(
            &mut transport,
            &bus_arc,
            &rx,
            5_000,
            WaitLevel::Loading,
            nav_start,
            Some(&mut probe),
            "https://spa.example/loading",
        );

        let href_text = server_handle.join().unwrap();

        let ci = result.expect("dom-loading should resolve to a CommitInfo");
        assert_eq!(
            ci.ready_state, "loading",
            "probe must not override the requested wait_level with 'complete'"
        );
        assert_eq!(
            ci.committed_url, "https://spa.example/loading",
            "Loading path must apply the same location.href fallback as Interactive/Complete"
        );
        assert!(
            href_text.contains("location.href"),
            "the only eval the mock server should see is the dom-loading URL fallback, got: {href_text}"
        );
    }

    // ── iter-130 Theme A: literal "about:blank" fallback ───────────────────

    #[test]
    fn unit_needs_href_fallback_covers_empty_and_stale_about_blank() {
        // Empty candidate: always needs the fallback (iter-122 Theme B, unchanged).
        assert!(needs_href_fallback("", "https://example.com/"));
        // Literal "about:blank" while a different URL was requested: the
        // comparis.ch SPA route-commit case — needs the fallback.
        assert!(needs_href_fallback(
            "about:blank",
            "https://www.comparis.ch/hypotheken"
        ));
        // A real URL that matches or differs from the request: never needs
        // the fallback — the event's own URL is trustworthy.
        assert!(!needs_href_fallback(
            "https://example.com/",
            "https://example.com/"
        ));
        assert!(!needs_href_fallback(
            "https://example.com/other",
            "https://example.com/"
        ));
        // A genuine navigation TO about:blank must not trigger a spurious
        // fallback round-trip — the requested and committed URLs agree.
        assert!(!needs_href_fallback("about:blank", "about:blank"));
    }

    // ── iter-138 Theme F: must_reresolve_href ──────────────────────────────

    fn probe_with_trust(trust_event_url: bool) -> (ff_rdp_core::ActorId, ReadyStateProbe<'static>) {
        // `tab_actor` needs a stable address for the probe's lifetime; leak a
        // tiny `ActorId` for the test (fine — unit tests are short-lived).
        let tab: &'static ff_rdp_core::ActorId =
            Box::leak(Box::new(ff_rdp_core::ActorId::from("conn0/tabDescriptor1")));
        let probe = ReadyStateProbe {
            console_actor: ff_rdp_core::ActorId::from("conn0/console1"),
            tab_actor: tab,
            pre_epoch: 0.0,
            first_probe_at: Instant::now(),
            probe_interval: Duration::from_millis(50),
            poll_enabled: false,
            pre_href: String::new(),
            trust_event_url,
        };
        (tab.clone(), probe)
    }

    /// `unit_must_reresolve_href_no_probe_defers_to_needs_href_fallback` — with
    /// no probe at all (the `Events`-strategy `navigate` case), the decision
    /// collapses to the pre-existing `needs_href_fallback` check.
    #[test]
    fn unit_must_reresolve_href_no_probe_defers_to_needs_href_fallback() {
        assert!(!must_reresolve_href(
            None,
            "https://example.com/",
            "https://example.com/"
        ));
        assert!(must_reresolve_href(None, "", "https://example.com/"));
    }

    /// `unit_must_reresolve_href_trusted_probe_defers_to_needs_href_fallback`
    /// — `trust_event_url: true` (plain `navigate`) behaves exactly like the
    /// no-probe case: a well-formed candidate is trusted verbatim.
    #[test]
    fn unit_must_reresolve_href_trusted_probe_defers_to_needs_href_fallback() {
        let (_tab, probe) = probe_with_trust(true);
        assert!(!must_reresolve_href(
            Some(&probe),
            "https://example.com/",
            "https://example.com/"
        ));
        assert!(must_reresolve_href(
            Some(&probe),
            "about:blank",
            "https://example.com/"
        ));
    }

    /// `unit_must_reresolve_href_untrusted_probe_always_reresolves` — iter-138
    /// Theme F: `trust_event_url: false` (`back`/`forward`/`reload`) forces
    /// re-resolution even for a perfectly well-formed candidate URL, because
    /// it might be a subframe's.
    #[test]
    fn unit_must_reresolve_href_untrusted_probe_always_reresolves() {
        let (_tab, probe) = probe_with_trust(false);
        assert!(
            must_reresolve_href(
                Some(&probe),
                "https://cdn.optimizely.com/client_storage/x.html",
                "https://example.com/"
            ),
            "a well-formed but wrong (subframe) URL must not be trusted when \
             trust_event_url is false"
        );
        assert!(must_reresolve_href(
            Some(&probe),
            "https://example.com/",
            "https://example.com/"
        ));
    }

    // ── iter-138 Theme A/G: extract_document_status ────────────────────────

    fn doc_resource(url: &str, resource_id: u64) -> ff_rdp_core::NetworkResource {
        ff_rdp_core::NetworkResource {
            actor: ff_rdp_core::ActorId::from(format!("conn0/netEvent{resource_id}")),
            method: "GET".to_owned(),
            url: url.to_owned(),
            is_xhr: false,
            cause_type: "document".to_owned(),
            started_date_time: "2026-01-01T00:00:00Z".to_owned(),
            timestamp: 0.0,
            resource_id,
        }
    }

    fn status_update(resource_id: u64, status: &str) -> ff_rdp_core::NetworkResourceUpdate {
        ff_rdp_core::NetworkResourceUpdate {
            resource_id,
            status: Some(status.to_owned()),
            ..Default::default()
        }
    }

    /// `unit_extract_document_status_matches_cause_and_url` — the AC-facing
    /// happy path: one `document`-cause resource matching the requested URL,
    /// with a status update.
    #[test]
    fn unit_extract_document_status_matches_cause_and_url() {
        let resources = vec![doc_resource("https://example.com/404", 1)];
        let updates = vec![status_update(1, "404")];
        assert_eq!(
            extract_document_status(&resources, &updates, "https://example.com/404"),
            Some(404)
        );
    }

    /// `unit_extract_document_status_ignores_subframe_document_resource` —
    /// iter-138 Theme A hardening: a subframe's own `document`-cause request
    /// (same cause type, different URL) must not be mistaken for the page's
    /// own navigation — the same contamination risk Theme F guards against
    /// for `document-event`.
    #[test]
    fn unit_extract_document_status_ignores_subframe_document_resource() {
        let resources = vec![
            doc_resource("https://cdn.example.com/iframe.html", 1),
            doc_resource("https://example.com/page", 2),
        ];
        let updates = vec![status_update(1, "200"), status_update(2, "503")];
        assert_eq!(
            extract_document_status(&resources, &updates, "https://example.com/page"),
            Some(503),
            "must report the requested URL's own status, not the subframe's"
        );
    }

    /// `unit_extract_document_status_none_when_no_document_resource_matches`
    /// — no matching resource (e.g. `--no-wait`, or the status update hadn't
    /// arrived yet) reports `None`, which the caller surfaces as JSON `null`.
    #[test]
    fn unit_extract_document_status_none_when_no_document_resource_matches() {
        let resources = vec![doc_resource("https://example.com/other", 1)];
        let updates = vec![status_update(1, "200")];
        assert_eq!(
            extract_document_status(&resources, &updates, "https://example.com/page"),
            None
        );
        assert_eq!(
            extract_document_status(&[], &[], "https://example.com/page"),
            None
        );
    }

    /// `unit_extract_document_status_prefers_last_match_on_redirect` — a
    /// redirect chain can produce more than one `document`-cause resource for
    /// the same requested URL; the LAST one (the hop that actually
    /// committed) wins over the first (the original, redirected request).
    #[test]
    fn unit_extract_document_status_prefers_last_match_on_redirect() {
        let resources = vec![
            doc_resource("https://example.com/page", 1),
            doc_resource("https://example.com/page", 2),
        ];
        let updates = vec![status_update(1, "302"), status_update(2, "200")];
        assert_eq!(
            extract_document_status(&resources, &updates, "https://example.com/page"),
            Some(200)
        );
    }

    /// `unit_extract_document_status_survives_later_update_without_status` —
    /// regression guard for a live-Firefox-only bug (not reproducible against
    /// any mock): `resources-updated-array` entries are incremental partial
    /// updates, and Firefox carries `status` only on the FIRST update for a
    /// resource — a SECOND update (e.g. carrying `totalTime`/`contentSize`)
    /// leaves `status: None`. Naively taking "the single most-recent update
    /// record" instead of "the most recent value seen per field" silently
    /// turned a real 200/404 into `null` the instant a second update arrived
    /// — which real Firefox traffic always produces. Caught by
    /// `live_138_with_network_keeps_envelope`, not by any prior unit test.
    #[test]
    fn unit_extract_document_status_survives_later_update_without_status() {
        let resources = vec![doc_resource("https://example.com/page", 1)];
        let updates = vec![
            status_update(1, "200"),
            ff_rdp_core::NetworkResourceUpdate {
                resource_id: 1,
                total_time: Some(45),
                ..Default::default()
            },
        ];
        assert_eq!(
            extract_document_status(&resources, &updates, "https://example.com/page"),
            Some(200),
            "a later update that doesn't carry `status` must not erase an \
             earlier one that did"
        );
    }

    // ── iter-138 Theme B/C: probe_same_document_commit ──────────────────────

    /// `unit_probe_same_document_commit_returns_new_href_when_changed_and_complete`
    /// — the core same-document detection: `document.readyState === 'complete'
    /// && location.href !== pre_href` resolving truthy returns the new href.
    #[test]
    fn unit_probe_same_document_commit_returns_new_href_when_changed_and_complete() {
        use std::io::Write as _;
        use std::net::TcpListener;

        use ff_rdp_core::transport::{RdpTransport, encode_frame};

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let console_actor = "conn0/console1";

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
                &serde_json::json!("https://example.com/route1"),
            )
        });

        let mut transport =
            RdpTransport::connect("127.0.0.1", port, Duration::from_secs(5)).unwrap();
        let console_actor = ff_rdp_core::ActorId::from(console_actor);

        let result =
            probe_same_document_commit(&mut transport, &console_actor, "https://example.com/page");

        let eval_text = server_handle.join().unwrap();
        assert!(
            eval_text.contains("readyState") && eval_text.contains("location.href"),
            "condition must check both readyState and location.href: {eval_text}"
        );
        assert_eq!(result, Some("https://example.com/route1".to_owned()));
    }

    /// `unit_probe_same_document_commit_returns_none_when_href_unchanged` — a
    /// `null` result (the IIFE's own signal for "not yet") is treated as "not
    /// a same-document commit", not an error.
    #[test]
    fn unit_probe_same_document_commit_returns_none_when_href_unchanged() {
        use std::io::Write as _;
        use std::net::TcpListener;

        use ff_rdp_core::transport::{RdpTransport, encode_frame};

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let console_actor = "conn0/console1";

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
                &serde_json::Value::Null,
            )
        });

        let mut transport =
            RdpTransport::connect("127.0.0.1", port, Duration::from_secs(5)).unwrap();
        let console_actor = ff_rdp_core::ActorId::from(console_actor);

        let result =
            probe_same_document_commit(&mut transport, &console_actor, "https://example.com/page");
        server_handle.join().unwrap();
        assert_eq!(result, None);
    }

    /// `unit_probe_same_document_commit_empty_pre_href_returns_none` — an
    /// empty baseline (the pre-navigation `location.href` eval itself failed)
    /// disables the check entirely without attempting an eval round-trip.
    #[test]
    fn unit_probe_same_document_commit_empty_pre_href_returns_none() {
        use std::io::Write as _;
        use std::net::TcpListener;

        use ff_rdp_core::transport::{RdpTransport, encode_frame};

        // The server sends only the greeting `connect()` requires, then never
        // answers anything else — if the function attempted an eval
        // round-trip despite the empty `pre_href`, this would block until
        // the transport's read timeout fires (proving the assertion below
        // wrong: the fast path must return well inside it).
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server_handle = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut writer = stream;
            let greeting = serde_json::json!({
                "from": "root", "applicationType": "browser", "traits": {}
            });
            writer
                .write_all(encode_frame(&serde_json::to_string(&greeting).unwrap()).as_bytes())
                .unwrap();
        });

        let mut transport =
            RdpTransport::connect("127.0.0.1", port, Duration::from_secs(5)).unwrap();
        transport
            .set_read_timeout(Some(Duration::from_millis(50)))
            .unwrap();
        let console_actor = ff_rdp_core::ActorId::from("conn0/console1");

        let started = Instant::now();
        let result = probe_same_document_commit(&mut transport, &console_actor, "");
        let elapsed = started.elapsed();

        assert_eq!(result, None);
        assert!(
            elapsed < Duration::from_millis(40),
            "empty pre_href must return immediately without an eval round-trip, took {elapsed:?}"
        );
        drop(transport);
        let _ = server_handle.join();
    }

    /// iter-130 Theme A: `unit_navigate_dom_complete_literal_about_blank_falls_back_to_href`
    ///
    /// Reproduces the comparis.ch SPA route-commit finding (dogfooding-session-61
    /// #5): the `dom-complete` document-event's `url` field is the literal string
    /// `"about:blank"` even though the real requested URL has genuinely landed
    /// (`ready_state: complete`, and a manual `eval location.href` confirms it).
    /// `committed_url` must be resolved from `location.href`, not surfaced as a
    /// literal `about:blank` that would make a caller think navigation failed.
    #[test]
    fn unit_navigate_dom_complete_literal_about_blank_falls_back_to_href() {
        use std::io::Write as _;
        use std::net::TcpListener;

        use ff_rdp_core::transport::{RdpTransport, encode_frame};

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let console_actor = "conn0/console1";
        let tab_actor = "conn0/tabDescriptor1";

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

            answer_get_target(&mut reader, &mut writer, tab_actor, console_actor);

            answer_one_eval(
                &mut reader,
                &mut writer,
                console_actor,
                &serde_json::json!("https://www.comparis.ch/hypotheken"),
            )
        });

        let mut transport =
            RdpTransport::connect("127.0.0.1", port, Duration::from_secs(5)).unwrap();

        // dom-loading commits with the initial "about:blank" placeholder, then
        // dom-complete fires — still reporting the literal "about:blank" — even
        // though the real page has landed (this is the exact shape observed on
        // comparis.ch route commits).
        let (tx, rx) = std::sync::mpsc::channel::<std::sync::Arc<Resource>>();
        tx.send(std::sync::Arc::new(Resource::DocumentEvent(
            serde_json::json!({ "name": "dom-loading", "url": "about:blank" }),
        )))
        .unwrap();
        tx.send(std::sync::Arc::new(Resource::DocumentEvent(
            serde_json::json!({ "name": "dom-complete", "url": "about:blank" }),
        )))
        .unwrap();
        drop(tx);

        let watcher_actor = ff_rdp_core::ActorId::from("conn0/watcher1");
        let bus_arc = Arc::new(Mutex::new(ResourceCommand::new(watcher_actor)));
        let tab = ff_rdp_core::ActorId::from(tab_actor);

        let nav_start = Instant::now();
        let mut probe = ReadyStateProbe {
            console_actor: ff_rdp_core::ActorId::from("conn0/stale-console"),
            tab_actor: &tab,
            pre_epoch: 0.0,
            // Push the probe far into the future — only the dom-complete
            // fallback path should fire, not the interleaved probe.
            first_probe_at: nav_start + Duration::from_secs(30),
            probe_interval: Duration::from_secs(30),
            poll_enabled: true,
            pre_href: String::new(),
            trust_event_url: true,
        };

        let result = wait_for_doc_complete(
            &mut transport,
            &bus_arc,
            &rx,
            5_000,
            WaitLevel::Complete,
            nav_start,
            Some(&mut probe),
            "https://www.comparis.ch/hypotheken",
        );

        server_handle.join().unwrap();

        let ci = result.expect("dom-complete should resolve to a CommitInfo");
        assert_eq!(ci.ready_state, "complete");
        assert_eq!(
            ci.committed_url, "https://www.comparis.ch/hypotheken",
            "a literal about:blank dom-complete URL must fall back to location.href \
             when it does not match the requested URL"
        );
    }
}
