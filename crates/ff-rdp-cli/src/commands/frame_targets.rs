//! One frame-target enumeration entry point that behaves identically with and
//! without the daemon (iteration 137 Theme A).
//!
//! # Why this module exists
//!
//! `ff_rdp_core::enumerate_frame_targets` works by issuing
//! `watchTargets("frame")` and draining the `target-available-form` events
//! Firefox pushes in response. That is only correct on a connection that is
//! **not already watching** frame targets: Firefox's
//! `ParentProcessWatcherRegistry.watchTargets` (see
//! `devtools/server/actors/watcher/ParentProcessWatcherRegistry.sys.mjs`)
//! just adds the type to the watcher's session data, so a second
//! subscription on the same connection re-delivers nothing.
//!
//! The ff-rdp daemon owns the single RDP connection to Firefox and subscribes
//! once, at startup. Every proxied command therefore issued a no-op
//! `watchTargets` and drained an empty window — `enumerate_frame_targets`
//! returned **zero** targets, not even the top-level one. That silently voided
//! everything iteration 129 shipped (`click --frame`, the cross-origin frame
//! scan, `consent accept`) for the default connection mode, while the same
//! commands worked under `--no-daemon`. The iteration-129 live tests all
//! passed `--no-daemon`, so nothing caught it.
//!
//! The fix: the daemon records every target form it sees and serves the
//! snapshot over a `{"to":"daemon","type":"frame-targets"}` request; this
//! module replays those raw packets through
//! [`ff_rdp_core::target_events_from_packets`] — the same add/replace/remove
//! rules the direct drain uses — so both modes produce the same
//! `Vec<TargetEvent>`.

use std::time::{Duration, Instant};

use ff_rdp_core::{
    DEFAULT_FRAME_TARGETS_SETTLE, TabActor, TargetEvent, enumerate_frame_targets,
    target_events_from_packets,
};
use serde_json::{Value, json};

use crate::commands::connect_tab::ConnectedTab;
use crate::error::AppError;

/// How long the daemon path re-polls the daemon's snapshot before answering.
///
/// Matched to [`DEFAULT_FRAME_TARGETS_SETTLE`] so a caller waits the same in
/// both connection modes: the direct path always drains for the full settle
/// window, and the daemon path polls for exactly as long before concluding
/// that it has seen every frame the page is going to create.
const DAEMON_SNAPSHOT_SETTLE: Duration = DEFAULT_FRAME_TARGETS_SETTLE;

/// Gap between daemon snapshot polls.
const DAEMON_SNAPSHOT_POLL: Duration = Duration::from_millis(50);

/// Enumerate this tab's window-global targets (top level + every same-origin
/// and cross-origin frame), using whichever mechanism the current connection
/// supports.
///
/// * **Daemon connection** — asks the daemon for its recorded target forms and
///   replays them, re-polling for [`DAEMON_SNAPSHOT_SETTLE`] and keeping the
///   largest snapshot, so a command issued immediately after `navigate` does
///   not race Firefox's frame spawning.
/// * **Direct connection** — the iteration-129 path: opt into server-side
///   target switching via `get_watcher_with_options(Some(true))` and drain the
///   live event stream.
///
/// **Callers MUST NOT call this more than once per CLI invocation** on a
/// direct connection — see `click.rs`'s `fetch_frame_targets` note: the second
/// `watchTargets` is a no-op and yields an empty list. The daemon path is
/// idempotent, but callers must not rely on that difference.
pub(crate) fn fetch_frame_targets(ctx: &mut ConnectedTab) -> Result<Vec<TargetEvent>, AppError> {
    if ctx.via_daemon {
        return fetch_via_daemon(ctx, DAEMON_SNAPSHOT_SETTLE);
    }
    fetch_direct(ctx)
}

/// Direct-connection enumeration (iteration-129 behaviour, unchanged).
fn fetch_direct(ctx: &mut ConnectedTab) -> Result<Vec<TargetEvent>, AppError> {
    let tab_actor = ctx.target_tab_actor().clone();
    let watcher_actor =
        TabActor::get_watcher_with_options(ctx.transport_mut(), &tab_actor, Some(true))
            .map_err(AppError::from)?;
    enumerate_frame_targets(
        ctx.transport_mut(),
        &watcher_actor,
        DEFAULT_FRAME_TARGETS_SETTLE,
    )
    .map_err(AppError::from)
}

/// Daemon-connection enumeration: replay the daemon's recorded target forms.
///
/// Polls because the daemon's snapshot is only as complete as what Firefox has
/// announced so far. A page whose frames are still being created reports the
/// top-level target alone for a while after `navigate` returns; giving up on
/// that first answer would reintroduce exactly the "0 frame(s) available"
/// class of lie this iteration removes, just with a narrower window.
fn fetch_via_daemon(
    ctx: &mut ConnectedTab,
    settle: Duration,
) -> Result<Vec<TargetEvent>, AppError> {
    let deadline = Instant::now() + settle;
    let mut latest = request_frame_targets(ctx)?;

    // Poll for the **whole** settle window rather than stopping at the first
    // answer that has more than one target. The direct path always drains its
    // full window, and frames that matter arrive late: a consent iframe on
    // theguardian.com appears a second or two after `navigate` returns, so an
    // early exit would report the page's `about:blank` placeholders and miss
    // the CMP — daemon and direct mode disagreeing again, just more subtly.
    //
    // The daemon may also still be bringing its subscription up (it started
    // while Firefox was tabless, or restarted and re-establishes on a
    // background thread); `watcher_ready` distinguishes "no frames" from
    // "cannot answer yet".
    while Instant::now() < deadline {
        std::thread::sleep(DAEMON_SNAPSHOT_POLL);
        let next = request_frame_targets(ctx)?;
        // Never regress to a shorter snapshot mid-poll: a `target-destroyed`
        // for a transient frame must not discard targets already observed.
        if next.watcher_ready
            && (!latest.watcher_ready || next.targets.len() > latest.targets.len())
        {
            latest = next;
        }
    }

    if !latest.watcher_ready {
        return Err(AppError::Unsupported {
            error_type: "daemon_watcher_not_ready",
            message: "the daemon has not established its frame-target subscription yet, \
                      so frame enumeration would report zero frames that do not reflect \
                      the page.\n\
                      hint: retry in a moment, run `ff-rdp daemon status` to check \
                      (`live_target_count`), or use --no-daemon for a direct connection."
                .to_owned(),
            details: None,
        });
    }

    Ok(latest.targets)
}

/// One `frame-targets` answer: the replayed targets plus whether the daemon's
/// subscription was live when it answered.
struct DaemonSnapshot {
    targets: Vec<TargetEvent>,
    watcher_ready: bool,
}

/// Issue one `{"to":"daemon","type":"frame-targets"}` request and parse the
/// reply.
///
/// The request travels over the same socket as ordinary RDP traffic, so the
/// reply is picked out by `from == "daemon" && type == "frame-targets"` while
/// forwarded Firefox pushes stream past.
fn request_frame_targets(ctx: &mut ConnectedTab) -> Result<DaemonSnapshot, AppError> {
    let transport = ctx.transport_mut();
    transport
        .send(&json!({"to": "daemon", "type": "frame-targets"}))
        .map_err(AppError::from)?;

    let reply = ff_rdp_core::transport::recv_event_from(transport, "daemon", |m| {
        m.get("type").and_then(Value::as_str) == Some("frame-targets")
    })
    .map_err(AppError::from)?;

    let packets: Vec<Value> = reply
        .get("targets")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    Ok(DaemonSnapshot {
        targets: target_events_from_packets(packets.iter()),
        // Absent field → treat as ready, so a newer CLI against an older
        // daemon degrades to the pre-readiness behaviour instead of refusing
        // every frame-aware command.
        watcher_ready: reply
            .get("watcher_ready")
            .and_then(Value::as_bool)
            .unwrap_or(true),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// AC: `unit_frame_targets_replay_matches_direct_rules` — replaying the
    /// daemon's recorded packets must apply the same dedupe/removal rules the
    /// live drain uses, so a daemon snapshot and a `--no-daemon` drain of the
    /// same event sequence yield the same targets.
    #[test]
    fn unit_frame_targets_replay_matches_direct_rules() {
        let packets = [
            json!({
                "type": "target-available-form",
                "target": {
                    "actor": "server0.conn0.windowGlobal1",
                    "url": "https://example.com/",
                    "targetType": "frame",
                    "isTopLevelTarget": true,
                    "consoleActor": "server0.conn0.console1",
                },
            }),
            json!({
                "type": "target-available-form",
                "target": {
                    "actor": "server0.conn0.windowGlobal2",
                    "url": "https://cmp.example.net/frame",
                    "targetType": "frame",
                    "isTopLevelTarget": false,
                    "consoleActor": "server0.conn0.console2",
                },
            }),
            // Re-announcement of the same frame must replace, not duplicate.
            json!({
                "type": "target-available-form",
                "target": {
                    "actor": "server0.conn0.windowGlobal2",
                    "url": "https://cmp.example.net/frame?v=2",
                    "targetType": "frame",
                    "isTopLevelTarget": false,
                    "consoleActor": "server0.conn0.console2",
                },
            }),
            json!({
                "type": "target-available-form",
                "target": {
                    "actor": "server0.conn0.windowGlobal3",
                    "url": "https://ads.example.org/",
                    "targetType": "frame",
                    "isTopLevelTarget": false,
                },
            }),
            json!({
                "type": "target-destroyed-form",
                "target": {
                    "actor": "server0.conn0.windowGlobal3",
                    "targetType": "frame",
                    "isTopLevelTarget": false,
                },
            }),
        ];

        let targets = target_events_from_packets(packets.iter());

        assert_eq!(
            targets.len(),
            2,
            "top level + one surviving frame (the destroyed one is gone, the \
             re-announced one is deduped): {targets:?}"
        );
        assert!(targets[0].is_top_level, "first-seen order is preserved");
        assert_eq!(
            targets[1].url.as_deref(),
            Some("https://cmp.example.net/frame?v=2"),
            "the later form replaces the earlier one for the same actor"
        );
    }

    /// AC: `unit_frame_targets_replay_empty_snapshot` — an empty daemon
    /// snapshot must yield an empty list rather than an error, so callers
    /// report "no frames" instead of failing the whole command.
    #[test]
    fn unit_frame_targets_replay_empty_snapshot() {
        let packets: Vec<Value> = Vec::new();
        assert!(target_events_from_packets(packets.iter()).is_empty());
    }
}
