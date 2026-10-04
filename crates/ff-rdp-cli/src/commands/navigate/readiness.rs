use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use ff_rdp_core::{
    Grip, NavCause, RdpTransport, Resource, ResourceCommand, ResourceType, RootActor, TabActor,
    WatcherActor,
};
use serde_json::{Value, json};

use crate::commands::js_helpers::{WaitForPredicate, eval_or_bail, wait_for_predicates};
use crate::error::AppError;

use super::status::{
    DocumentStatusTracker, FallbackStatusEvidence, StatusUnknown, status_grace_budget_ms,
};
use super::{WaitAfterNav, WaitLevel, restore_timeout};

/// The result of waiting for a navigation to commit.
#[derive(Debug)]
pub(crate) struct CommitInfo {
    /// The URL observed after the navigation committed.
    pub(crate) committed_url: String,
    /// The `document.readyState` observed when the commit condition was met.
    pub(crate) ready_state: String,
    /// Wall-clock milliseconds elapsed from navigate dispatch to commit.
    pub(crate) elapsed_ms: u64,
    /// The main document's HTTP status code (iter-138 Theme A), when observed
    /// via a `network-event` resource whose `cause_type == "document"` and
    /// whose canonical URL matches the committed (or requested) URL. Callers
    /// must surface this as an explicit `null`, never omit it — consistent
    /// with iter-128's always-present-nullable-key convention.
    pub(crate) http_status: Option<u16>,
    /// Why [`Self::http_status`] is `None`, and `None` itself when it is
    /// `Some` (iter-166 Theme B). Kept in lockstep with `http_status` so a
    /// caller can tell "the server sent no status" from "we never looked".
    pub(crate) status_reason: Option<StatusUnknown>,
}

/// Configuration for the interleaved `document.readyState` fast-path used by the
/// `Both` wait strategy (iter-122 Theme A).
///
/// On FF152 the `dom-complete` `document-event` resource may never fire for a
/// page that has, in fact, finished loading — so a naive event-only wait burns
/// the whole events budget (~7 s) before the readystate fallback ever runs.
/// When this config is present, [`wait_for_doc_complete`] interleaves a
/// lightweight atomic readiness sample (using the same freshness/progress
/// predicate as the pure readystate path) into its drain loop, returning when
/// the page reports `complete` without waiting out the events budget.
pub(crate) struct ReadyStateProbe<'a> {
    /// Console actor bound to the navigating docshell, used to evaluate JS.
    ///
    /// Captured *before* `navigateTo` is dispatched, so it is bound to the
    /// **pre-navigation** docshell. Firefox tears down the docshell (and, for
    /// cross-process navigations, the child process) once the new document
    /// commits, which invalidates this ID (`noSuchActor` on every eval). The
    /// wait loop refreshes it via `tab_actor` as soon as `dom-loading` is
    /// observed (see the `noSuchActor` fix, iter-124).
    pub(crate) console_actor: Option<ff_rdp_core::ActorId>,
    /// Tab descriptor actor used to re-resolve `console_actor` once the new
    /// docshell has committed (`getTarget` returns fresh actor IDs).
    pub(crate) tab_actor: &'a ff_rdp_core::ActorId,
    /// Optional positive `navigationStart` captured before dispatch. A known
    /// epoch is authoritative; absent epoch evidence may use a known changed
    /// href as progress, without claiming a fresh document.
    pub(crate) pre_epoch: Option<f64>,
    /// Do not probe until this instant, giving the (faster, richer)
    /// `dom-complete` event a head start on pages that do fire it promptly.
    pub(crate) first_probe_at: Instant,
    /// Minimum spacing between readystate probes so events keep priority.
    pub(crate) probe_interval: Duration,
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
    pub(crate) poll_enabled: bool,
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
    pub(crate) pre_href: String,
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
    pub(crate) trust_event_url: bool,
}

/// Capture primitives in one evaluation, so a subsequent document cannot supply
/// the URL for an earlier document's readiness result. Actor IDs and cached target
/// forms do not establish immutable document identity.
pub(crate) const READINESS_SAMPLE: &str = "JSON.stringify({readyState: document.readyState, \
    epoch: performance.timing.navigationStart, href: window.location.href})";

#[derive(Clone, Copy)]
pub(crate) struct ReadinessCheck<'a> {
    pub(crate) pre_epoch: Option<f64>,
    pub(crate) pre_href: &'a str,
    pub(crate) requested_url: &'a str,
}

impl ReadinessCheck<'_> {
    fn accepted_href(self, grip: &Grip) -> Option<String> {
        let Grip::Value(Value::String(encoded)) = grip else {
            return None;
        };
        let sample: Value = serde_json::from_str(encoded).ok()?;
        if sample.get("readyState")?.as_str()? != "complete" {
            return None;
        }
        let href = sample.get("href")?.as_str()?;
        if href.is_empty()
            || (!self.requested_url.is_empty() && needs_href_fallback(href, self.requested_url))
        {
            return None;
        }
        let progressed = match self.pre_epoch {
            // A known epoch remains authoritative, even when href changed.
            Some(before) => sample
                .get("epoch")?
                .as_f64()
                .is_some_and(|now| now > before),
            // Missing epoch evidence must not become zero. A known changed href
            // can establish progress under the existing changed-href contract;
            // it does not prove a fresh document or navigation causality.
            None => !self.pre_href.is_empty() && href != self.pre_href,
        };
        progressed.then(|| href.to_owned())
    }
}

/// An absent/exceptional/malformed baseline cannot authorize freshness.
pub(crate) fn capture_pre_nav_epoch(
    ctx: &mut crate::commands::connect_tab::ConnectedTab,
    context: &str,
) -> Option<f64> {
    let console = ctx.target().console_actor.clone();
    let result = eval_or_bail(ctx, &console, "performance.timing.navigationStart", context).ok()?;
    match result.result {
        Grip::Value(Value::Number(n)) => n.as_f64().filter(|n| n.is_finite() && *n > 0.0),
        _ => None,
    }
}

/// Resolve the immutable serialized sample, not a new document evaluation.
/// Callers keep the evaluation, bounded substring fetch and successful release
/// inside the same read deadline, target guard and event-replay scope.
pub(crate) fn evaluate_readiness_sample(
    transport: &mut RdpTransport,
    console: &ff_rdp_core::ActorId,
) -> Result<ff_rdp_core::EvalResult, ff_rdp_core::ProtocolError> {
    let mut result =
        ff_rdp_core::WebConsoleActor::evaluate_js_async(transport, console, READINESS_SAMPLE)?;
    if result.exception.is_none()
        && let Grip::LongString { actor, length, .. } = &result.result
    {
        let full = ff_rdp_core::LongStringActor::full_string(transport, actor.as_ref(), *length)?;
        // Reuse the existing explicit release semantics (including an already
        // absent grip). Never issue cleanup RPCs after an incomplete fetch:
        // that reply still belongs to the failed operation.
        let grip = std::mem::replace(&mut result.result, Grip::Null);
        ff_rdp_core::LongStringScopedGrip::without_queue(grip).release(transport)?;
        // full_string bounds the announced length before fetching. Also bound
        // actual UTF-8 bytes before parsing the serialized document sample.
        if full.len() > ff_rdp_core::LongStringActor::MAX_FETCH {
            return Err(ff_rdp_core::ProtocolError::InvalidPacket(
                "readiness sample exceeds longstring byte limit".into(),
            ));
        }
        result.result = Grip::Value(Value::String(full));
    }
    Ok(result)
}

/// Best-effort atomic readiness sample for the event loop. Evaluation failures
/// mean "not ready yet"; the event loop retains its own deadline and replay.
fn probe_readystate_complete(
    transport: &mut RdpTransport,
    console_actor: &ff_rdp_core::ActorId,
    check: ReadinessCheck<'_>,
) -> Option<String> {
    match evaluate_readiness_sample(transport, console_actor) {
        Ok(result) if result.exception.is_none() => check.accepted_href(&result.result),
        _ => None,
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
/// A same-document navigation keeps its console, but this check cannot know
/// in advance whether navigation will replace the document. Watched callers
/// must revalidate the console before evaluating this condition.
///
/// Returns the new `location.href` once it differs from `pre_href` AND
/// `document.readyState === 'complete'` (the document was already fully
/// loaded before the same-document navigation began, and same-document
/// navigations never change that). Returns `None` while the condition
/// doesn't hold, on any transport/eval error (treated as "not yet" so a
/// transient hiccup never aborts the wait), and when `pre_href` is empty
/// (no baseline to compare against — e.g. the pre-navigation `location.href`
/// eval itself failed).
pub(crate) fn probe_same_document_commit(
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
pub(crate) fn probe_same_document_commit_safe(
    transport: &mut RdpTransport,
    bus_arc: &Arc<Mutex<ResourceCommand>>,
    console_actor: &ff_rdp_core::ActorId,
    pre_href: &str,
    retained: Option<&mut FallbackStatusEvidence>,
) -> Option<String> {
    if pre_href.is_empty() {
        // Skip the sink dance entirely when the check itself is a no-op —
        // `probe_same_document_commit` would return `None` immediately
        // without touching `transport`.
        return None;
    }
    with_event_replay(transport, bus_arc, retained, |t| {
        probe_same_document_commit(t, console_actor, pre_href)
    })
}

/// Run a **blocking RDP round-trip** from inside the navigation wait loop
/// without losing the watcher events that happen to be on the wire while it
/// runs (iter-169 Theme A — the defect this iteration fixes).
///
/// # The bug this exists to prevent
///
/// `evaluate_js_async` and `getTarget` both resolve through
/// `recv_reply_from`, which reads raw packets off `transport` until it finds
/// its own reply and hands every *other* packet it reads to the transport's
/// event sink — or drops it on the floor when no sink is installed. Inside
/// `wait_for_doc_complete` no sink is installed, so every such call was a
/// window in which a `resources-updated-array` could be read and discarded.
///
/// That window is not theoretical, and it is not narrow. `navigate`'s
/// `Both` strategy issues a blocking `getTarget`
/// ([`refresh_probe_console_actor`]) the instant `dom-loading` arrives —
/// which is, to within a few milliseconds, when Firefox emits the main
/// document's response line. Measured on Firefox 153, 30 cold-start
/// `navigate https://example.com` runs (see the iteration-169 plan): 29 runs
/// delivered two updates for the document's resource, the first carrying
/// `status: "200"`; the one failing run delivered only the second, and then
/// sat out the full 2 034 ms grace window waiting for an update that had
/// already been read and thrown away. `status_reason` said
/// `no_status_reported` — truthfully, from the tracker's point of view, and
/// misleadingly from the caller's.
///
/// # How it works
///
/// Install a temporary sink for the duration of `f`, then replay everything
/// it captured through `bus_arc.dispatch_event` in arrival order, so the wait
/// loop's next top-of-loop drain observes those packets exactly as if it had
/// read them off the wire itself. The previous sink (if any) is restored
/// afterwards, so nesting is safe.
///
/// Every blocking round-trip issued from inside the wait loop must go through
/// this. [`probe_same_document_commit_safe`] already did (iter-138 hardening
/// for the same bug class); the console-actor refresh, the readystate probe
/// and the `location.href` fallbacks did not.
pub(crate) fn with_event_replay<T>(
    transport: &mut RdpTransport,
    bus_arc: &Arc<Mutex<ResourceCommand>>,
    mut retained: Option<&mut FallbackStatusEvidence>,
    f: impl FnOnce(&mut RdpTransport) -> T,
) -> T {
    let (tx, rx) = std::sync::mpsc::channel::<Value>();
    let prev_sink = transport.swap_event_sink(Some(tx));
    let result = f(transport);
    transport.swap_event_sink(prev_sink);

    // Replay anything the round-trip swallowed, in delivery order, so the
    // main loop's next top-of-loop drain observes it exactly as if it had
    // read it directly off the wire itself.
    let captured: Vec<Value> = rx.try_iter().collect();
    if !captured.is_empty() {
        tracing::debug!(
            packets = captured.len(),
            "navigate: replaying events swallowed by a blocking round-trip"
        );
        let mut bus = bus_arc
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        for packet in captured {
            if let Some(evidence) = retained.as_deref_mut() {
                evidence.observe(&packet);
            }
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
pub(crate) fn must_reresolve_href(
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
/// A lookup failure retains the existing best-effort actor. A failed refresh
/// returns `false` so the
/// caller does NOT latch `probe_refreshed` — the new docshell
/// may not have finished registering server-side yet (a transient
/// `getTarget` failure), so the next probe-timer tick should retry rather
/// than permanently stranding the probe on the stale actor (iter-124 review
/// fix: latching on `Err` reintroduced the exact `noSuchActor` bug this
/// function exists to fix, just intermittently instead of always).
fn refresh_probe_console_actor(
    transport: &mut RdpTransport,
    probe: &mut ReadyStateProbe<'_>,
    deadline: Instant,
) -> bool {
    match crate::commands::connect_tab::resolve_target(
        transport,
        probe.tab_actor,
        deadline.min(Instant::now() + Duration::from_millis(100)),
    ) {
        Ok(fresh) => {
            probe.console_actor = Some(fresh.console_actor);
            true
        }
        Err(e) => {
            tracing::debug!(
                error = %e,
                "navigate: readystate probe console actor refresh failed; \
                 probe will retry target resolution on the next attempt"
            );
            false
        }
    }
}

/// Resolve `window.location.href` via `console_actor`, returning an empty string
/// on any error or exception. Used for the pre-dispatch href baseline and as
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
        Ok(result) if result.exception.is_none() => match result.result {
            Grip::Value(serde_json::Value::String(s)) => s,
            _ => String::new(),
        },
        _ => String::new(),
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
pub(crate) fn needs_href_fallback(candidate: &str, requested_url: &str) -> bool {
    // URL schemes are case-insensitive, but the scheme-specific content is
    // not: ABOUT:blank explicitly requests blank, whereas about:Blank does not.
    let requested_blank = requested_url
        .split_once(':')
        .is_some_and(|(scheme, path)| scheme.eq_ignore_ascii_case("about") && path == "blank");
    candidate.is_empty() || (candidate == "about:blank" && !requested_blank)
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
/// Feed a network resource into `tracker` (iter-138 Theme A tracking, shared by
/// `wait_for_doc_complete`'s main drain and its post-loop grace-wait), and
/// return the inner `Value` when `resource` is a `DocumentEvent` (the caller
/// should continue processing it), or `None` when `resource` was a
/// `NetworkEvent`/`NetworkUpdate` (already handled here) or an unrelated
/// resource type (nothing to do).
///
/// Which resource is the main document is decided later, by
/// [`DocumentStatusTracker::pick_document`], rather than the instant an event
/// arrives: before iter-166 this function matched eagerly on
/// `url == requested_url` and so could not use the committed URL, which is only
/// known once the wait has resolved.
pub(crate) fn extract_document_event<'a>(
    resource: &'a Resource,
    tracker: &mut DocumentStatusTracker,
) -> Option<&'a Value> {
    match resource {
        Resource::NetworkEvent(res) => {
            tracker.note_resource(res);
            None
        }
        Resource::NetworkUpdate(upd) => {
            tracker.note_update(upd);
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
pub(crate) fn wait_for_doc_complete(
    transport: &mut RdpTransport,
    bus_arc: &Arc<Mutex<ResourceCommand>>,
    rx: &std::sync::mpsc::Receiver<std::sync::Arc<Resource>>,
    timeout_ms: u64,
    wait_level: WaitLevel,
    nav_start: Instant,
    probe: Option<&mut ReadyStateProbe<'_>>,
    requested_url: &str,
    network_observed: bool,
) -> Result<CommitInfo, AppError> {
    wait_for_doc_complete_retaining_status(
        transport,
        bus_arc,
        rx,
        timeout_ms,
        wait_level,
        nav_start,
        probe,
        requested_url,
        network_observed,
        None,
    )
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn wait_for_doc_complete_retaining_status(
    transport: &mut RdpTransport,
    bus_arc: &Arc<Mutex<ResourceCommand>>,
    rx: &std::sync::mpsc::Receiver<std::sync::Arc<Resource>>,
    timeout_ms: u64,
    wait_level: WaitLevel,
    nav_start: Instant,
    mut probe: Option<&mut ReadyStateProbe<'_>>,
    requested_url: &str,
    network_observed: bool,
    mut retained: Option<&mut FallbackStatusEvidence>,
) -> Result<CommitInfo, AppError> {
    let deadline = Instant::now() + Duration::from_millis(timeout_ms);
    // Wall-clock time of the dispatch, in the epoch milliseconds Firefox
    // stamps on `document-event`s — see `is_stale_lifecycle_event`.
    let dispatched_at_ms = epoch_ms_at(nav_start);

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
    // The main document's network resources and their observed HTTP statuses
    // (iter-138 Theme A). Only populated when the caller subscribed to
    // `ResourceType::NetworkEvent` alongside `DocumentEvent` (currently only
    // `navigate`'s `run_core` does — `back`/`forward`/`reload` don't, and pass
    // `network_observed: false` so the envelope says `not_observed` instead of
    // implying the server was silent: Theme A only covers `navigate`).
    let mut tracker = if network_observed {
        DocumentStatusTracker::observing()
    } else {
        DocumentStatusTracker::default()
    };

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
            let Some(v) = extract_document_event(arc.as_ref(), &mut tracker) else {
                continue;
            };
            {
                let name = v.get("name").and_then(|n| n.as_str()).unwrap_or("");
                let url = v
                    .get("url")
                    .and_then(|u| u.as_str())
                    .unwrap_or("")
                    .to_owned();
                // iter-169: the counterpart to the `network-event` tracing in
                // `DocumentStatusTracker`. Between the two, `RUST_LOG=debug`
                // shows exactly which half of the wait is starved when a
                // navigation verb burns its whole events budget — which is
                // how iteration 174's `reload` defect was localised.
                tracing::debug!(event = name, %url, "navigate: document-event observed");

                if commit_url.is_none() && is_stale_lifecycle_event(name, v, dispatched_at_ms) {
                    // The watcher replays the outgoing document's lifecycle
                    // events when the subscription starts. Leaving a page that
                    // was still loading (measured: a GitHub page at
                    // `interactive`, right after a navigating `click`), its
                    // replayed `dom-interactive` set the commit URL and its
                    // `dom-complete` ended the wait — `navigate` reported the
                    // page it left as `committed_url`.
                    tracing::debug!(
                        event = name,
                        "navigate: pre-dispatch lifecycle event ignored"
                    );
                    continue;
                }

                match name {
                    "dom-loading" => {
                        // Always detect neterror early — Firefox loads about:neterror
                        // as a document and we will see dom-loading with the
                        // neterror URL before dom-complete fires.
                        if is_neterror_url(&url) {
                            return Err(AppError::Navigation {
                                cause: error_page_cause(&url),
                                url,
                                firefox_error: None,
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
                        //
                        // iter-169 Theme A: this blocking `getTarget` fires
                        // at the exact moment Firefox emits the main
                        // document's response line, and before this
                        // iteration it dropped whatever it read while
                        // scanning for its reply — losing the
                        // `resources-updated-array` that carries `status`
                        // outright. `with_event_replay` captures and replays
                        // those packets instead; see its doc comment for the
                        // measurement.
                        if !probe_refreshed
                            && let Some(p) = probe.as_deref_mut()
                            && ((wait_level == WaitLevel::Complete && p.poll_enabled)
                                || needs_href_fallback(&url, requested_url))
                            && with_event_replay(transport, bus_arc, retained.as_deref_mut(), |t| {
                                refresh_probe_console_actor(t, p, deadline)
                            })
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
                                    match probe.as_deref().and_then(|p| p.console_actor.clone()) {
                                        Some(actor) => with_event_replay(
                                            transport,
                                            bus_arc,
                                            retained.as_deref_mut(),
                                            |t| eval_location_href(t, &actor),
                                        ),
                                        None => String::new(),
                                    }
                                } else {
                                    url
                                };
                            break 'wait CommitInfo {
                                committed_url,
                                ready_state: "loading".to_owned(),
                                elapsed_ms,
                                // Resolved once, after the loop and its grace-wait (iter-166):
                                // the committed URL is not known until this break.
                                http_status: None,
                                status_reason: None,
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
                                    match probe.as_deref().and_then(|p| p.console_actor.clone()) {
                                        Some(actor) => with_event_replay(
                                            transport,
                                            bus_arc,
                                            retained.as_deref_mut(),
                                            |t| eval_location_href(t, &actor),
                                        ),
                                        None => String::new(),
                                    }
                                } else {
                                    eff_url
                                };
                            break 'wait CommitInfo {
                                committed_url,
                                ready_state: "interactive".to_owned(),
                                elapsed_ms,
                                // Resolved once, after the loop and its grace-wait (iter-166):
                                // the committed URL is not known until this break.
                                http_status: None,
                                status_reason: None,
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
                            let mut href =
                                match probe.as_deref().and_then(|p| p.console_actor.clone()) {
                                    Some(actor) => with_event_replay(
                                        transport,
                                        bus_arc,
                                        retained.as_deref_mut(),
                                        |t| eval_location_href(t, &actor),
                                    ),
                                    None => String::new(),
                                };
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
                                && with_event_replay(
                                    transport,
                                    bus_arc,
                                    retained.as_deref_mut(),
                                    |t| refresh_probe_console_actor(t, p, deadline),
                                )
                                && let Some(actor) = p.console_actor.clone()
                            {
                                probe_refreshed = true;
                                href = with_event_replay(
                                    transport,
                                    bus_arc,
                                    retained.as_deref_mut(),
                                    |t| eval_location_href(t, &actor),
                                );
                            }
                            if needs_href_fallback(&href, requested_url) {
                                // Still ambiguous after a fresh lookup — most
                                // likely still the intermediate docshell's own
                                // dom-complete. Discard its untrusted URL.
                                // Keep waiting for the real
                                // navigation's dom-loading/dom-complete (or a
                                // later probe tick, which retries the same fresh
                                // lookup). `commit_url`/`interactive_url` were
                                // already reset by the `.take()` calls above, so
                                // the next real dom-loading is tracked cleanly.
                                continue;
                            }
                            tracing::debug!(branch = "dom-complete-href", committed_url = %href, "navigate: completion selected");
                            break 'wait CommitInfo {
                                committed_url: href,
                                ready_state: "complete".to_owned(),
                                elapsed_ms,
                                // Resolved once, after the loop and its grace-wait (iter-166):
                                // the committed URL is not known until this break.
                                http_status: None,
                                status_reason: None,
                            };
                        }
                        tracing::debug!(branch = "dom-complete", committed_url = %committed, "navigate: completion selected");
                        break 'wait CommitInfo {
                            committed_url: committed,
                            ready_state: "complete".to_owned(),
                            elapsed_ms,
                            // Resolved once, after the loop and its grace-wait (iter-166):
                            // the committed URL is not known until this break.
                            http_status: None,
                            status_reason: None,
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
            && let (Some(p), Some(when)) = (probe.as_deref_mut(), same_doc_next_check_at)
            && Instant::now() >= when
            && !p.pre_href.is_empty()
        {
            if let Some(actor) = p.console_actor.as_ref()
                && let Some(href) =
                probe_same_document_commit_safe(transport, bus_arc, actor, &p.pre_href, retained.as_deref_mut())
                // A changed URL is not necessarily a same-document commit:
                // a cross-process transition can expose a complete blank
                // document. For a known destination, apply the same ambiguity
                // guard as the event and fresh-epoch paths. History traversal
                // has no requested URL and may legitimately return to blank.
                && (requested_url.is_empty() || !needs_href_fallback(&href, requested_url))
            {
                tracing::debug!(branch = "same-document", %href, pre_href = %p.pre_href, console_actor = ?p.console_actor, "navigate: completion selected");
                let elapsed_ms = u64::try_from(nav_start.elapsed().as_millis()).unwrap_or(u64::MAX);
                break 'wait CommitInfo {
                    committed_url: href,
                    ready_state: "complete".to_owned(),
                    elapsed_ms,
                    // Resolved once, after the loop and its grace-wait (iter-166):
                    // the committed URL is not known until this break.
                    http_status: None,
                    status_reason: None,
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
            // iter-169 Theme A: each of these round-trips reads
            // raw packets off the wire while scanning for its own reply, so
            // each is wrapped so a `resources-updated-array` caught in the
            // middle is replayed into the bus rather than dropped.
            if !probe_refreshed {
                // A failed refresh keeps the existing best-effort actor; the
                // sample below then simply finds nothing new.
                let _ = with_event_replay(transport, bus_arc, retained.as_deref_mut(), |t| {
                    refresh_probe_console_actor(t, p, deadline)
                });
            }
            let check = ReadinessCheck {
                pre_epoch: p.pre_epoch,
                pre_href: &p.pre_href,
                requested_url,
            };
            if let Some(probe_actor) = p.console_actor.clone()
                && let Some(committed) =
                    with_event_replay(transport, bus_arc, retained.as_deref_mut(), |t| {
                        t.with_read_deadline(deadline, |t| {
                            Ok(probe_readystate_complete(t, &probe_actor, check))
                        })
                        .unwrap_or_default()
                    })
            {
                tracing::debug!(branch = "readiness-sample", committed_url = %committed, console_actor = ?p.console_actor, "navigate: completion selected");
                let elapsed_ms = u64::try_from(nav_start.elapsed().as_millis()).unwrap_or(u64::MAX);
                break 'wait CommitInfo {
                    committed_url: committed,
                    ready_state: "complete".to_owned(),
                    elapsed_ms,
                    http_status: None,
                    status_reason: None,
                };
            }
            // Rejected samples may belong to a transitional direct target. A
            // later lookup must evaluate the whole sample on the new actor; it
            // must never combine this actor's readiness with that actor's href.
            probe_refreshed = false;
            // Re-arm the probe timer regardless of the outcome above.
            next_probe_at = Some(Instant::now() + p.probe_interval);
        }

        // Pump the transport — will block up to `poll_interval` then return
        // Timeout, which we treat as idle (keep looping).
        // The lock is acquired ONLY for dispatch_event (not held during recv).
        match transport.recv() {
            Ok(msg) => {
                if let Some(evidence) = retained.as_deref_mut() {
                    evidence.observe(&msg);
                }
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
    // before finalizing — well under the caller's overall timeout.
    //
    // iter-166: the resolution now runs against `commit_info.committed_url` as
    // well as `requested_url`, so it can only happen here — the committed URL
    // does not exist until the loop above breaks. Two consequences follow, and
    // `status_reason` is what makes both expressible:
    //
    // * `NotObserved` — nobody subscribed, so no amount of waiting can produce
    //   a status. Skipping the loop entirely (rather than spinning it out on a
    //   condition that can never become true) takes 300 ms off every
    //   `back`/`forward`/`reload`.
    // * `NoStatusReported` — the document's request HAS been identified and it
    //   committed, so its response line exists and the update carrying it is
    //   merely late. That is worth waiting materially longer for; the 300 ms
    //   of iter-138 made `live_138_navigate_reports_404` fail roughly one run
    //   in three even on an idle machine, reporting `null` for a page whose
    //   404 was already on the wire. The loop exits the instant the status
    //   lands, so the longer budget costs nothing in the common case.
    //
    // `NoDocumentRequest` keeps the original short window: the `network-event`
    // itself may still be in flight, but nothing guarantees one is coming.
    // The budget is re-derived on every pass rather than fixed up front,
    // because the reason itself changes as events arrive: a wait that starts
    // out `NoDocumentRequest` becomes `NoStatusReported` the moment the
    // document's `network-event` lands, and that is exactly when the longer
    // budget should apply.
    let resolved = |t: &DocumentStatusTracker| t.resolve(requested_url, &commit_info.committed_url);
    let grace_start = Instant::now();
    loop {
        let (status, reason) = resolved(&tracker);
        if status.is_some() {
            break;
        }
        let budget_ms = status_grace_budget_ms(reason);
        if grace_start.elapsed() >= Duration::from_millis(budget_ms) {
            break;
        }
        while let Ok(arc) = rx.try_recv() {
            let _ = extract_document_event(arc.as_ref(), &mut tracker);
        }
        if resolved(&tracker).0.is_some() {
            break;
        }
        match transport.recv() {
            Ok(msg) => {
                if let Some(evidence) = retained.as_deref_mut() {
                    evidence.observe(&msg);
                }
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
    let (status, reason) = resolved(&tracker);
    // iter-169 Theme A instrumentation. The envelope's `elapsed_ms` is
    // snapshotted at commit time (inside the wait loop above) and so says
    // nothing about how long this grace loop ran — which made a
    // `no_status_reported` envelope reporting `elapsed_ms: 250` look like
    // proof the 2000 ms budget had been skipped when it is simply not
    // measuring it. Log the grace-loop's own elapsed alongside the tracker's
    // contents so the next diagnosis starts from a measurement instead of an
    // inference.
    tracing::debug!(
        grace_ms = u64::try_from(grace_start.elapsed().as_millis()).unwrap_or(u64::MAX),
        observing = tracker.observing,
        doc_resources = tracker.docs.len(),
        status_updates = tracker.statuses.len(),
        reason = reason.map_or("none", StatusUnknown::as_str),
        committed_url = %commit_info.committed_url,
        requested_url,
        selected_resource = ?tracker.pick_document(requested_url, &commit_info.committed_url),
        docs = ?tracker.docs,
        statuses = ?tracker.statuses,
        "navigate: document status resolved"
    );
    commit_info.http_status = status;
    commit_info.status_reason = reason;

    Ok(commit_info)
}

/// Extract the `e=` parameter value from an `about:neterror` URL.
///
/// Returns the raw `e=` value so the caller can pass it to
/// [`NavCause::from_e_param`] for typed classification.
pub(crate) fn classify_neterror(url: &str) -> Option<&str> {
    // about:neterror?e=dnsNotFound&... / about:certerror?e=nssBadCert&...
    let query = url
        .strip_prefix("about:neterror?")
        .or_else(|| url.strip_prefix("about:certerror?"))?;
    query
        .split('&')
        .find(|seg| seg.starts_with("e="))?
        .strip_prefix("e=")
}

/// Returns `true` when `url` is one of Firefox's load-failure pages:
/// `about:neterror` (DNS, refused, reset, …) or `about:certerror` (an
/// untrusted, expired or mismatched TLS certificate — dogfooding session 64
/// #1, where `navigate https://expired.badssl.com/` reported success).
pub(crate) fn is_neterror_url(url: &str) -> bool {
    url.starts_with("about:neterror") || url.starts_with("about:certerror")
}

/// Wall-clock epoch milliseconds at `instant` (a moment in the past).
pub(crate) fn epoch_ms_at(instant: Instant) -> f64 {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    now.saturating_sub(instant.elapsed()).as_secs_f64() * 1000.0
}

/// Tolerance for comparing Firefox's event timestamps with ff-rdp's own clock
/// reading (same machine, but each side rounds independently).
const STALE_EVENT_SLACK_MS: f64 = 50.0;

/// Whether a `dom-interactive`/`dom-complete` event, arriving before any
/// `dom-loading` of this wait, describes a document lifecycle that happened
/// before this command dispatched its navigation — i.e. the watcher's replay
/// of the outgoing document's existing events, not the load the wait is for.
///
/// Firefox stamps these events with an epoch-millisecond `time`. An absent or
/// zero `time` (Firefox sends `0` for some subframe and cached documents)
/// proves nothing, so such events are kept. `dom-loading` is never judged by
/// its timestamp: it is what starts tracking a load, and once one has been
/// seen the events that follow belong to it (the caller only asks before
/// then).
pub(crate) fn is_stale_lifecycle_event(name: &str, event: &Value, dispatched_at_ms: f64) -> bool {
    if !matches!(name, "dom-interactive" | "dom-complete") {
        return false;
    }
    let time = event.get("time").and_then(Value::as_f64).unwrap_or(0.0);
    time > 0.0 && time + STALE_EVENT_SLACK_MS < dispatched_at_ms
}

/// The typed cause for an error-page URL accepted by [`is_neterror_url`].
///
/// `about:certerror` is a certificate failure whatever its `e=` says (Firefox
/// uses `nssBadCert` for expired, self-signed and wrong-host alike); an
/// `about:neterror` is classified by its `e=` parameter.
pub(crate) fn error_page_cause(url: &str) -> NavCause {
    if url.starts_with("about:certerror") {
        return NavCause::CertError;
    }
    classify_neterror(url).map_or(
        NavCause::Unknown("unknown".to_owned()),
        NavCause::from_e_param,
    )
}

/// Firefox's own error code for the certificate failure the tab is showing
/// (e.g. `SEC_ERROR_EXPIRED_CERTIFICATE`, `MOZILLA_PKIX_ERROR_SELF_SIGNED_CERT`,
/// `SSL_ERROR_BAD_CERT_DOMAIN`), read from the `about:certerror` document via
/// `document.getFailedCertSecurityInfo()`. `None` when the page is not a
/// certificate error page or the call is unavailable.
pub(crate) fn eval_cert_error_code(
    transport: &mut RdpTransport,
    console_actor: &ff_rdp_core::ActorId,
) -> Option<String> {
    const JS: &str = "(() => { try { return document.getFailedCertSecurityInfo().errorCodeString || null; } catch (e) { return null; } })()";
    match ff_rdp_core::WebConsoleActor::evaluate_js_async(transport, console_actor, JS) {
        Ok(result) if result.exception.is_none() => match result.result {
            Grip::Value(Value::String(s)) if !s.is_empty() => Some(s),
            _ => None,
        },
        _ => None,
    }
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
pub(crate) fn is_readystate_fresh(current_nav_start: f64, pre_epoch: f64) -> bool {
    ReadinessCheck {
        pre_epoch: Some(pre_epoch),
        pre_href: "",
        requested_url: "https://new.test/",
    }
    .accepted_href(&Grip::Value(scripted_readiness(
        current_nav_start,
        "https://new.test/",
        "complete",
    )))
    .is_some()
}

// Declared protocol values for the actual Rust predicate; not Firefox fixtures.
#[cfg(test)]
pub(crate) fn scripted_readiness(epoch: f64, href: &str, state: &str) -> Value {
    json!(json!({"epoch":epoch,"href":href,"readyState":state}).to_string())
}

/// Poll `document.readyState` on the current console under one absolute
/// deadline; a zero budget makes exactly one evaluation.
pub(crate) fn poll_direct_readystate(
    ctx: &mut crate::commands::connect_tab::ConnectedTab,
    check: ReadinessCheck<'_>,
    timeout_ms: u64,
) -> Result<String, AppError> {
    use ff_rdp_core::ProtocolError;
    let console = ctx.target().console_actor.clone();
    let deadline = Instant::now() + Duration::from_millis(timeout_ms);
    loop {
        let evaluate = |t: &mut RdpTransport| evaluate_readiness_sample(t, &console);
        let result = if timeout_ms == 0 {
            evaluate(ctx.transport_mut())
        } else {
            ctx.transport_mut().with_read_deadline(deadline, evaluate)
        };
        match result {
            Ok(result) => {
                if let Some(exception) = result.exception {
                    let message = format!(
                        "navigate readystate: JS evaluation error{}",
                        exception
                            .message
                            .map_or_else(String::new, |m| format!(": {m}"))
                    );
                    return Err(AppError::User(
                        ff_rdp_core::sanitize_for_terminal(&message).into_owned(),
                    ));
                }
                if let Some(href) = check.accepted_href(&result.result) {
                    return Ok(href);
                }
            }
            Err(ProtocolError::Timeout) => {}
            Err(error) => return Err(error.into()),
        }
        if Instant::now() >= deadline {
            return Err(AppError::Timeout("waiting for document readiness".into()));
        }
        std::thread::sleep(
            Duration::from_millis(100).min(deadline.saturating_duration_since(Instant::now())),
        );
    }
}

/// Poll `document.readyState == "complete"` until the deadline, returning a
/// `CommitInfo` when the condition is met.
///
/// Readiness, epoch and committed href come from one sample. A known baseline
/// epoch requires a strictly newer epoch; when unavailable, a known changed href
/// can establish progress (not fresh-document identity or navigation causality).
/// Missing evidence keeps polling under the existing deadline.
///
/// Used by the `readystate` and `both` wait strategies as a fallback when the
/// document-event resource stream doesn't fire within the timeout budget.
pub(crate) fn wait_for_readystate_complete(
    ctx: &mut crate::commands::connect_tab::ConnectedTab,
    timeout_ms: u64,
    check: ReadinessCheck<'_>,
    nav_start: Instant,
) -> Result<CommitInfo, AppError> {
    let url = match poll_direct_readystate(ctx, check, timeout_ms) {
        Ok(href) => href,
        Err(AppError::Timeout(_)) => {
            let total_elapsed_ms =
                u64::try_from(nav_start.elapsed().as_millis()).unwrap_or(u64::MAX);
            return Err(AppError::Timeout(format!(
                "navigate: document.readyState did not reach 'complete' (with navigation progress) \
                 within {total_elapsed_ms}ms — use --no-wait to skip or increase --timeout"
            )));
        }
        Err(e) => return Err(e),
    };

    let elapsed_ms = u64::try_from(nav_start.elapsed().as_millis()).unwrap_or(u64::MAX);
    Ok(CommitInfo {
        committed_url: url,
        ready_state: "complete".to_owned(),
        elapsed_ms,
        // The readystate poll doesn't subscribe to network events — no
        // status is ever observable from this path (iter-138 Theme A covers
        // only the primary events-based `wait_for_doc_complete` path), which
        // is exactly what `not_observed` says (iter-166).
        http_status: None,
        status_reason: Some(StatusUnknown::NotObserved),
    })
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
pub(crate) fn split_wait_budget(timeout_ms: u64) -> (u64, u64) {
    let reserved_ms = (timeout_ms * 30 / 100).max(1000).min(timeout_ms / 2);
    let events_budget = timeout_ms.saturating_sub(reserved_ms);
    (reserved_ms, events_budget)
}

/// Resolve the tab's `WatcherActor`, requesting **server-side target
/// switching** (iter-174).
///
/// Every navigation wait in this module depends on `document-event` resources
/// (`dom-loading` / `dom-interactive` / `dom-complete`). Those are emitted by
/// a content-process resource watcher that only exists on a target the
/// **watcher** instantiated — and Firefox only instantiates one for the
/// top-level window global when `getWatcher` was called with
/// `isServerTargetSwitchingEnabled: true`
/// (`devtools/shared/specs/descriptors/tab.js`; see also
/// `kb/research/frame-targets.md`). Without the flag, `watchTargets("frame")`
/// is accepted and acked, `watchResources(["document-event", ...])` is
/// accepted and acked, and then **only parent-process resources are ever
/// delivered** — `will-navigate` and `network-event` arrive, the three
/// content-process `dom-*` events never do.
///
/// Measured on FF154, static localhost page, `main` @ `7d457af`
/// (iteration-174's plan carries the full trace):
///
/// | command                                   | before   | after   |
/// |-------------------------------------------|----------|---------|
/// | `reload`                                  | 21011 ms | 115 ms  |
/// | `navigate --wait-strategy events`         | timeout (30 s) | ~150 ms |
///
/// The 21 s is not a hang: it is `split_wait_budget(30000).1` burnt in full
/// by a `dom-complete` that can never arrive, after which
/// `wait_for_readystate_complete` polls `document.readyState` and produces a
/// correct-looking envelope — which is why this survived four iterations
/// unnoticed (`status: null, status_reason: "not_observed"` was the only
/// visible symptom).
///
/// The flag also moves top-level target delivery onto the watcher, so the
/// actor obtained earlier from the descriptor's `getTarget` may be swapped
/// out by a subsequent navigation. Every caller here already re-resolves via
/// `refresh_console_actor` / `refresh_probe_console_actor` after the commit,
/// so that is the pre-existing contract rather than a new requirement. It is
/// deliberately NOT flipped on the generic `connect_and_get_target` path (see
/// `TabActor::get_watcher_with_options`' own CAUTION) — only on the two
/// navigation waits that consume `document-event`.
pub(crate) fn get_navigation_watcher(
    ctx: &mut crate::commands::connect_tab::ConnectedTab,
    tab_actor: &ff_rdp_core::ActorId,
) -> Result<ff_rdp_core::ActorId, AppError> {
    TabActor::get_watcher_with_options(ctx.transport_mut(), tab_actor, Some(true))
        .map_err(AppError::from)
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
    ctx: &mut crate::commands::connect_tab::ConnectedTab,
    cli_timeout: u64,
    requested_url: &str,
    conditions: &crate::cli::args::NetworkConditionsArgs,
    dispatch: impl FnOnce(&mut RdpTransport) -> Result<(), AppError>,
) -> Result<serde_json::Value, AppError> {
    let tab_actor = ctx.target_tab_actor().clone();
    // iter-174: `Some(true)` — without it the three `dom-*` document-events
    // never arrive on a direct connection. See `get_navigation_watcher`.
    let watcher_actor = get_navigation_watcher(ctx, &tab_actor)?;
    // `--throttle`/`--block` (reload): set on this connection's watcher before
    // the dispatch, so they govern the load this command waits for.
    let conditions_applied =
        crate::commands::network_conditions::apply(ctx, &watcher_actor, conditions)?;

    let pre_nav_epoch = capture_pre_nav_epoch(ctx, "nav_action: pre-nav epoch eval");

    // `window.location.href` captured before dispatch (iter-138 Themes B/C)
    // — the baseline `probe_same_document_commit` compares against to detect
    // a completed same-document traversal (SPA `popstate`, fragment nav)
    // that never fires a `document-event` at all. Best-effort like
    // `pre_nav_epoch`: an empty string just disables that check rather than
    // blocking the navigation action.
    let pre_nav_href: String = {
        let console_actor = ctx.target().console_actor.clone();
        eval_location_href(ctx.transport_mut(), &console_actor)
    };

    // See run_core's identical prelude for why watchTargets("frame") must
    // precede watchResources — the watcher delivers nothing until both have
    // been issued (iter-79 Theme A).
    WatcherActor::watch_targets(ctx.transport_mut(), &watcher_actor, "frame")
        .map_err(AppError::from)?;

    let bus_arc = ctx.get_or_init_resource_command(watcher_actor.clone());
    // iter-169 Theme B: subscribe to `NetworkEvent` alongside `DocumentEvent`,
    // exactly as `run_core` does, so `back`/`forward`/`reload` report the main
    // document's HTTP status instead of omitting the key. iter-130 Theme B
    // promised all four navigation verbs the same envelope; until now the
    // three history verbs delivered `{committed_url, ready_state, elapsed_ms}`
    // and stopped, so `--jq '.results.status'` on a `reload` returned `null`
    // for a reason no caller could see.
    let (sub_id, rx) = bus_arc
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .subscribe(
            ctx.transport_mut(),
            &[ResourceType::DocumentEvent, ResourceType::NetworkEvent],
        )
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
        console_actor: Some(ctx.target().console_actor.clone()),
        tab_actor: &tab_actor,
        pre_epoch: pre_nav_epoch,
        first_probe_at: nav_start + Duration::from_millis(300),
        probe_interval: Duration::from_millis(250),
        poll_enabled: false,
        pre_href: pre_nav_href.clone(),
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
            // iter-169 Theme B: these three verbs now subscribe to
            // `NetworkEvent` too (see the `subscribe` call above), so a
            // missing status here means the same thing it means for
            // `navigate` — the server or the document produced none — rather
            // than "we never looked".
            true,
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
                wait_for_readystate_complete(
                    ctx,
                    remaining,
                    ReadinessCheck {
                        pre_epoch: pre_nav_epoch,
                        pre_href: &pre_nav_href,
                        requested_url,
                    },
                    nav_start,
                )
            }
        }
        Err(e) => Err(e),
    };
    // Dogfooding session 64 N2: `back`/`forward`/`reload` landing on an
    // error page (`about:certerror`, `about:neterror`) fail the way `navigate`
    // does. A commit with no observed HTTP status gets the same `listTabs`
    // check `navigate` runs, because `location.href` and the document-events
    // report the failed URL, never the error page's own. A `reload` timeout
    // is reclassified when the tab shows an error page; a `back`/`forward`
    // timeout is not — with no history entry to go to, the tab may simply
    // still show the error page it started on.
    let commit_info = if requested_url.is_empty() {
        commit_info?
    } else {
        reclassify_timeout_as_neterror(ctx, requested_url, commit_info)?
    };
    if commit_info.http_status.is_none() {
        let failed_url = if commit_info.committed_url.is_empty() {
            requested_url
        } else {
            commit_info.committed_url.as_str()
        };
        if let Some(nav_err) = check_real_tab_url_for_neterror(ctx, failed_url) {
            return Err(nav_err);
        }
    }

    refresh_console_actor(ctx);

    let mut result = json!({
        "committed_url": commit_info.committed_url,
        "ready_state": commit_info.ready_state,
        "elapsed_ms": commit_info.elapsed_ms,
        // iter-169 Theme B: both keys, always, on every one of the four
        // navigation verbs. `status_reason` is non-null exactly when `status`
        // is null, so a caller can tell "the server sent no status" from "no
        // request was made" (a BFCache-restored `back`) from "we never
        // looked".
        "status": commit_info.http_status,
        "status_reason": commit_info.status_reason.map(StatusUnknown::as_str),
    });
    crate::commands::network_conditions::insert_echo(&mut result, conditions_applied.as_ref());
    Ok(result)
}

/// Run the `--wait-for` predicates from `wait_opts`, re-resolving actors first.
///
/// Returns `Some(json)` when predicates were specified, `None` when none were given.
pub(crate) fn run_wait_for_predicates(
    ctx: &mut crate::commands::connect_tab::ConnectedTab,
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
    let console_actor =
        ctx.refresh_target_until(Instant::now() + Duration::from_millis(opts.wait_timeout))?;

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
/// Theme K: the consoleActor ID cached in `ctx.target()` is bound to the old
/// docshell. After navigation (including to about:neterror pages), callers
/// that reuse this connection fetch a fresh actor before their next `eval`
/// so it does not get `noSuchActor`.
///
/// This is a best-effort operation; failures are logged to stderr and swallowed.
///
/// iter-220: this also *consumes* the navigation announcement the transport
/// latched. `navigate` has already waited for its own commit by the time this
/// runs, so the target resolved here is the document the command produced —
/// there is nothing left for `--with-page`'s settle loop to wait for, and
/// leaving the latch armed would send it polling `getTarget` for a change that
/// already happened. `click` and `type --submit` deliberately do NOT clear it:
/// their navigation is still in flight when `page_view::attach` runs, which is
/// the whole defect iter-220 fixes.
pub(crate) fn refresh_console_actor(ctx: &mut crate::commands::connect_tab::ConnectedTab) {
    ctx.refresh_target();
    let _ = ctx.take_navigation_started();
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
pub(crate) fn check_real_tab_url_for_neterror(
    ctx: &mut crate::commands::connect_tab::ConnectedTab,
    requested_url: &str,
) -> Option<AppError> {
    // listTabs is a root-level RPC and may interleave with other pending events,
    // so we only do this when we suspect a neterror (non-fatal: if it fails we
    // fall through to the caller's success path).
    let Ok(tabs) = RootActor::list_tabs(ctx.transport_mut()) else {
        return None;
    };

    // The tab this connection drives (`--tab` may name a background one);
    // the selected tab only when the descriptor is not listed.
    let own = ctx.target_tab_actor().clone();
    let tab_url = tabs
        .iter()
        .find(|t| t.actor == own)
        .or_else(|| tabs.iter().find(|t| t.selected))
        .map(|t| t.url.clone())
        .unwrap_or_default();

    if !is_neterror_url(&tab_url) {
        return None;
    }

    let cause = error_page_cause(&tab_url);
    let firefox_error = if matches!(cause, NavCause::CertError) {
        // The cached console actor may still be the pre-navigation one; the
        // error page is its own document.
        ctx.refresh_target();
        let console_actor = ctx.target().console_actor.clone();
        eval_cert_error_code(ctx.transport_mut(), &console_actor)
    } else {
        None
    };
    // `back`/`forward` cannot know their landing URL up front; the error
    // page names the one that failed in its `u=` parameter.
    let url = if requested_url.is_empty() {
        error_page_failed_url(&tab_url).unwrap_or_default()
    } else {
        requested_url.to_owned()
    };
    Some(AppError::Navigation {
        cause,
        url,
        firefox_error,
    })
}

/// The failed URL an `about:neterror` / `about:certerror` page carries in its
/// `u=` query parameter, percent-decoded.
///
/// Not form-decoded: Firefox escapes `u=` path-style, so a literal `+` stays
/// a `+`, and an unescaped `&` inside it belongs to the URL. The value runs
/// to the next parameter Firefox appends after it (`&c=`, `&d=`, `&f=`), or
/// to the end.
pub(crate) fn error_page_failed_url(error_page_url: &str) -> Option<String> {
    let query = error_page_url.split_once('?')?.1;
    let start = if let Some(rest) = query.strip_prefix("u=") {
        rest
    } else {
        query.split_once("&u=")?.1
    };
    let end = ["&c=", "&d=", "&f="]
        .iter()
        .filter_map(|p| start.find(p))
        .min()
        .unwrap_or(start.len());
    Some(percent_decode(&start[..end]))
}

/// Decode `%XX` escapes (a malformed escape is kept verbatim); `+` is literal.
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && let Some(hex) = s.get(i + 1..i + 3)
            && hex.bytes().all(|c| c.is_ascii_hexdigit())
            && let Ok(b) = u8::from_str_radix(hex, 16)
        {
            out.push(b);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
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
pub(crate) fn reclassify_timeout_as_neterror(
    ctx: &mut crate::commands::connect_tab::ConnectedTab,
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
