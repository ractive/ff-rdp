//! `--throttle` / `--block` on `navigate` and `reload`.
//!
//! Throttling and URL blocking are configured on the parent-process
//! `NetworkParentActor` obtained from the tab's watcher, and they live exactly
//! as long as the RDP connection that set them. ff-rdp opens one connection
//! per command, so the only place they can take effect is on the connection
//! that then performs the navigation: these are flags on the navigating
//! commands, applied right before they navigate, not a standalone command.
//!
//! # Prerequisite
//!
//! The network-parent actor throws `"Not listening for network events"` unless
//! `watchResources(["network-event"])` was issued on the owning watcher first,
//! so [`apply`] subscribes before configuring.

use ff_rdp_core::{ActorId, NetworkParentFront, Registry, ThrottleProfile, WatcherFront};
use serde_json::{Value, json};

use crate::cli::args::{NetworkConditionsArgs, ThrottleProfileArg};
use crate::error::AppError;

use super::connect_tab::ConnectedTab;

fn to_core_profile(arg: ThrottleProfileArg) -> ThrottleProfile {
    match arg {
        ThrottleProfileArg::Slow3g => ThrottleProfile::Slow3g,
        ThrottleProfileArg::Fast3g => ThrottleProfile::Fast3g,
    }
}

/// Patterns to send for `--block`. A single empty pattern means "no list" —
/// sending `[""]` would block every URL.
fn block_patterns(args: &NetworkConditionsArgs) -> Vec<String> {
    args.block.iter().filter(|p| !p.is_empty()).cloned().collect()
}

/// Whether the caller asked for any network condition at all.
pub(crate) fn is_requested(args: &NetworkConditionsArgs) -> bool {
    args.throttle.is_some() || !block_patterns(args).is_empty()
}

/// Apply `args` on `ctx`'s connection, returning the
/// `{throttle, blocked_urls}` echo for the command's `results`, or `None`
/// when nothing was requested (no round trip is made then).
///
/// `watcher_actor` is the watcher the caller already obtained for its
/// navigation. It must be passed in rather than fetched here: a tab
/// descriptor creates its watcher once per connection, with the options of
/// the first `getWatcher` call, and the navigating commands need theirs
/// created with server-side target switching (see
/// `navigate::get_navigation_watcher`).
///
/// The settings end when `ctx` disconnects, i.e. when the command exits.
pub(crate) fn apply(
    ctx: &mut ConnectedTab,
    watcher_actor: &ActorId,
    args: &NetworkConditionsArgs,
) -> Result<Option<Value>, AppError> {
    if !is_requested(args) {
        return Ok(None);
    }
    let watcher_actor = watcher_actor.clone();
    let watcher_front = WatcherFront::new(
        watcher_actor.clone(),
        Registry::default(),
        Some(watcher_actor.clone()),
    );
    watcher_front
        .watch_resources(ctx.transport_mut(), &["network-event"])
        .map_err(AppError::from)?;
    let network_parent_actor = watcher_front
        .get_network_parent_actor(ctx.transport_mut())
        .map_err(AppError::from)?;
    let network_parent =
        NetworkParentFront::new(network_parent_actor, Registry::default(), watcher_actor);

    let throttle_echo = match args.throttle {
        Some(arg) => {
            let profile = to_core_profile(arg);
            network_parent
                .set_network_throttling(ctx.transport_mut(), profile)
                .map_err(AppError::from)?;
            json!(profile.as_str())
        }
        None => Value::Null,
    };

    let patterns = block_patterns(args);
    let blocked_echo = if patterns.is_empty() {
        Value::Null
    } else {
        network_parent
            .set_blocked_urls(ctx.transport_mut(), &patterns)
            .map_err(AppError::from)?;
        json!(patterns)
    };

    Ok(Some(json!({
        "throttle": throttle_echo,
        "blocked_urls": blocked_echo,
    })))
}

/// Attach the [`apply`] echo to a command's `results` object under
/// `network_conditions`.
pub(crate) fn insert_echo(results: &mut Value, echo: Option<Value>) {
    if let (Some(echo), Some(obj)) = (echo, results.as_object_mut()) {
        obj.insert("network_conditions".to_owned(), echo);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(throttle: Option<ThrottleProfileArg>, block: &[&str]) -> NetworkConditionsArgs {
        NetworkConditionsArgs {
            throttle,
            block: block.iter().map(|s| (*s).to_owned()).collect(),
        }
    }

    #[test]
    fn nothing_requested_without_flags() {
        assert!(!is_requested(&args(None, &[])));
        assert!(!is_requested(&args(None, &[""])), "an empty pattern blocks nothing");
    }

    #[test]
    fn requested_with_throttle_or_block() {
        assert!(is_requested(&args(Some(ThrottleProfileArg::Slow3g), &[])));
        assert!(is_requested(&args(None, &["*.png"])));
    }

    #[test]
    fn empty_patterns_are_dropped() {
        assert_eq!(block_patterns(&args(None, &["", "*.gif"])), vec!["*.gif"]);
    }

    #[test]
    fn profiles_map_to_core() {
        assert_eq!(
            to_core_profile(ThrottleProfileArg::Slow3g).as_str(),
            "slow-3g"
        );
        assert_eq!(
            to_core_profile(ThrottleProfileArg::Fast3g).as_str(),
            "fast-3g"
        );
    }

    /// iter-164: the patterns the user typed must reach the network-parent
    /// actor as `setBlockedUrls({urls: [...]})`, unchanged and in order — so
    /// "never sent" and "sent but not enforced" stay distinguishable without
    /// a live Firefox.
    #[test]
    fn block_patterns_reach_the_actor() {
        use std::net::{TcpListener, TcpStream};
        use std::time::Duration;

        use ff_rdp_core::{FramedReader, FramedWriter, RdpTransport};

        let urls = block_patterns(&args(None, &["favicon", "*.png"]));
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind stub Firefox");
        let addr = listener.local_addr().expect("stub addr");
        let server = std::thread::spawn(move || {
            let (stream, _) = listener.accept().expect("accept");
            let reply_stream: TcpStream = stream.try_clone().expect("clone stub stream");
            let mut reader = FramedReader::from_stream(stream);
            let req = reader.recv().expect("stub read request");
            // `setBlockedUrls` declares no response block but is not oneway —
            // Firefox still sends an empty ACK the client must read.
            let mut writer = FramedWriter::from_stream(reply_stream);
            writer
                .send(&json!({"from": "server1.conn0.networkParent9"}))
                .expect("stub ack");
            req
        });

        let mut transport =
            RdpTransport::connect_raw("127.0.0.1", addr.port(), Duration::from_secs(5))
                .expect("connect to stub Firefox");
        let front = NetworkParentFront::new(
            ActorId::from("server1.conn0.networkParent9"),
            Registry::default(),
            ActorId::from("server1.conn0.watcher9"),
        );
        front
            .set_blocked_urls(&mut transport, &urls)
            .expect("set_blocked_urls");

        let req = server.join().expect("stub thread");
        assert_eq!(req["to"], "server1.conn0.networkParent9");
        assert_eq!(req["type"], "setBlockedUrls");
        assert_eq!(req["urls"], json!(["favicon", "*.png"]), "{req}");
    }

    #[test]
    fn echo_lands_under_network_conditions() {
        let mut results = json!({"url": "https://example.com/"});
        insert_echo(&mut results, Some(json!({"throttle": "slow-3g"})));
        assert_eq!(results["network_conditions"]["throttle"], "slow-3g");
        let mut untouched = json!({"url": "x"});
        insert_echo(&mut untouched, None);
        assert!(untouched.get("network_conditions").is_none());
    }
}
