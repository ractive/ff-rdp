//! Frame-target enumeration for one command's connection (iteration 137
//! Theme A).
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
//! Actor IDs identify actors only within one connection, so the targets
//! returned here are only meaningful on the connection that enumerated them.

use ff_rdp_core::{DEFAULT_FRAME_TARGETS_SETTLE, TabActor, TargetEvent, enumerate_frame_targets};

use crate::commands::connect_tab::ConnectedTab;
use crate::error::AppError;

/// Enumerate this tab's window-global targets (top level + every same-origin
/// and cross-origin frame): opt into server-side target switching via
/// `get_watcher_with_options(Some(true))` and drain the live event stream.
///
/// **Callers MUST NOT call this more than once per CLI invocation** — see
/// `click.rs`'s `fetch_frame_targets` note: the second `watchTargets` is a
/// no-op and yields an empty list.
pub(crate) fn fetch_frame_targets(ctx: &mut ConnectedTab) -> Result<Vec<TargetEvent>, AppError> {
    let started = std::time::Instant::now();
    tracing::debug!(target: "ff_rdp_cli::frame_targets", "FRAME_TARGETS_BEGIN pid={}", std::process::id());
    let tab_actor = ctx.target_tab_actor().clone();
    let result = TabActor::get_watcher_with_options(ctx.transport_mut(), &tab_actor, Some(true))
        .map_err(AppError::from)
        .and_then(|watcher_actor| {
            enumerate_frame_targets(
                ctx.transport_mut(),
                &watcher_actor,
                DEFAULT_FRAME_TARGETS_SETTLE,
            )
            .map_err(AppError::from)
        });
    tracing::debug!(target: "ff_rdp_cli::frame_targets", "FRAME_TARGETS_END pid={} elapsed_ns={} result={:?}", std::process::id(), started.elapsed().as_nanos(), result);
    result
}
