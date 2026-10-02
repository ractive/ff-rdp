use serde_json::Value;

use ff_rdp_core::parse_network_resource_updates;

/// Why `navigate` reports `status: null` (iter-166 Theme B).
///
/// Before iter-166 a bare `null` conflated three very different situations, and
/// a caller scripting `navigate` could not tell "the server sent no status"
/// from "we never looked". Each variant below is emitted as the envelope's
/// `status_reason`, which is `null` exactly when `status` is non-`null`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StatusUnknown {
    /// This route never correlated the committed document's request, so no
    /// HTTP status could have been reported no matter what the server sent:
    /// `--no-wait` (returns before any resource can arrive), the
    /// pure-`readystate` wait strategy (never subscribes to `network-event`),
    /// and `reload --wait-idle` (streams network events but only counts them,
    /// against a quiescence deadline rather than a document).
    ///
    /// iter-169 removed `back`/`forward`/`reload` from this list — their
    /// commit-wait path now subscribes to `network-event` like `navigate` and
    /// reports a real status.
    NotObserved,
    /// Network events *were* observed, but none of them was the committed
    /// document's own request — a `data:`/`about:` URL, a bfcache restore, or
    /// a same-document (`pushState`/fragment) navigation, none of which issue
    /// one.
    NoDocumentRequest,
    /// The committed document's request was identified, but Firefox never
    /// reported an HTTP status for it — the response line had not arrived when
    /// the wait resolved, or the channel failed before one existed.
    NoStatusReported,
}

impl StatusUnknown {
    /// The stable wire string for the `status_reason` envelope key.
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::NotObserved => "not_observed",
            Self::NoDocumentRequest => "no_document_request",
            Self::NoStatusReported => "no_status_reported",
        }
    }
}

/// The `{status, status_reason}` pair for a route that never looked at the
/// network at all (iter-169 Theme B).
///
/// `back`/`forward`/`reload` used to omit both keys entirely, so
/// `--jq '.results.status'` returned `null` on a `reload` for a reason no
/// caller could see — indistinguishable from `navigate`'s meaningful `null`.
/// Every path of all four verbs now emits both keys; the ones that genuinely
/// cannot correlate a document request — `--no-wait`, which returns before any
/// resource can arrive, and `reload --wait-idle`, which counts frames against
/// a quiescence deadline — say so with `not_observed` rather than staying
/// silent.
pub(crate) fn not_observed_status() -> serde_json::Map<String, Value> {
    let mut map = serde_json::Map::new();
    map.insert("status".to_owned(), Value::Null);
    map.insert(
        "status_reason".to_owned(),
        Value::String(StatusUnknown::NotObserved.as_str().to_owned()),
    );
    map
}

/// Canonicalise a URL before comparing a *requested* (or *committed*) URL
/// against the URL Firefox reports on a `network-event` resource.
///
/// This is the iter-166 defect in one function. Firefox requests the
/// **canonical** form of whatever it is handed — `https://example.com` becomes
/// `https://example.com/` — while `requested_url` is the raw string the caller
/// typed. The old exact-string comparison therefore never matched the main
/// document on the single most common invocation there is, and `status` was
/// `null` for a page that plainly returned 200.
///
/// The fragment is stripped because it is never sent to the server, so a
/// `network-event` URL can never carry one; the query is deliberately kept,
/// since two same-path requests differing only in query really are different
/// requests and collapsing them would reintroduce the subframe-contamination
/// risk the `cause_type`/`url` pair exists to avoid.
///
/// Unparseable input is returned unchanged so the comparison degrades to the
/// old exact-string behaviour rather than to a panic.
pub(crate) fn canonical_doc_url(u: &str) -> String {
    url::Url::parse(u).map_or_else(
        |_| u.to_owned(),
        |mut parsed| {
            parsed.set_fragment(None);
            parsed.into()
        },
    )
}

/// Correlates the main document's `network-event` resource with the HTTP status
/// Firefox reports for it, and — when there is no status — records *why*.
///
/// Shared by both routes that can report `navigate`'s `status`: the streamed
/// one in [`wait_for_doc_complete`] (fed event-by-event as they arrive) and the
/// batch one in [`extract_document_status`] used by `--with-network` (fed from
/// the drained resource/update vectors). Before iter-166 those two carried
/// separate copies of the same matching rule, and both copies had the same bug.
#[derive(Debug, Default)]
pub(crate) struct DocumentStatusTracker {
    /// Whether the caller subscribed to `network-event` at all. `false` makes
    /// [`Self::resolve`] report [`StatusUnknown::NotObserved`] without
    /// pretending it looked.
    pub(crate) observing: bool,
    /// `(resource_id, canonical url)` for every `cause_type == "document"`
    /// resource seen, in arrival order.
    pub(crate) docs: Vec<(u64, String)>,
    /// `(resource_id, status)` for every status-carrying update, in arrival
    /// order. Firefox typically carries `status` only on the FIRST update for a
    /// resource, so this keeps every one rather than the most recent record.
    pub(crate) statuses: Vec<(u64, u16)>,
}

impl DocumentStatusTracker {
    /// A tracker for a route that subscribed to `network-event`.
    pub(crate) fn observing() -> Self {
        Self {
            observing: true,
            ..Self::default()
        }
    }

    /// Record a `network-event` resource, keeping only document loads.
    pub(crate) fn note_resource(&mut self, res: &ff_rdp_core::NetworkResource) {
        tracing::debug!(
            id = res.resource_id,
            cause = %res.cause_type,
            url = %res.url,
            "navigate: network-event resource observed"
        );
        if res.cause_type == "document" {
            self.docs
                .push((res.resource_id, canonical_doc_url(&res.url)));
        }
    }

    /// Record a `network-event` update, keeping only the status-carrying ones.
    pub(crate) fn note_update(&mut self, upd: &ff_rdp_core::NetworkResourceUpdate) {
        tracing::debug!(
            id = upd.resource_id,
            status = ?upd.status,
            "navigate: network-event update observed"
        );
        if let Some(ref s) = upd.status
            && let Ok(code) = s.parse::<u16>()
        {
            self.statuses.push((upd.resource_id, code));
        }
    }

    /// Pick the main document's resource id.
    ///
    /// Preference order, most trustworthy first:
    /// 1. the URL that actually **committed** — the end of a redirect chain, so
    ///    the status reported is the one belonging to the document the caller
    ///    ended up with rather than an intermediate `301`;
    /// 2. the URL that was **requested** — identical to the above when nothing
    ///    redirected, and all there is when `location.href` could not be read.
    ///
    /// There is deliberately no third, looser rule. A `cause_type ==
    /// "document"` resource is emitted for subframe loads too, so "if only one
    /// document request was seen, use it" would report an iframe's status as
    /// the page's whenever the main document itself issued no request (a
    /// bfcache restore, `about:blank`). Reporting nothing — with a
    /// `status_reason` that says so — beats reporting the wrong number.
    ///
    /// Within a preference, the LAST match wins: a redirect chain can produce
    /// several resources for the same URL, and the hop that actually committed
    /// is the final one.
    pub(crate) fn pick_document(&self, requested_url: &str, committed_url: &str) -> Option<u64> {
        for want in [committed_url, requested_url] {
            if want.is_empty() {
                continue;
            }
            let want = canonical_doc_url(want);
            if let Some((id, _)) = self.docs.iter().rev().find(|(_, u)| *u == want) {
                return Some(*id);
            }
        }
        None
    }

    /// The main document's HTTP status, or the reason there isn't one.
    ///
    /// Exactly one side of the pair is `Some`, which is what lets the envelope
    /// guarantee `status_reason == null` iff `status != null`.
    pub(crate) fn resolve(
        &self,
        requested_url: &str,
        committed_url: &str,
    ) -> (Option<u16>, Option<StatusUnknown>) {
        if !self.observing {
            return (None, Some(StatusUnknown::NotObserved));
        }
        let Some(id) = self.pick_document(requested_url, committed_url) else {
            return (None, Some(StatusUnknown::NoDocumentRequest));
        };
        self.statuses
            .iter()
            .rev()
            .find(|(rid, _)| *rid == id)
            .map_or((None, Some(StatusUnknown::NoStatusReported)), |(_, s)| {
                (Some(*s), None)
            })
    }
}

/// Observations retained only for this dispatch's Both fallback. This is
/// conservative request correlation, not immutable document identity: another
/// unobserved same-URL navigation from the same outgoing window is not proved
/// absent. Do not borrow the requested URL's status after an accepted redirect.
///
/// Firefox's navigation request innerWindowId identifies the outgoing window
/// (network-events.js onTopBrowsingContextWillNavigate), not the new document.
/// The typed Resource loses these ownership fields, so inspect raw packets at
/// the ordinary receive/replay boundaries without changing bus delivery.
#[derive(Default)]
pub(crate) struct FallbackStatusEvidence {
    context: Option<u64>,
    outgoing_window: Option<u64>,
    requests: Vec<(u64, String)>,
    statuses: Vec<(u64, u16)>,
}

impl FallbackStatusEvidence {
    pub(crate) fn new(target: &ff_rdp_core::TargetInfo) -> Self {
        Self {
            context: target.browsing_context_id.filter(|id| *id > 0),
            outgoing_window: target.inner_window_id.filter(|id| *id > 0),
            ..Self::default()
        }
    }

    // Private one-capture diagnostic: observes existing retained state only.
    // Raw receives (including teardown packets outside this observer) are
    // captured by the existing ff_rdp_core::transport TRACE target.
    pub(crate) fn trace_diagnostic(&self, phase: &'static str) {
        tracing::debug!(
            target: "ff_rdp_cli::navigation_166_diagnostic",
            phase,
            context = ?self.context,
            outgoing_window = ?self.outgoing_window,
            requests = ?self.requests,
            statuses = ?self.statuses,
            "166 retained status evidence"
        );
    }

    pub(crate) fn observe(&mut self, packet: &Value) {
        for update in parse_network_resource_updates(packet) {
            if let Some(status) = update.status.and_then(|s| s.parse::<u16>().ok()) {
                self.statuses.push((update.resource_id, status));
            }
        }
        let (Some(context), Some(window)) = (self.context, self.outgoing_window) else {
            return;
        };
        if packet["type"] != "resources-available-array" {
            return;
        }
        let Some(groups) = packet["array"].as_array() else {
            return;
        };
        for group in groups {
            if group[0] != "network-event" {
                continue;
            }
            let Some(resources) = group[1].as_array() else {
                continue;
            };
            for resource in resources {
                if resource["cause"]["type"] != "document"
                    || resource["isNavigationRequest"] != true
                    || resource["browsingContextID"].as_u64() != Some(context)
                    || resource["innerWindowId"].as_u64() != Some(window)
                {
                    continue;
                }
                if let (Some(id), Some(url)) =
                    (resource["resourceId"].as_u64(), resource["url"].as_str())
                {
                    self.requests.push((id, canonical_doc_url(url)));
                }
            }
        }
    }

    pub(crate) fn resolve(&self, accepted_url: &str) -> (Option<u16>, Option<StatusUnknown>) {
        let accepted = canonical_doc_url(accepted_url);
        let mut matching = self
            .requests
            .iter()
            .filter(|(_, url)| *url == accepted)
            .map(|(id, _)| *id);
        let Some(id) = matching.next() else {
            return (None, Some(StatusUnknown::NoDocumentRequest));
        };
        if matching.any(|other| other != id) {
            return (None, Some(StatusUnknown::NoDocumentRequest));
        }
        self.statuses
            .iter()
            .rev()
            .find(|(resource, _)| *resource == id)
            .map_or(
                (None, Some(StatusUnknown::NoStatusReported)),
                |(_, status)| (Some(*status), None),
            )
    }
}

/// The longest any grace window in this module may wait, in milliseconds
/// (iter-166's value, frozen by iter-169).
///
/// iter-166 raised the post-commit window from iter-138's 300 ms to 2 000 ms
/// and measured the residual failure rate at 1 run in 12. iter-169 then
/// measured *why* the residual cases failed — a blocking round-trip inside
/// the wait loop was reading the status update off the wire and discarding it
/// (see [`with_event_replay`]) — so more waiting could never have helped.
/// Raising this constant is therefore the wrong fix for any future
/// `no_status_reported`; it is asserted by `unit_169_grace_budget_is_capped`
/// so a well-meant bump has to argue with a test first.
pub(crate) const MAX_STATUS_GRACE_MS: u64 = 2000;

/// How long the post-commit grace loop may keep waiting for the main
/// document's HTTP status, given the reason it does not have one yet.
///
/// The budget is re-derived on every pass rather than fixed up front, because
/// the reason itself changes as events arrive: a wait that starts out
/// `NoDocumentRequest` becomes `NoStatusReported` the moment the document's
/// `network-event` lands, and that is exactly when the longer budget should
/// apply.
///
/// * [`StatusUnknown::NotObserved`] — this route never correlates a document
///   request, so no amount of waiting can produce a status. Zero, rather than
///   spinning out a window on a condition that can never become true.
/// * [`StatusUnknown::NoStatusReported`] — the document's request has been
///   identified and it committed, so its response line exists and the update
///   carrying it is merely late. Worth waiting materially longer for; the
///   loop exits the instant it lands, so this costs nothing in the common
///   case.
/// * everything else ([`StatusUnknown::NoDocumentRequest`], or no reason yet)
///   — the `network-event` may still be in flight, but nothing guarantees one
///   is coming, so keep iter-138's short window.
pub(crate) fn status_grace_budget_ms(reason: Option<StatusUnknown>) -> u64 {
    match reason {
        Some(StatusUnknown::NotObserved) => 0,
        Some(StatusUnknown::NoStatusReported) => MAX_STATUS_GRACE_MS,
        _ => 300,
    }
}

/// Build the document-status tracker for the batch (`--with-network`) route
/// from the drained resource/update vectors (iter-138 Theme G — `navigate
/// --with-network` gets the status "for free" since it already captures every
/// request).
///
/// Split from the resolution step because the two happen at different points:
/// `all_updates` is consumed by `merge_updates` before `location.href` has been
/// evaluated, so the tracker is built early and
/// [`DocumentStatusTracker::resolve`] is called later, once the committed URL
/// is known. iter-166 replaced this function's private copy of the matching
/// rule — which had the same exact-string-URL bug as the streamed route's — so
/// there is now exactly one implementation of "which request was the document".
pub(crate) fn extract_document_status(
    resources: &[ff_rdp_core::NetworkResource],
    updates: &[ff_rdp_core::NetworkResourceUpdate],
) -> DocumentStatusTracker {
    let mut tracker = DocumentStatusTracker::observing();
    for r in resources {
        tracker.note_resource(r);
    }
    // `resources-updated-array` entries are incremental partial updates —
    // Firefox typically carries `status` only on the FIRST update for a
    // resource, with later updates (contentSize, totalTime, ...) leaving it
    // `None`. Taking the single most-recent update record (as `merge_updates`
    // does for *all* fields) would silently lose the status the instant a
    // second update arrives, so the tracker keeps every status-carrying update
    // and resolves to the last one — mirroring `merge_updates`'s own "last
    // non-None value wins per field" semantics rather than "last record wins
    // overall".
    for u in updates {
        tracker.note_update(u);
    }
    tracker
}
