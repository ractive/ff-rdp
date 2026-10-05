//! Network-quiet drain shared by `reload --wait-idle` and `navigate --wait-idle`
//! (iter-295), plus navigate's post-drain image check.

use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use ff_rdp_core::{
    ActorId, ProtocolError, RdpTransport, Resource, ResourceCommand, ResourceType, SubscriptionId,
    WatcherActor, parse_network_resource_updates, parse_network_resources,
};
use serde_json::{Value, json};

use crate::error::AppError;

use super::status::DocumentStatusTracker;

/// True when an I/O error kind signals the peer closed the connection
/// (as opposed to a real transport failure worth surfacing).
///
/// Windows tends to surface a half-closed socket as `ConnectionReset` or
/// `ConnectionAborted` where Unix accepts a final `write` into the send buffer
/// and only reveals the close on the next `read` as `UnexpectedEof`. We treat
/// all of these — plus `BrokenPipe` — as a clean teardown so a `send` that
/// races the server's close does not abort the whole wait-idle flow. This was
/// the iter-108 Windows CI red in `reload_wait_idle_no_traffic_returns_idle_quickly`:
/// the mock closes the connection right after its (empty) followup batch, so on
/// Windows the fire-and-forget `reload` send failed with `ConnectionReset` and
/// the command exited non-zero with an empty stderr (the JSON error envelope
/// went to stdout).
pub(crate) fn is_conn_closed_kind(kind: std::io::ErrorKind) -> bool {
    matches!(
        kind,
        std::io::ErrorKind::UnexpectedEof
            | std::io::ErrorKind::ConnectionReset
            | std::io::ErrorKind::ConnectionAborted
            | std::io::ErrorKind::BrokenPipe
    )
}

/// What one [`drain_until_idle`] run saw.
pub(crate) struct IdleDrain {
    /// Network resources announced (`resources-available-array` only).
    pub(crate) requests_observed: u64,
    /// Milliseconds from the start of the drain to its end.
    pub(crate) idle_at_ms: u64,
    /// `false` when the drain stopped because `timeout_ms` ran out while
    /// requests were still arriving.
    pub(crate) reached_idle: bool,
}

/// Drain `network-event` resources from `transport` until none has arrived for
/// `idle_ms`, or `timeout_ms` has passed since the drain started.
///
/// `idle_from_start` decides when the quiet clock starts. `reload` sets it
/// `false`: the reload it just sent always produces a document request, so
/// the clock waits for the first event. `navigate` drains after the document
/// has committed, when a quiet page may produce no event at all, so the clock
/// starts with the drain and such a page is idle after `idle_ms`.
///
/// `requests_observed` counts resources from `resources-available-array` only:
/// every request also produces one or more `resources-updated-array` entries,
/// and counting those too roughly doubled the figure (dogfooding-session-64
/// #19: 293 on a 148-request page). Both kinds reset the idle timer and feed
/// `tracker`, which correlates the document's status.
///
/// A connection the peer closed ends the drain as idle. The socket's read
/// timeout is restored to `cli_timeout` on every return path.
pub(crate) fn drain_until_idle(
    transport: &mut RdpTransport,
    idle_ms: u64,
    timeout_ms: u64,
    cli_timeout: u64,
    idle_from_start: bool,
    mut tracker: Option<&mut DocumentStatusTracker>,
) -> Result<IdleDrain, AppError> {
    let poll_interval = Duration::from_millis(100);
    transport
        .set_read_timeout(Some(poll_interval))
        .map_err(AppError::from)?;

    let start = Instant::now();
    let total_deadline = Duration::from_millis(timeout_ms);
    let idle_threshold = Duration::from_millis(idle_ms);

    let mut requests_observed: u64 = 0;
    let mut last_event_at: Option<Instant> = idle_from_start.then_some(start);
    let mut reached_idle = false;

    loop {
        if let Some(t) = last_event_at
            && t.elapsed() >= idle_threshold
        {
            reached_idle = true;
            break;
        }
        if start.elapsed() >= total_deadline {
            break;
        }

        match transport.recv() {
            Ok(msg) => {
                let msg_type = msg.get("type").and_then(Value::as_str).unwrap_or_default();
                tracing::debug!(
                    msg_type,
                    elapsed_ms = start.elapsed().as_millis(),
                    "idle drain: message"
                );
                if msg_type == "resources-available-array" {
                    let n = count_network_events(&msg);
                    if n > 0 {
                        requests_observed += n;
                        last_event_at = Some(Instant::now());
                    }
                    if let Some(t) = tracker.as_deref_mut() {
                        for res in parse_network_resources(&msg) {
                            t.note_resource(&res);
                        }
                    }
                } else if msg_type == "resources-updated-array" {
                    if let Some(t) = tracker.as_deref_mut() {
                        for upd in parse_network_resource_updates(&msg) {
                            t.note_update(&upd);
                        }
                    }
                    last_event_at = Some(Instant::now());
                }
            }
            Err(ProtocolError::Timeout) => {}
            Err(ProtocolError::RecvFailed(ref e)) if is_conn_closed_kind(e.kind()) => {
                reached_idle = true;
                break;
            }
            Err(e) => {
                let _ = transport.set_read_timeout(Some(Duration::from_millis(cli_timeout)));
                return Err(AppError::from(e));
            }
        }
    }

    let idle_at_ms = u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX);
    let _ = transport.set_read_timeout(Some(Duration::from_millis(cli_timeout)));

    Ok(IdleDrain {
        requests_observed,
        idle_at_ms,
        reached_idle,
    })
}

/// Count the `network-event` resources in a watcher batch message. Batches
/// for other resource types count zero.
fn count_network_events(msg: &Value) -> u64 {
    msg.get("array").and_then(Value::as_array).map_or(0, |arr| {
        arr.iter()
            .filter_map(Value::as_array)
            .filter(|pair| pair.first().and_then(Value::as_str) == Some("network-event"))
            .filter_map(|p| p.get(1))
            .filter_map(Value::as_array)
            .map(Vec::len)
            .sum::<usize>()
    }) as u64
}

/// The JS `navigate --wait-idle` polls once the network is quiet: true when
/// every `document.images` entry is `complete`, except `loading="lazy"`
/// images entirely outside the viewport.
///
/// `img.complete` is the cheapest signal that nothing is still loading or
/// decoding, and it is also true for broken images. Calling `decode()`
/// instead would force decodes the page never asked for and skew the
/// measurements the caller is waiting to take (iter-295 design note).
///
/// The lazy exemption exists because the plan's premise that a
/// never-requested lazy image reports `complete` does not hold in Firefox:
/// measured on en.wikipedia.org/wiki/Matterhorn after the network went quiet,
/// 76 of 86 images were `complete === false`, every one `loading="lazy"` and
/// below the fold. Firefox will not request those until they scroll near the
/// viewport, so waiting on them always exhausted the budget. Lazy images that
/// intersect the viewport — the ones a screenshot shows — are still waited
/// for; making off-screen ones load is the viewport plan's job.
const IMAGES_COMPLETE_JS: &str = "[...document.images].every(i => { \
    if (i.complete) return true; \
    if (i.loading !== 'lazy') return false; \
    const r = i.getBoundingClientRect(); \
    return !(r.bottom > 0 && r.right > 0 && r.top < innerHeight && r.left < innerWidth); \
})";

/// The off-screen lazy images [`IMAGES_COMPLETE_JS`] exempted, reported as
/// `lazy_images_deferred`. Same predicate rather than "every incomplete
/// image", so an image the page inserts between the two evals is not
/// miscounted as deferred.
const LAZY_DEFERRED_JS: &str = "[...document.images].filter(i => { \
    if (i.complete || i.loading !== 'lazy') return false; \
    const r = i.getBoundingClientRect(); \
    return !(r.bottom > 0 && r.right > 0 && r.top < innerHeight && r.left < innerWidth); \
}).length";

/// The shortest quiet window `navigate --wait-idle` honours.
///
/// The watcher batches resource emission with `RESOURCES_THROTTLING_DELAY =
/// 100` ms (`devtools/server/actors/watcher.js`), so a page that requests
/// something every 50 ms still reaches the client as one batch per ~100 ms.
/// Any window at or near 100 ms reads such a page as idle between batches
/// (measured: `--idle-ms 100` on a 50 ms poller returned idle with one request
/// observed), so shorter `--idle-ms` values are raised to two batch periods.
const MIN_IDLE_MS: u64 = 200;

/// How `navigate --wait-idle` keeps `network-event` watched across the
/// navigation, so that requests the page starts right after `load` — between
/// the commit and the drain — are announced rather than lost.
pub(super) enum IdleWatch {
    /// `--wait-strategy readystate`: a raw `watchResources(["network-event"])`
    /// sent before `navigateTo`; released with `unwatchResources`.
    Raw(ActorId),
    /// `events`/`both`: a second `NetworkEvent` subscription on the
    /// navigation's resource bus. It holds the type's ref-count above zero
    /// when the commit wait unsubscribes, so the bus does not unwatch it; the
    /// receiver is kept alive so `gc` does not prune it, and holds the
    /// requests the bus dispatched during the commit wait.
    Bus {
        bus: Arc<Mutex<ResourceCommand>>,
        id: SubscriptionId,
        rx: Receiver<Arc<Resource>>,
    },
}

impl IdleWatch {
    /// Subscribe the idle watch on `bus` (no wire call when the commit wait's
    /// own subscription already watches `network-event`).
    pub(super) fn subscribe_bus(
        transport: &mut RdpTransport,
        bus: &Arc<Mutex<ResourceCommand>>,
    ) -> Result<Self, AppError> {
        let (id, rx) = bus
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .subscribe(transport, &[ResourceType::NetworkEvent])
            .map_err(AppError::from)?;
        Ok(Self::Bus {
            bus: Arc::clone(bus),
            id,
            rx,
        })
    }

    /// Best-effort release; a teardown error must not mask the result.
    ///
    /// Returns the requests announced before the drain started: what the bus
    /// dispatched to this subscription during the commit wait. The raw watch
    /// has no such record (the readystate wait's evals discard events), so it
    /// returns 0.
    fn release(self, transport: &mut RdpTransport) -> u64 {
        match self {
            Self::Raw(watcher) => {
                let _ = WatcherActor::unwatch_resources(transport, &watcher, &["network-event"]);
                0
            }
            Self::Bus { bus, id, rx } => {
                let _ = bus
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .unsubscribe(transport, id);
                rx.try_iter()
                    .filter(|r| matches!(**r, Resource::NetworkEvent(_)))
                    .count() as u64
            }
        }
    }
}

/// `navigate --wait-idle`: after the document has committed, drain
/// `network-event`s until the network has been quiet for `idle_ms`, then poll
/// [`IMAGES_COMPLETE_JS`] until it is true — both inside one `budget_ms`
/// measured from the start of the drain.
///
/// `watch` was set up before `navigateTo` and is released here;
/// `requests_observed` counts the requests announced since then. A request
/// that sends nothing for longer than the quiet window is not waited for;
/// the image check covers the images among such requests.
///
/// Returns `{"idle_at_ms", "requests_observed", "images_complete",
/// "lazy_images_deferred"}`, or a
/// timeout error naming `--wait-idle` when the budget ends first.
pub(super) fn wait_for_idle(
    ctx: &mut crate::commands::connect_tab::ConnectedTab,
    watch: IdleWatch,
    idle_ms: u64,
    budget_ms: u64,
    cli_timeout: u64,
) -> Result<Value, AppError> {
    let idle_ms = idle_ms.max(MIN_IDLE_MS);
    let started = Instant::now();
    let drained = drain_until_idle(
        ctx.transport_mut(),
        idle_ms,
        budget_ms,
        cli_timeout,
        true,
        None,
    );
    // Release before the error check, so a timeout does not leave the server
    // streaming events at a connection about to close.
    let announced_before_drain = watch.release(ctx.transport_mut());
    let drained = drained?;
    // Like `reload --wait-idle`, count every request this command saw from
    // `navigateTo` on, not only those after the commit: a page whose load
    // event fires late has its post-load requests announced during the commit
    // wait, before the drain starts.
    let requests_observed = announced_before_drain + drained.requests_observed;
    if !drained.reached_idle {
        return Err(AppError::Timeout(budget_exhausted_message(
            idle_ms,
            budget_ms,
            &format!(
                "the network never stayed quiet for {idle_ms} ms ({requests_observed} requests observed)"
            ),
        )));
    }

    let remaining = budget_ms.saturating_sub(elapsed_ms(started));
    if remaining == 0 {
        return Err(AppError::Timeout(budget_exhausted_message(
            idle_ms,
            budget_ms,
            "no budget was left for the image check",
        )));
    }
    // The pre-navigation console actor died with the old document.
    super::refresh_console_actor(ctx);
    let console_actor = ctx.target().console_actor.clone();
    let timeout_msg = budget_exhausted_message(
        idle_ms,
        budget_ms,
        "document.images were still loading when the budget ran out",
    );
    super::super::js_helpers::poll_js_condition(
        ctx,
        &console_actor,
        IMAGES_COMPLETE_JS,
        remaining,
        "navigate --wait-idle: image check threw",
        &timeout_msg,
    )?;

    let idle_at_ms = elapsed_ms(started);
    // Informational only: a failed count is `null`, never an error.
    let lazy_deferred = ff_rdp_core::WebConsoleActor::evaluate_js_async(
        ctx.transport_mut(),
        &console_actor,
        LAZY_DEFERRED_JS,
    )
    .ok()
    .and_then(|r| match r.result {
        ff_rdp_core::Grip::Value(v) => v.as_u64(),
        _ => None,
    });

    Ok(json!({
        "idle_at_ms": idle_at_ms,
        "requests_observed": requests_observed,
        "images_complete": true,
        "lazy_images_deferred": lazy_deferred,
    }))
}

fn elapsed_ms(since: Instant) -> u64 {
    u64::try_from(since.elapsed().as_millis()).unwrap_or(u64::MAX)
}

fn budget_exhausted_message(idle_ms: u64, budget_ms: u64, why: &str) -> String {
    format!(
        "navigate --wait-idle timed out after {budget_ms} ms (--timeout-ms): {why}; \
         raise --timeout-ms or lower --idle-ms (effective quiet window {idle_ms} ms, \
         minimum {MIN_IDLE_MS})"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn count_network_events_ignores_other_resource_types() {
        let msg = json!({
            "type": "resources-available-array",
            "array": [
                ["network-event", [{"actor": "a"}, {"actor": "b"}]],
                ["document-event", [{"name": "dom-complete"}]],
            ]
        });
        assert_eq!(count_network_events(&msg), 2);
        assert_eq!(count_network_events(&json!({"type": "x"})), 0);
    }

    #[test]
    fn budget_message_names_the_flag_and_both_knobs() {
        let msg = budget_exhausted_message(100, 200, "busy");
        assert!(msg.contains("--wait-idle"), "{msg}");
        assert!(msg.contains("--timeout-ms"), "{msg}");
        assert!(msg.contains("--idle-ms"), "{msg}");
        assert!(msg.contains("busy"), "{msg}");
    }

    #[test]
    fn conn_closed_kinds_are_treated_as_teardown() {
        use std::io::ErrorKind;
        for kind in [
            ErrorKind::UnexpectedEof,
            ErrorKind::ConnectionReset,
            ErrorKind::ConnectionAborted,
            ErrorKind::BrokenPipe,
        ] {
            assert!(is_conn_closed_kind(kind), "{kind:?}");
        }
        for kind in [
            ErrorKind::TimedOut,
            ErrorKind::PermissionDenied,
            ErrorKind::NotConnected,
        ] {
            assert!(!is_conn_closed_kind(kind), "{kind:?}");
        }
    }
}
