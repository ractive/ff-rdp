use ff_rdp_core::{
    ConsoleResource, ProtocolError, RdpTransport, TabActor, WatcherActor, WebConsoleActor,
    parse_console_notification, parse_console_resources,
};
use serde_json::{Value, json};

use crate::cli::args::Cli;
use crate::error::AppError;
use crate::output;
use crate::output_controls::{OutputControls, SortDir};
use crate::output_pipeline::OutputPipeline;

use super::connect_tab::{ConnectedTab, connect_and_get_target};

pub fn run(cli: &Cli, level: Option<&str>, pattern: Option<&str>) -> Result<(), AppError> {
    let mut ctx = connect_and_get_target(cli)?;
    let console_actor = ctx.target.console_actor.clone();

    // Start listeners — best-effort; some Firefox builds reject certain listener types.
    if let Err(e) = WebConsoleActor::start_listeners(
        ctx.transport_mut(),
        &console_actor,
        &["PageError", "ConsoleAPI"],
    ) {
        eprintln!("warning: startListeners failed: {e}");
    }

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
            eprintln!(
                "debug: getCachedMessages(PageError+ConsoleAPI) failed ({e}), retrying with ConsoleAPI only"
            );
            WebConsoleActor::get_cached_messages(
                ctx.transport_mut(),
                &console_actor,
                &["ConsoleAPI"],
            )
            .map_err(AppError::from)?
        }
    };

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
        controls.apply_sort(&mut results);
    }
    let (limited, total, truncated) = controls.apply_limit(results, Some(50));
    let shown = limited.len();
    let limited = controls.apply_fields(limited);

    let meta = json!({"host": cli.host, "port": cli.port});
    let envelope =
        output::envelope_with_truncation(&json!(limited), shown, total, truncated, &meta);

    OutputPipeline::from_cli(cli)?
        .finalize(&envelope)
        .map_err(AppError::from)
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
    // Activate the console actor's internal listeners before subscribing via
    // the Watcher.  Firefox requires the console actor to be "listening" for
    // the watcher's console-message subscription to deliver events; without
    // this call, console.log() calls made via eval produce no events.
    // Best-effort: some Firefox builds reject certain listener types.
    let console_actor = ctx.target.console_actor.clone();
    if let Err(e) = WebConsoleActor::start_listeners(
        ctx.transport_mut(),
        &console_actor,
        &["PageError", "ConsoleAPI"],
    ) {
        eprintln!("warning: startListeners failed: {e}");
    }

    let tab_actor = ctx.target_tab_actor().clone();
    let watcher_actor =
        TabActor::get_watcher(ctx.transport_mut(), &tab_actor).map_err(AppError::from)?;

    WatcherActor::watch_resources(
        ctx.transport_mut(),
        &watcher_actor,
        &["console-message", "error-message"],
    )
    .map_err(AppError::from)?;

    let result = follow_loop(ctx.transport_mut(), level, regex, jq_filter);

    // Best-effort cleanup — ignore errors since we may be exiting anyway.
    let _ = WatcherActor::unwatch_resources(
        ctx.transport_mut(),
        &watcher_actor,
        &["console-message", "error-message"],
    );

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

    let result = follow_loop(ctx.transport_mut(), level, regex, jq_filter);

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
/// Firefox delivers console messages via two channels:
///
/// 1. **Watcher stream** (`resources-available-array`): new console messages
///    generated by page scripts that the Watcher actor observes.
///
/// 2. **Direct console actor push** (`consoleAPICall` / `pageError`): Firefox
///    149+ pushes these directly to the console actor when `startListeners` is
///    active.  This path fires when `console.log()` is called via
///    `evaluateJSAsync`, so without handling it the follow mode silently drops
///    all eval-triggered log output.
///
/// Both channels must be handled to ensure complete coverage.
fn follow_loop(
    transport: &mut RdpTransport,
    level: Option<&str>,
    regex: Option<&regex::Regex>,
    jq_filter: Option<&str>,
) -> Result<(), AppError> {
    use std::io::Write;

    loop {
        match transport.recv() {
            Ok(msg) => {
                let msg_type = msg.get("type").and_then(Value::as_str).unwrap_or_default();

                // Collect resources from whichever channel delivered this message.
                let resources: Vec<ConsoleResource> = if msg_type == "resources-available-array" {
                    // Watcher stream: batch of console/error-message resources.
                    parse_console_resources(&msg)
                } else if let Some(notification) = parse_console_notification(&msg) {
                    // Direct push from the console actor (consoleAPICall / pageError).
                    // Convert ConsoleMessage → ConsoleResource so both paths share
                    // the same filtering and emission logic below.
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
                    // Unrecognised message type — skip silently.
                    continue;
                };

                for res in resources {
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

#[cfg(test)]
mod tests {
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
