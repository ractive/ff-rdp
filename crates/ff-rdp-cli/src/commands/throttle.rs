//! `ff-rdp throttle` — drive the Firefox `NetworkParentActor`.
//!
//! Exposes the parent-process network-configuration surface: request throttling
//! (`setNetworkThrottling`) and URL blocking (`setBlockedUrls`).  A positional
//! `PROFILE` (`slow-3g`/`fast-3g`/`off`) sets the throttling tier; `--block`
//! replaces the URL block-list.  The two compose in a single call.
//!
//! # Prerequisite
//!
//! The network-parent actor throws `"Not listening for network events"` unless
//! `watchResources(["network-event"])` was issued on the owning watcher first,
//! so this command subscribes before configuring.
//!
//! # Lifetime
//!
//! As with `emulate`, throttling/blocking live only for the RDP connection that
//! set them.  Under the daemon that means "until the daemon restarts"; with
//! `--no-daemon` the one-shot process disconnects immediately, so the envelope
//! carries a `lifetime_warning` telling scripts the setting was discarded.

use ff_rdp_core::{NetworkParentFront, Registry, TabActor, ThrottleProfile, WatcherFront};
use serde_json::{Value, json};

use crate::cli::args::{Cli, ThrottleArgs, ThrottleProfileArg};
use crate::daemon::registry as daemon_registry;
use crate::daemon::throttle_state::{self, ThrottleState};
use crate::error::AppError;
use crate::output;
use crate::output_pipeline::OutputPipeline;

use super::connect_tab::connect_and_get_target;

/// Theme D (iter-131): throttling does not disable the HTTP cache, so a
/// `reload` while throttled can still look far faster than the profile alone
/// would suggest. Attached to every `throttle status` response so the caveat
/// travels with the data, not just `--help`.
const CACHE_CAVEAT: &str = "throttling does not bypass the HTTP cache — a `reload` while throttled \
     may still be served from cache and look far faster than the profile alone \
     would suggest; use `reload --hard` to force a network fetch";

/// Warning attached to the envelope when the configuration was applied over a
/// one-shot (`--no-daemon`) connection that disconnects immediately, dropping
/// the throttling/blocking on the floor.
pub(crate) const ONE_SHOT_LIFETIME_WARNING: &str = "throttle/block lifetime: this one-shot connection only — with --no-daemon the \
     configuration is discarded when this process disconnects; start the daemon \
     (drop --no-daemon) to keep throttling/blocking active across commands";

/// Map the CLI profile enum to the core `ThrottleProfile`, or `None` for
/// `off`/`status`. `run()` intercepts `Status` before this is ever called for
/// it (it is a read-only query, not something to send to Firefox); the arm
/// here exists only so the match stays exhaustive, and returns the same
/// "no throttling" value `off` would.
fn to_core_profile(arg: ThrottleProfileArg) -> Option<ThrottleProfile> {
    match arg {
        ThrottleProfileArg::Slow3g => Some(ThrottleProfile::Slow3g),
        ThrottleProfileArg::Fast3g => Some(ThrottleProfile::Fast3g),
        ThrottleProfileArg::Off | ThrottleProfileArg::Status => None,
    }
}

/// `profile` as the string stored/echoed in JSON (`"slow-3g"` / `"fast-3g"`),
/// or `None` for `off`.
fn profile_wire_name(arg: ThrottleProfileArg) -> Option<&'static str> {
    match arg {
        ThrottleProfileArg::Slow3g => Some("slow-3g"),
        ThrottleProfileArg::Fast3g => Some("fast-3g"),
        ThrottleProfileArg::Off | ThrottleProfileArg::Status => None,
    }
}

/// Return `true` when the user asked to touch the block-list at all
/// (`--block …` or `--unblock`). No `--block`/`--unblock` at all means "no
/// block flag was given".
fn wants_block_change(args: &ThrottleArgs) -> bool {
    args.unblock || !args.block.is_empty()
}

/// Resolve the block-list to send, treating `--unblock` and a single empty
/// `--block ''` pattern as "clear the list" (per the documented shorthand),
/// and any other `--block <pattern>…` as a literal replacement list.
fn resolve_block_urls(args: &ThrottleArgs) -> Vec<String> {
    if args.unblock || args.block.iter().all(String::is_empty) {
        Vec::new()
    } else {
        args.block.clone()
    }
}

pub fn run(cli: &Cli, args: &ThrottleArgs) -> Result<(), AppError> {
    // Theme D (iter-131): `status` is a read-only query — it never opens an
    // RDP connection or touches Firefox, so it is handled entirely
    // separately before any of the mutating validation/connection logic below.
    if args.profile == Some(ThrottleProfileArg::Status) {
        if wants_block_change(args) {
            return Err(AppError::User(
                "throttle status: read-only — remove --block/--unblock \
                 (combine a profile change with a separate `throttle <profile>` call)"
                    .to_owned(),
            ));
        }
        return run_status(cli);
    }

    // Argument validation before opening any connection: require at least one
    // action so `throttle` with no args is a descriptive error, not a no-op.
    if args.profile.is_none() && !wants_block_change(args) {
        return Err(AppError::User(
            "throttle: nothing to do — pass a PROFILE (slow-3g|fast-3g|off|status) \
             and/or --block <pattern>… (or --unblock to clear the block-list)"
                .to_owned(),
        ));
    }

    let mut ctx = connect_and_get_target(cli)?;
    let via_daemon = ctx.via_daemon;
    let tab_actor = ctx.target_tab_actor().clone();

    // Resolve watcher → network-parent actor.
    let watcher_actor =
        TabActor::get_watcher(ctx.transport_mut(), &tab_actor).map_err(AppError::from)?;
    let watcher_front = WatcherFront::new(
        watcher_actor.clone(),
        Registry::default(),
        Some(watcher_actor.clone()),
    );

    // Prerequisite: the network-parent actor only accepts throttling/blocking
    // once the session is listening for network events.
    watcher_front
        .watch_resources(ctx.transport_mut(), &["network-event"])
        .map_err(AppError::from)?;

    let network_parent_actor = watcher_front
        .get_network_parent_actor(ctx.transport_mut())
        .map_err(AppError::from)?;
    let network_parent =
        NetworkParentFront::new(network_parent_actor, Registry::default(), watcher_actor);

    // Apply throttling (if a profile was named). `to_core_profile` maps the
    // CLI enum to `Some(profile)` for a tier or `None` for `off`.
    let profile_echo: Value = if let Some(arg) = args.profile {
        if let Some(profile) = to_core_profile(arg) {
            network_parent
                .set_network_throttling(ctx.transport_mut(), profile)
                .map_err(AppError::from)?;
            json!(profile.as_str())
        } else {
            // `off` — clear throttling.
            network_parent
                .clear_network_throttling(ctx.transport_mut())
                .map_err(AppError::from)?;
            json!("off")
        }
    } else {
        Value::Null
    };

    // Theme D (iter-131): record what was just applied so a later
    // `throttle status` (a separate process, possibly minutes later) can
    // recall it. Only meaningful under the daemon — a one-shot connection
    // discards the setting on disconnect (see `ONE_SHOT_LIFETIME_WARNING`),
    // so there is nothing truthful to persist for it. Best-effort: a failure
    // to record state must not fail a `throttle` call that already succeeded
    // against Firefox; `throttle status` degrades to "nothing recorded".
    if via_daemon
        && let Some(arg) = args.profile
        && let Ok(Some(daemon_info)) = daemon_registry::read_registry(cli.port)
    {
        let state = ThrottleState {
            profile: profile_wire_name(arg).map(str::to_owned),
            set_at: chrono::Utc::now().to_rfc3339(),
            daemon_pid: daemon_info.pid,
        };
        let _ = throttle_state::write_throttle_state(cli.port, &state);
    }

    // Apply blocking (if requested). `--unblock` and a single empty
    // `--block ''` both clear the list; `--block <pats>` replaces it.
    let blocked_echo: Value = if wants_block_change(args) {
        let urls = resolve_block_urls(args);
        network_parent
            .set_blocked_urls(ctx.transport_mut(), &urls)
            .map_err(AppError::from)?;
        json!(urls)
    } else {
        Value::Null
    };

    let mut results = json!({
        "profile": profile_echo,
        "blocked_urls": blocked_echo,
    });
    if let Some(obj) = results.as_object_mut()
        && !via_daemon
    {
        obj.insert(
            "lifetime_warning".to_owned(),
            json!(ONE_SHOT_LIFETIME_WARNING),
        );
    }

    // Keep the default envelope lean: connection metadata is only attached
    // under --verbose, matching the other commands.
    let mut meta = if cli.is_verbose() {
        json!({ "via_daemon": via_daemon })
    } else {
        json!({})
    };
    crate::connection_meta::merge_into_if_verbose(
        &mut meta,
        &cli.host,
        cli.port,
        None,
        cli.is_verbose(),
    );

    let envelope = output::envelope(&results, 1, &meta);
    OutputPipeline::from_cli(cli)?.finalize(&envelope)
}

/// `throttle status` (Theme D, iter-131): report the profile last applied via
/// the daemon on `cli.port`. Never opens an RDP connection — Firefox's
/// network-parent actor has no getter, so there is nothing to query; this
/// reads the client-side bookkeeping [`run`] writes on a successful
/// `throttle <profile>` call.
fn run_status(cli: &Cli) -> Result<(), AppError> {
    let daemon_info = daemon_registry::read_registry(cli.port)
        .map_err(|e| AppError::from(anyhow::anyhow!("reading daemon registry: {e:#}")))?;

    let (profile, note): (Value, Option<&'static str>) = match daemon_info {
        None => (
            Value::Null,
            Some(
                "no daemon is running on this port — throttle state is not tracked \
                 without it (a --no-daemon connection discards its throttling on \
                 disconnect, so there is nothing durable to report)",
            ),
        ),
        Some(info) => {
            let state = throttle_state::read_throttle_state(cli.port)
                .map_err(|e| AppError::from(anyhow::anyhow!("reading throttle state: {e:#}")))?;
            match state {
                None => (
                    Value::Null,
                    Some(
                        "no `throttle <profile>` call has been recorded since this daemon started",
                    ),
                ),
                Some(s) if s.daemon_pid != info.pid => (
                    Value::Null,
                    Some(
                        "stale — the daemon has restarted since this profile was recorded; \
                         Firefox restarts with no throttling active",
                    ),
                ),
                Some(s) => (s.profile.map_or(Value::Null, |p| json!(p)), None),
            }
        }
    };

    let mut results = json!({
        "profile": profile,
        "cache_caveat": CACHE_CAVEAT,
    });
    if let (Some(n), Some(obj)) = (note, results.as_object_mut()) {
        obj.insert("note".to_owned(), json!(n));
    }

    let mut meta = json!({});
    crate::connection_meta::merge_into_if_verbose(
        &mut meta,
        &cli.host,
        cli.port,
        None,
        cli.is_verbose(),
    );

    let envelope = output::envelope(&results, 1, &meta);
    OutputPipeline::from_cli(cli)?.finalize(&envelope)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_args() -> ThrottleArgs {
        ThrottleArgs {
            profile: None,
            block: Vec::new(),
            unblock: false,
        }
    }

    #[test]
    fn to_core_profile_maps_tiers() {
        assert_eq!(
            to_core_profile(ThrottleProfileArg::Slow3g),
            Some(ThrottleProfile::Slow3g)
        );
        assert_eq!(
            to_core_profile(ThrottleProfileArg::Fast3g),
            Some(ThrottleProfile::Fast3g)
        );
        assert_eq!(to_core_profile(ThrottleProfileArg::Off), None);
        assert_eq!(to_core_profile(ThrottleProfileArg::Status), None);
    }

    // ── profile_wire_name (Theme D, iter-131) ────────────────────────────────

    #[test]
    fn profile_wire_name_maps_tiers_and_omits_off_status() {
        assert_eq!(
            profile_wire_name(ThrottleProfileArg::Slow3g),
            Some("slow-3g")
        );
        assert_eq!(
            profile_wire_name(ThrottleProfileArg::Fast3g),
            Some("fast-3g")
        );
        assert_eq!(profile_wire_name(ThrottleProfileArg::Off), None);
        assert_eq!(profile_wire_name(ThrottleProfileArg::Status), None);
    }

    #[test]
    fn wants_block_change_false_when_no_block_flags() {
        assert!(!wants_block_change(&base_args()));
    }

    #[test]
    fn wants_block_change_true_for_patterns() {
        let mut a = base_args();
        a.block = vec!["*.png".to_owned()];
        assert!(wants_block_change(&a));
    }

    #[test]
    fn wants_block_change_true_for_unblock() {
        let mut a = base_args();
        a.unblock = true;
        assert!(wants_block_change(&a));
    }

    #[test]
    fn resolve_block_urls_clears_for_unblock() {
        let mut a = base_args();
        a.unblock = true;
        assert_eq!(resolve_block_urls(&a), Vec::<String>::new());
    }

    #[test]
    fn resolve_block_urls_clears_for_single_empty_pattern() {
        // Documented shorthand (see `ThrottleArgs::block` doc comment / --help):
        // `--block ''` clears the list, same as `--unblock`.
        let mut a = base_args();
        a.block = vec![String::new()];
        assert_eq!(resolve_block_urls(&a), Vec::<String>::new());
    }

    #[test]
    fn resolve_block_urls_keeps_nonempty_patterns() {
        let mut a = base_args();
        a.block = vec!["*.png".to_owned(), "ads.example.com".to_owned()];
        assert_eq!(
            resolve_block_urls(&a),
            vec!["*.png".to_owned(), "ads.example.com".to_owned()]
        );
    }

    #[test]
    fn run_requires_at_least_one_action() {
        // No profile and no block flags → a descriptive user error before any
        // connection is attempted. We assert the validation branch directly
        // (the connect path needs a live Firefox).
        let args = base_args();
        assert!(args.profile.is_none() && !wants_block_change(&args));
    }
}
