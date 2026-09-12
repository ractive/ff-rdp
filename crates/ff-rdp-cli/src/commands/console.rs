use ff_rdp_core::{
    ConsoleResource, ProtocolError, RdpTransport, TabActor, WatcherActor, WebConsoleActor,
    parse_console_notification, parse_console_resources,
};
use serde_json::{Value, json};

use crate::cli::args::Cli;
use crate::error::AppError;
use crate::hints::{HintContext, HintSource};
use crate::output;
use crate::output_controls::{OutputControls, SortDir};
use crate::output_pipeline::OutputPipeline;

use super::connect_tab::{ConnectedTab, connect_and_get_target};

pub fn run(cli: &Cli, level: Option<&str>, pattern: Option<&str>) -> Result<(), AppError> {
    let mut ctx = connect_and_get_target(cli)?;
    let console_actor = ctx.target.console_actor.clone();

    // Prime the server-side message cache before reading it. Firefox's
    // WebConsole actor only records messages into the cache that
    // `getCachedMessages` reads *after* `startListeners` has been called on
    // that actor (see kb/rdp/actors/console.md). On a fresh --no-daemon
    // connection the listeners have never been started, so without this call
    // `getCachedMessages` legitimately returns nothing — even for a
    // `console.log` an earlier `ff-rdp eval` just emitted.
    prime_console_cache(cli, ctx.transport_mut(), &console_actor);

    // Retrieve all cached console messages.
    // If the combined request fails (Firefox may reject PageError serialization),
    // fall back to ConsoleAPI-only to recover partial results.
    let messages = match WebConsoleActor::get_cached_messages(
        ctx.transport_mut(),
        &console_actor,
        &["PageError", "ConsoleAPI"],
    ) {
        Ok(msgs) => msgs,
        Err(e) => {
            if cli.is_verbose() {
                // stderr-ok: (b) debug/diagnostic, gated on --verbose.
                eprintln!(
                    "debug: getCachedMessages(PageError+ConsoleAPI) failed ({e}), retrying with ConsoleAPI only"
                );
            }
            WebConsoleActor::get_cached_messages(
                ctx.transport_mut(),
                &console_actor,
                &["ConsoleAPI"],
            )
            .map_err(AppError::from)?
        }
    };

    // Track pre-filter count for summary.
    let raw_total = messages.len();

    // Apply filters.
    let regex = pattern
        .map(|p| {
            regex::RegexBuilder::new(p)
                .size_limit(1_000_000)
                .build()
                .map_err(|e| AppError::User(format!("invalid --pattern regex: {e}")))
        })
        .transpose()?;

    let filtered: Vec<_> = messages
        .into_iter()
        .filter(|msg| {
            if let Some(l) = level
                && !msg.level.eq_ignore_ascii_case(l)
            {
                return false;
            }
            if let Some(ref re) = regex
                && !re.is_match(&msg.message)
            {
                return false;
            }
            true
        })
        .collect();

    // Compute per-level counts over the filtered set (before --limit truncation).
    let mut by_level: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
    for msg in &filtered {
        *by_level.entry(msg.level.clone()).or_insert(0) += 1;
    }
    let matched = filtered.len();

    // Convert to JSON output.
    let mut results: Vec<serde_json::Value> = filtered
        .iter()
        .map(|msg| {
            json!({
                "level": msg.level,
                "message": msg.message,
                "source": msg.source,
                "line": msg.line,
                "timestamp": msg.timestamp,
            })
        })
        .collect();

    // Apply output controls: default sort timestamp desc, default limit 50.
    let controls = OutputControls::from_cli(cli, SortDir::Desc);
    if cli.sort.is_none() {
        let dir = controls.sort_dir;
        results.sort_by(|a, b| {
            let ta = a["timestamp"].as_f64().unwrap_or(0.0);
            let tb = b["timestamp"].as_f64().unwrap_or(0.0);
            let cmp = ta.partial_cmp(&tb).unwrap_or(std::cmp::Ordering::Equal);
            match dir {
                SortDir::Asc => cmp,
                SortDir::Desc => cmp.reverse(),
            }
        });
    } else {
        controls.apply_sort(&mut results)?;
    }
    controls.validate_fields(&results)?;
    let (limited, total, truncated) = controls.apply_limit(results, Some(50));
    let shown = limited.len();
    let limited = controls.apply_fields(limited);

    let mut meta = json!({});
    crate::connection_meta::merge_into_if_verbose(
        &mut meta,
        &cli.host,
        cli.port,
        None,
        cli.is_verbose(),
    );
    // iter-134: always present, not gated by --verbose — an
    // agent can tell how this command executed without a
    // separate `daemon status` round-trip.
    crate::connection_meta::merge_route(&mut meta, ctx.via_daemon);
    let mut envelope =
        output::envelope_with_truncation(&json!(limited), shown, total, truncated, &meta);

    // Capture error count before consuming by_level.
    let error_count = by_level.get("error").copied().unwrap_or(0);

    // Insert summary: pre-filter total, post-filter matched, shown after --limit,
    // and per-level counts over the filtered (but not truncated) set.
    if let Some(obj) = envelope.as_object_mut() {
        let by_level_json: serde_json::Map<String, serde_json::Value> =
            by_level.into_iter().map(|(k, v)| (k, json!(v))).collect();
        obj.insert(
            "summary".to_string(),
            json!({
                "total": raw_total,
                "matched": matched,
                "shown": shown,
                "by_level": by_level_json,
            }),
        );
    }

    if total == 0
        && let Some(obj) = envelope.as_object_mut()
    {
        obj.insert(
            "hint".to_string(),
            json!(
                "No console messages captured. Use --follow to stream live messages, \
                 or generate some with: ff-rdp eval 'console.log(\"test\")'"
            ),
        );
    }

    let hint_ctx = HintContext::new(HintSource::Console).with_has_errors(error_count > 0);
    OutputPipeline::from_cli(cli)?.finalize_with_hints(&envelope, Some(&hint_ctx))
}

/// Prime the WebConsole actor's message cache so a following
/// `getCachedMessages` sees `console.log` / page-error output that was emitted
/// before this connection was opened.
///
/// Firefox only records messages into the cache that `getCachedMessages` reads
/// *after* `startListeners` has run on the target's console actor (see the
/// `getCachedMessages` / `startListeners` sections of
/// `kb/rdp/actors/console.md`). A fresh `--no-daemon` `ff-rdp console`
/// invocation has never started listeners, so without this call the very first
/// `getCachedMessages` legitimately returns an empty set — even for a message
/// an earlier `ff-rdp eval 'console.log(...)'` just logged.
///
/// This is best-effort: `startListeners` is a legacy path that some Firefox
/// builds answer differently, and a failure here must not abort the read (a
/// primed-but-empty cache and a start-listeners error are both handled by the
/// subsequent `getCachedMessages`). Failures are surfaced only under
/// `--verbose`. This does NOT double-deliver against the `--follow` watcher
/// path: the `console` command and the script runner's `assert_no_console_errors`
/// step (`run_get_errors`) are short-lived single reads that never subscribe to
/// the watcher bus (`live_console_no_double_delivery` covers the combined case).
fn prime_console_cache(
    cli: &Cli,
    transport: &mut ff_rdp_core::RdpTransport,
    console_actor: &ff_rdp_core::ActorId,
) {
    if let Err(e) =
        WebConsoleActor::start_listeners(transport, console_actor, &["PageError", "ConsoleAPI"])
        && cli.is_verbose()
    {
        // stderr-ok: (b) debug/diagnostic, gated on --verbose.
        eprintln!("debug: startListeners(PageError+ConsoleAPI) failed ({e}); reading cache anyway");
    }
}

/// Return all cached console error messages as a JSON array.
///
/// Used by the script runner's `assert_no_console_errors` step.  Returns
/// only messages with `level == "error"`.
pub fn run_get_errors(cli: &Cli) -> Result<Vec<serde_json::Value>, crate::error::AppError> {
    let mut ctx = connect_and_get_target(cli)?;
    let console_actor = ctx.target.console_actor.clone();

    // Prime the server-side cache — see the note in `run`. Without a prior
    // `startListeners`, `getCachedMessages` returns nothing on a fresh
    // connection, so `assert_no_console_errors` would silently pass even when
    // the page had logged errors before this connection was opened.
    prime_console_cache(cli, ctx.transport_mut(), &console_actor);

    let messages = match WebConsoleActor::get_cached_messages(
        ctx.transport_mut(),
        &console_actor,
        &["PageError", "ConsoleAPI"],
    ) {
        Ok(msgs) => msgs,
        Err(e) => {
            if cli.is_verbose() {
                // stderr-ok: (b) debug/diagnostic, gated on --verbose.
                eprintln!(
                    "debug: getCachedMessages(PageError+ConsoleAPI) failed ({e}), retrying with ConsoleAPI only"
                );
            }
            WebConsoleActor::get_cached_messages(
                ctx.transport_mut(),
                &console_actor,
                &["ConsoleAPI"],
            )
            .map_err(crate::error::AppError::from)?
        }
    };

    let errors: Vec<serde_json::Value> = messages
        .into_iter()
        .filter(|m| m.level.eq_ignore_ascii_case("error"))
        .map(|m| {
            serde_json::json!({
                "level": m.level,
                "message": m.message,
                "source": m.source,
                "line": m.line,
            })
        })
        .collect();

    Ok(errors)
}

/// Stream console messages in real time until the connection is closed.
///
/// Subscribes to `console-message` and `error-message` resource types via the
/// WatcherActor (direct mode) or daemon stream protocol (daemon mode), then
/// loops reading events and printing each matching message as a compact JSON
/// line (NDJSON format) to stdout.
///
/// Exits cleanly when the connection is closed (e.g. Firefox exits or the
/// daemon is killed). Ctrl-C terminates the process, which is acceptable.
pub fn run_follow(cli: &Cli, level: Option<&str>, pattern: Option<&str>) -> Result<(), AppError> {
    let mut ctx = connect_and_get_target(cli)?;

    let regex = pattern
        .map(|p| {
            regex::RegexBuilder::new(p)
                .size_limit(1_000_000)
                .build()
                .map_err(|e| AppError::User(format!("invalid --pattern regex: {e}")))
        })
        .transpose()?;

    if ctx.via_daemon {
        run_follow_daemon(&mut ctx, level, regex.as_ref(), cli.jq.as_deref())
    } else {
        run_follow_direct(&mut ctx, level, regex.as_ref(), cli.jq.as_deref())
    }
}

fn run_follow_direct(
    ctx: &mut ConnectedTab,
    level: Option<&str>,
    regex: Option<&regex::Regex>,
    jq_filter: Option<&str>,
) -> Result<(), AppError> {
    let tab_actor = ctx.target_tab_actor().clone();

    // iter-252: `console-message` and `error-message` are both
    // `FrameTargetResources` (`devtools/server/actors/resources/index.js`),
    // i.e. emitted from the content process by a per-frame target actor —
    // exactly the class iteration 174 found starving on the direct route.
    //
    // Two server-side preconditions have to hold before a single one arrives,
    // and this call site used to satisfy neither:
    //
    //  * `isServerTargetSwitchingEnabled: true`. Without it
    //    `shouldNotifyWindowGlobal` rejects the top-level browsing context
    //    (`watcher/browsing-context-helpers.sys.mjs:174-182`), so the watcher
    //    never instantiates a frame target for the page.
    //  * `watchTargets("frame")` before `watchResources`. The content-process
    //    half of `watchResources` fans the new resource types out over
    //    `watcherDataObject.actors`
    //    (`js-process-actor/DevToolsProcessChild.sys.mjs:409-414`) — the
    //    targets `watchTargets` created. The top-level target obtained from
    //    the descriptor's `getTarget` is deliberately *not* in that list
    //    (only web extensions get a `TargetActorRegistry` fallback), so with
    //    an empty list the subscription reaches nobody.
    //
    // The daemon route has always done both in `establish_watcher`, which is
    // why it was unaffected. The `get_watcher_with_options` CAUTION about the
    // flag moving top-level target delivery onto the watcher does not bite
    // here: `follow_loop` never touches the target actor, it only reads
    // events off the transport.
    let watcher_actor =
        TabActor::get_watcher_with_options(ctx.transport_mut(), &tab_actor, Some(true))
            .map_err(AppError::from)?;

    // Both subscription requests can emit catch-up events before their ACK.
    // recv_reply_from forwards those to the sink; retain them in wire order
    // and drain them before reading newer events from the socket.
    let (events_tx, events_rx) = std::sync::mpsc::channel();
    let previous_sink = ctx.transport_mut().swap_event_sink(Some(events_tx));
    let subscribed = (|| {
        WatcherActor::watch_targets(ctx.transport_mut(), &watcher_actor, "frame")?;
        WatcherActor::watch_resources(
            ctx.transport_mut(),
            &watcher_actor,
            &["console-message", "error-message"],
        )
    })();
    ctx.transport_mut().set_event_sink(previous_sink);
    subscribed.map_err(AppError::from)?;

    let result = follow_loop(
        ctx.transport_mut(),
        events_rx.try_iter(),
        level,
        regex,
        jq_filter,
        true,
    );

    // Best-effort cleanup — ignore errors since we may be exiting anyway.
    let _ = WatcherActor::unwatch_resources(
        ctx.transport_mut(),
        &watcher_actor,
        &["console-message", "error-message"],
    );
    let _ = WatcherActor::unwatch_targets(ctx.transport_mut(), &watcher_actor, Some("frame"), None);

    result
}

fn run_follow_daemon(
    ctx: &mut ConnectedTab,
    level: Option<&str>,
    regex: Option<&regex::Regex>,
    jq_filter: Option<&str>,
) -> Result<(), AppError> {
    use crate::daemon::client::{start_daemon_stream, stop_daemon_stream};

    start_daemon_stream(ctx.transport_mut(), "console-message").map_err(AppError::from)?;
    start_daemon_stream(ctx.transport_mut(), "error-message").map_err(AppError::from)?;

    let result = follow_loop(
        ctx.transport_mut(),
        std::iter::empty(),
        level,
        regex,
        jq_filter,
        false,
    );

    // Best-effort cleanup — ignore errors since we may be exiting anyway.
    let _ = stop_daemon_stream(ctx.transport_mut(), "console-message");
    let _ = stop_daemon_stream(ctx.transport_mut(), "error-message");

    result
}

/// Inner loop: read events from the transport and emit matching console
/// messages as compact JSON lines (NDJSON).
///
/// Each message is a single compact JSON object on its own line so that
/// consumers can process the stream with tools like `jq` or `jq -c`.
/// If `jq_filter` is set, it is applied to each message before printing.
///
/// Firefox delivers console messages via the watcher's
/// `resources-available-array` stream.  In direct-follow mode the caller
/// issues `watchResources(console-message, error-message)` before invoking
/// `follow_loop`; in daemon-follow mode the equivalent subscription is set
/// up by `start_daemon_stream(...)`.  Either way, `follow_loop` reads the
/// resulting watcher frames off the transport.  We also accept legacy
/// `consoleAPICall` / `pageError` pushes defensively in case a server build
/// emits them without an explicit `startListeners` call — see iter-71c.
fn follow_loop(
    transport: &mut RdpTransport,
    mut catch_up: impl Iterator<Item = Value>,
    level: Option<&str>,
    regex: Option<&regex::Regex>,
    jq_filter: Option<&str>,
    release_grips: bool,
) -> Result<(), AppError> {
    use std::io::Write;

    let mut deliveries = ConsoleDeliveries::default();
    loop {
        match catch_up.next().map_or_else(|| transport.recv(), Ok) {
            Ok(msg) => {
                let msg_type = msg.get("type").and_then(Value::as_str).unwrap_or_default();

                // Collect resources from whichever channel delivered this message.
                let resources = parse_follow_messages(&msg);
                // Identity is derived from protocol values, never by parsing
                // rendered text (which may itself be a literal JSON string).
                // Keep the original grips in the emitted output.
                let mut identity_event = msg.clone();
                remove_grip_actor_ids(&mut identity_event);
                let identities = parse_follow_messages(&identity_event);
                for (res, identity) in resources.into_iter().zip(identities) {
                    if deliveries.is_duplicate(&identity, msg_type == "resources-available-array") {
                        continue;
                    }
                    if let Some(l) = level
                        && !res.level.eq_ignore_ascii_case(l)
                    {
                        continue;
                    }
                    if let Some(re) = regex
                        && !re.is_match(&res.message)
                    {
                        continue;
                    }
                    let entry = json!({
                        "level": res.level,
                        "message": res.message,
                        "source": res.source,
                        "line": res.line,
                        "timestamp": res.timestamp,
                    });
                    if let Some(filter) = jq_filter {
                        let values =
                            output::apply_jq_filter(&entry, filter).map_err(AppError::from)?;
                        for v in values {
                            println!(
                                "{}",
                                serde_json::to_string(&v)
                                    .map_err(|e| AppError::Internal(e.into()))?
                            );
                        }
                    } else {
                        println!(
                            "{}",
                            serde_json::to_string(&entry)
                                .map_err(|e| AppError::Internal(e.into()))?
                        );
                    }
                    // Flush stdout so each message appears immediately in tail-like usage.
                    let _ = std::io::stdout().flush();
                }
                if release_grips {
                    release_follow_grips(transport, &msg).map_err(AppError::from)?;
                }
            }
            Err(ProtocolError::Timeout) => {
                // Normal poll timeout — keep waiting for more events.
            }
            Err(ProtocolError::RecvFailed(ref e))
                if e.kind() == std::io::ErrorKind::UnexpectedEof
                    || e.kind() == std::io::ErrorKind::ConnectionReset
                    || e.kind() == std::io::ErrorKind::BrokenPipe =>
            {
                // Connection closed cleanly (Firefox exited, daemon stopped, etc.).
                return Ok(());
            }
            Err(e) => return Err(AppError::from(e)),
        }
    }
}

/// Only the direct connection owns these actors. Daemon stream readers receive
/// copies of shared events and must leave their lifetime to the daemon.
fn release_follow_grips(transport: &mut RdpTransport, event: &Value) -> Result<(), ProtocolError> {
    if !matches!(
        event["type"].as_str(),
        Some("resources-available-array" | "consoleAPICall" | "pageError")
    ) {
        return Ok(());
    }
    let mut actors = std::collections::BTreeSet::new();
    collect_follow_grips(event, &mut actors);
    for actor in actors {
        // All three specs accept release. Object/longString send an ACK;
        // Firefox 155 SymbolActor.release destroys itself before protocol/Actor
        // can send its declared reply. Do not wait for it. The ordinary follow
        // receive loop consumes ACKs (which contain no console resources) and
        // events in wire order, without a second receiver or a lossy queue.
        if let Err(error) = transport.send(&json!({"to": actor, "type": "release"})) {
            if matches!(&error, ProtocolError::SendFailed(io) if matches!(io.kind(),
                std::io::ErrorKind::BrokenPipe | std::io::ErrorKind::ConnectionReset
                | std::io::ErrorKind::NotConnected))
            {
                // Disconnection frees the pool. Still emit any buffered
                // catch-up records before the receive loop observes EOF.
                return Ok(());
            }
            return Err(error);
        }
    }
    Ok(())
}

fn collect_follow_grips<'a>(value: &'a Value, actors: &mut std::collections::BTreeSet<&'a str>) {
    match value {
        Value::Object(fields) => {
            if matches!(
                value["type"].as_str(),
                Some("object" | "longString" | "symbol")
            ) && let Some(actor) = value["actor"].as_str()
            {
                actors.insert(actor);
            }
            // Previews contain further object/symbol grips; symbol names can
            // themselves be longString grips. They have independent lifetimes.
            for field in fields.values() {
                collect_follow_grips(field, actors);
            }
        }
        Value::Array(items) => {
            for item in items {
                collect_follow_grips(item, actors);
            }
        }
        _ => {}
    }
}

fn parse_follow_messages(event: &Value) -> Vec<ConsoleResource> {
    if event["type"] == "resources-available-array" {
        parse_console_resources(event)
    } else if let Some(notification) = parse_console_notification(event) {
        vec![ConsoleResource {
            level: notification.level,
            message: notification.message,
            source: notification.source,
            line: notification.line,
            column: notification.column,
            timestamp: notification.timestamp,
            resource_id: None,
        }]
    } else {
        Vec::new()
    }
}

/// Firefox allocates grips separately for legacy and resource delivery. Their
/// actor IDs identify those handles, not the console call. Retain every other
/// value (including string length/initial text and object previews), and never
/// alter strings that merely look like JSON or contain an actor's name.
/// `object/utils.js::createValueGrip` allocates actors only for objects, long
/// strings, and symbols. Symbol names can themselves be long-string grips.
fn remove_grip_actor_ids(value: &mut Value) {
    match value {
        Value::Object(fields) => {
            if matches!(
                fields.get("type").and_then(Value::as_str),
                Some("longString" | "object" | "symbol")
            ) {
                fields.remove("actor");
            }
            for field in fields.values_mut() {
                remove_grip_actor_ids(field);
            }
        }
        Value::Array(values) => {
            for item in values {
                remove_grip_actor_ids(item);
            }
        }
        _ => {}
    }
}

/// Pair the resource and legacy copies of one console call without collapsing
/// repeated calls on the same channel. Keep a bounded recent window for a
/// long-lived follow; timestamp-less messages cannot be identified safely.
#[derive(Default)]
struct ConsoleDeliveries {
    unmatched: std::collections::VecDeque<(ConsoleResource, bool)>,
}

impl ConsoleDeliveries {
    fn is_duplicate(&mut self, message: &ConsoleResource, resource_channel: bool) -> bool {
        if !message.timestamp.is_finite() || message.timestamp <= 0.0 {
            return false;
        }
        if let Some(index) = self.unmatched.iter().position(|(seen, channel)| {
            *channel != resource_channel
                && seen.timestamp.to_bits() == message.timestamp.to_bits()
                && seen.level == message.level
                && seen.message == message.message
                && seen.source == message.source
                && seen.line == message.line
                && seen.column == message.column
        }) {
            self.unmatched.remove(index);
            return true;
        }
        if self.unmatched.len() == 1024 {
            self.unmatched.pop_front();
        }
        self.unmatched
            .push_back((message.clone(), resource_channel));
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn console_identity_excludes_only_grip_handles() {
        let events: Vec<Value> = serde_json::from_str(include_str!(
            "../../tests/fixtures/console_follow_preformatted_events.json"
        ))
        .unwrap();
        let mut deliveries = ConsoleDeliveries::default();
        let mut emitted = Vec::new();
        for mut event in events {
            let channel = event["type"] == "resources-available-array";
            remove_grip_actor_ids(&mut event);
            for message in parse_follow_messages(&event) {
                if !deliveries.is_duplicate(&message, channel) {
                    emitted.push(message);
                }
            }
        }
        assert_eq!(emitted.len(), 8);
        let long = &emitted[2];
        assert!(long.message.contains("iter252-record:long:"));
        let mut distinct = long.clone();
        distinct.message = distinct.message.replace("10020", "10021");
        assert!(!deliveries.is_duplicate(long, true));
        assert!(
            !deliveries.is_duplicate(&distinct, false),
            "length remains significant"
        );
        distinct = long.clone();
        distinct.message = distinct.message.replace("long:", "other:");
        assert!(
            !deliveries.is_duplicate(&distinct, false),
            "initial text remains significant"
        );
        distinct = long.clone();
        distinct.timestamp += 1.0;
        assert!(
            !deliveries.is_duplicate(&distinct, false),
            "separate calls remain significant"
        );
        assert!(deliveries.is_duplicate(long, false));

        let symbol = &emitted[3];
        assert!(symbol.message.contains("iter252-record:symbol"));
        assert!(!deliveries.is_duplicate(symbol, true));
        let mut renamed = symbol.clone();
        renamed.message = renamed.message.replace("record:symbol", "record:other");
        assert!(
            !deliveries.is_duplicate(&renamed, false),
            "symbol names remain significant"
        );
        assert!(deliveries.is_duplicate(symbol, false));

        let object: Value = serde_json::from_str(&emitted[4].message).unwrap();
        let properties = &object["preview"]["ownProperties"];
        assert_eq!(properties["type"]["value"], "symbol");
        assert_eq!(properties["actor"]["value"], "user-data");
        assert_eq!(
            properties["nested"]["value"]["name"],
            "iter252-record:nested"
        );
        assert!(properties["nested"]["value"].get("actor").is_none());
        let named: Value = serde_json::from_str(&emitted[5].message).unwrap();
        assert_eq!(named["name"]["type"], "longString");
        assert!(named["name"].get("actor").is_none());

        // An ordinary logged string may contain literal JSON, including keys
        // named type/actor. It must not be parsed and normalized as a grip.
        let mut literal = Value::String(r#"{"type":"longString","actor":"literal"}"#.into());
        let original = literal.clone();
        remove_grip_actor_ids(&mut literal);
        assert_eq!(literal, original);
    }

    #[test]
    fn console_deliveries_pair_channels_and_preserve_repeated_calls() {
        let mut deliveries = ConsoleDeliveries::default();
        let mut message = ConsoleResource {
            level: "log".into(),
            message: "same".into(),
            source: "page.js".into(),
            line: 1,
            column: 1,
            timestamp: 1000.0,
            resource_id: None,
        };
        assert!(!deliveries.is_duplicate(&message, true));
        assert!(!deliveries.is_duplicate(&message, true));
        assert!(deliveries.is_duplicate(&message, false));
        assert!(deliveries.is_duplicate(&message, false));
        message.timestamp += 1.0;
        assert!(!deliveries.is_duplicate(&message, false));
        assert!(deliveries.is_duplicate(&message, true));
        message.timestamp = 0.0;
        assert!(!deliveries.is_duplicate(&message, false));
        assert!(!deliveries.is_duplicate(&message, true));
    }
    /// Verify that a normal pattern compiles successfully under the size limit.
    #[test]
    fn accepts_reasonable_regex() {
        let result = regex::RegexBuilder::new(r"(?i)error|warn")
            .size_limit(1_000_000)
            .build();
        assert!(result.is_ok());
    }

    /// Verify that a pattern exceeding a small compiled-regex size limit is rejected.
    #[test]
    fn rejects_oversized_regex() {
        let oversized = (0..100)
            .map(|i| format!("literal_{i}"))
            .collect::<Vec<_>>()
            .join("|");
        let result = regex::RegexBuilder::new(&oversized).size_limit(64).build();
        assert!(result.is_err(), "expected oversized pattern to be rejected");
    }
}
