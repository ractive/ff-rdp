//! Playbook-scoped `network-event` subscription for `ff-rdp run` (iter-181).
//!
//! # Why this exists
//!
//! `run` executes every step against its own short-lived connection. Before
//! this module, `assert_network` therefore armed a `network-event` watcher
//! *when the step started* and unwatched when it finished. Firefox's
//! `watchResources` delivers events that occur while watching — it does not
//! replay history — so the canonical playbook shape
//!
//! ```json
//! {"click":  {"selector": "button[type=submit]"}},
//! {"assert_network": {"url_contains": "/api/auth/sign-in", "status": 200}}
//! ```
//!
//! was a race between the arming sequence (connect → `getWatcher` →
//! `watchResources`) and the response. Idle, the arming usually won; loaded, it
//! lost, and because such a playbook has exactly one request in flight, losing
//! produced `events_in_buffer: 0` rather than a partial count. Iteration 179
//! measured it: 4/4 pass idle, 8/8 fail under a `-j6` load generator.
//!
//! # What it does instead
//!
//! [`PlaybookNetworkWatch`] holds **one** connection open for the whole script
//! with the watcher armed before the first step runs, and accumulates every
//! resource and update it sees. `assert_network` then reads an accumulated
//! buffer instead of arming its own: a request triggered by step N is still
//! visible at step N+1, however long the intervening steps take.
//!
//! # Between steps, events queue in the kernel
//!
//! Nothing reads the subscription socket while an ordinary step runs, so events
//! accumulate in the receive buffer and are picked up by the next
//! [`PlaybookNetworkWatch::poll`]. They are not lost — but a very long playbook
//! against a very chatty page can fill that buffer, after which Firefox queues
//! the frames on its side instead. Draining between steps was considered and
//! rejected: it needs a read timeout short enough to be free (single-digit ms),
//! and [`ff_rdp_core::transport::recv_from`] reads a frame with `read_exact`,
//! so a timeout landing mid-frame desyncs the stream. A queue is better than a
//! desync.
//!
//! # What it deliberately does not do
//!
//! It does not touch the **daemon** route. The daemon already holds a standing
//! subscription that buffers across steps, so [`PlaybookNetworkWatch::arm`]
//! detects that route and returns `Ok(None)`, leaving `assert_network` on its
//! existing daemon drain.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use ff_rdp_core::{ActorId, NetworkResource, NetworkResourceUpdate, TabActor, WatcherActor};
use serde_json::Value;

use crate::cli::args::Cli;
use crate::error::AppError;

use super::connect_tab::{ConnectedTab, connect_and_get_target};
use super::network_events::{build_network_entries, drain_network_events_timed, fold_update};

/// Upper bound on requests retained by a single playbook subscription.
///
/// A watcher armed for the whole script keeps receiving events while the
/// playbook runs, so — unlike the per-step drain it replaces — its buffer has
/// no natural end. A long playbook against a chatty page (ad-heavy sites push
/// hundreds of requests per minute) would otherwise grow without limit. When
/// the cap is exceeded the **oldest** requests are dropped, because
/// `assert_network` asks about what just happened, never about the first page
/// load. Dropped requests are counted and reported in the failure diagnostics
/// so a miss caused by eviction is never silent.
const MAX_BUFFERED_REQUESTS: usize = 4096;

/// Wall-clock slice one [`PlaybookNetworkWatch::poll`] spends reading.
///
/// Matches the read-timeout granularity inside
/// [`drain_network_events_timed`], so a poll costs one read cycle and the
/// caller keeps control between slices (it can stop as soon as its predicate
/// matches rather than burning the whole step timeout).
pub(crate) const POLL_SLICE: Duration = Duration::from_millis(250);

/// A `network-event` subscription that outlives the step that reads it.
///
/// Owns its own connection to Firefox. See the [module docs](self) for why the
/// per-step alternative is a race.
pub(crate) struct PlaybookNetworkWatch {
    /// The dedicated connection. Kept alive for the whole script; steps
    /// continue to use their own connections.
    ctx: ConnectedTab,
    /// The armed watcher, needed to unwatch on drop.
    watcher_actor: ActorId,
    /// Requests seen so far, oldest first.
    resources: Vec<NetworkResource>,
    /// Response metadata folded by `resource_id`, so repeated updates for one
    /// request collapse instead of accumulating.
    update_map: HashMap<u64, NetworkResourceUpdate>,
    /// How many requests were evicted by [`MAX_BUFFERED_REQUESTS`].
    evicted: usize,
}

impl PlaybookNetworkWatch {
    /// Connect, arm a `network-event` watcher, and keep both.
    ///
    /// Returns `Ok(None)` when the connection resolved to the **daemon**,
    /// which already holds a standing subscription — arming a second one there
    /// would duplicate events and change what `route: "daemon"` reports.
    ///
    /// The route is discovered by connecting rather than by re-deciding the
    /// daemon policy here, so this subscription can never disagree with the
    /// route the steps themselves take.
    pub(crate) fn arm(cli: &Cli) -> Result<Option<Self>, AppError> {
        let mut ctx = connect_and_get_target(cli)?;
        if ctx.via_daemon {
            return Ok(None);
        }

        let tab_actor = ctx.target_tab_actor().clone();
        let watcher_actor =
            TabActor::get_watcher(ctx.transport_mut(), &tab_actor).map_err(AppError::from)?;
        WatcherActor::watch_resources(ctx.transport_mut(), &watcher_actor, &["network-event"])
            .map_err(AppError::from)?;

        Ok(Some(Self {
            ctx,
            watcher_actor,
            resources: Vec::new(),
            update_map: HashMap::new(),
            evicted: 0,
        }))
    }

    /// Read whatever has arrived, for at most `budget`, into the buffer.
    ///
    /// Events that arrived earlier are already queued on the socket, so a poll
    /// after a long step returns them immediately; the budget only bounds how
    /// long we wait for events that have not arrived yet.
    pub(crate) fn poll(&mut self, budget: Duration) -> Result<(), AppError> {
        let (resources, updates, _) =
            drain_network_events_timed(self.ctx.transport_mut(), budget).map_err(AppError::from)?;
        self.resources.extend(resources);
        for update in updates {
            fold_update(&mut self.update_map, update);
        }
        self.evict_overflow();
        Ok(())
    }

    /// Drop the oldest requests, and their updates, once over the cap.
    fn evict_overflow(&mut self) {
        self.evicted += evict_overflow_from(
            &mut self.resources,
            &mut self.update_map,
            MAX_BUFFERED_REQUESTS,
        );
    }

    /// The accumulated buffer as `assert_network`-shaped JSON entries.
    pub(crate) fn entries(&self) -> Vec<Value> {
        build_network_entries(&self.resources, &self.update_map)
    }

    /// How many requests eviction has discarded so far.
    ///
    /// Reported in `assert_network`'s failure diagnostics: a non-zero value
    /// means "not found" could mean "found, then evicted".
    pub(crate) fn evicted(&self) -> usize {
        self.evicted
    }
}

/// Drop the oldest `resources.len() - cap` entries (and their folded updates),
/// returning how many were dropped.
///
/// Pulled out of [`PlaybookNetworkWatch::evict_overflow`] so the eviction
/// policy is unit-testable on plain `Vec`/`HashMap` values — the struct it
/// lives on otherwise needs a live Firefox connection to construct via
/// [`PlaybookNetworkWatch::arm`].
fn evict_overflow_from(
    resources: &mut Vec<NetworkResource>,
    update_map: &mut HashMap<u64, NetworkResourceUpdate>,
    cap: usize,
) -> usize {
    let excess = resources.len().saturating_sub(cap);
    if excess == 0 {
        return 0;
    }
    for dropped in resources.drain(..excess) {
        update_map.remove(&dropped.resource_id);
    }
    excess
}

impl Drop for PlaybookNetworkWatch {
    /// Best-effort unwatch. The connection closing would end the subscription
    /// anyway, but an explicit `unwatchResources` lets Firefox tear the
    /// watcher down without waiting for the socket to drop, and it runs on
    /// every exit path — including a step that returned `Err` and bailed the
    /// script — because it is a `Drop`, not a call the runner has to remember.
    fn drop(&mut self) {
        let watcher = self.watcher_actor.clone();
        let _ =
            WatcherActor::unwatch_resources(self.ctx.transport_mut(), &watcher, &["network-event"]);
    }
}

/// Poll `watch` until `predicate` matches an entry or `deadline` passes.
///
/// Returns the buffer as of the last look, so the caller can report
/// `events_in_buffer` for the same snapshot it decided on. Checking **before**
/// the first poll is deliberate: when the request already completed during an
/// earlier step it is in the buffer already, and the assertion must not pay a
/// poll slice to discover that.
pub(crate) fn wait_for_match(
    watch: &mut PlaybookNetworkWatch,
    deadline: Instant,
    predicate: &dyn Fn(&Value) -> bool,
) -> Result<(Vec<Value>, bool), AppError> {
    loop {
        let entries = watch.entries();
        if entries.iter().any(predicate) {
            return Ok((entries, true));
        }
        let now = Instant::now();
        if now >= deadline {
            return Ok((entries, false));
        }
        let budget = (deadline - now).min(POLL_SLICE);
        watch.poll(budget)?;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A minimal [`NetworkResource`] distinguished only by `resource_id`,
    /// which is all [`evict_overflow_from`] looks at.
    fn resource(resource_id: u64) -> NetworkResource {
        NetworkResource {
            actor: ActorId::from("net0"),
            method: "GET".to_owned(),
            url: format!("https://example.com/{resource_id}"),
            is_xhr: false,
            cause_type: "fetch".to_owned(),
            started_date_time: String::new(),
            timestamp: 0.0,
            resource_id,
        }
    }

    /// Under the cap, nothing is dropped and the reported count is zero.
    #[test]
    fn unit_181_evict_overflow_under_cap_drops_nothing() {
        let mut resources: Vec<NetworkResource> = (0..5).map(resource).collect();
        let mut updates = HashMap::new();
        let dropped = evict_overflow_from(&mut resources, &mut updates, 10);
        assert_eq!(dropped, 0);
        assert_eq!(resources.len(), 5);
    }

    /// Over the cap, the **oldest** entries go — `assert_network` asks about
    /// what just happened, never about the first request of a long run — and
    /// their folded updates go with them so `update_map` cannot outlive the
    /// resource it describes.
    #[test]
    fn unit_181_evict_overflow_drops_oldest_first_with_their_updates() {
        let mut resources: Vec<NetworkResource> = (0..10).map(resource).collect();
        let mut updates: HashMap<u64, NetworkResourceUpdate> = (0..10)
            .map(|id| (id, NetworkResourceUpdate::default()))
            .collect();

        let dropped = evict_overflow_from(&mut resources, &mut updates, 4);

        assert_eq!(dropped, 6);
        assert_eq!(resources.len(), 4);
        // The four newest (ids 6..10) survive, oldest-first order preserved.
        let remaining_ids: Vec<u64> = resources.iter().map(|r| r.resource_id).collect();
        assert_eq!(remaining_ids, vec![6, 7, 8, 9]);
        // Updates for evicted resources are gone; updates for survivors are not.
        for id in 0..6 {
            assert!(
                !updates.contains_key(&id),
                "resource {id} should be evicted"
            );
        }
        for id in 6..10 {
            assert!(updates.contains_key(&id), "resource {id} should survive");
        }
    }

    /// A cap of zero is a degenerate but legal input: everything goes.
    #[test]
    fn unit_181_evict_overflow_cap_zero_drops_everything() {
        let mut resources: Vec<NetworkResource> = (0..3).map(resource).collect();
        let mut updates = HashMap::new();
        let dropped = evict_overflow_from(&mut resources, &mut updates, 0);
        assert_eq!(dropped, 3);
        assert!(resources.is_empty());
    }
}
